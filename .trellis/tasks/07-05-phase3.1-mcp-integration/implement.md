# Phase 3.1 Implementation Plan — MCP stdio integration

> 实施计划。配合 [prd.md](./prd.md) + [design.md](./design.md) 使用。
> 6 个 round，每个 round 独立可 commit。

## Round 列表

### Round 1：settings.rs 加载层

**目标**：单独可编译、可单测的 settings 加载模块。

**改动**：
- 新建 `src-tauri/src/settings.rs`
  - `Settings` / `McpSettings` / `ChatMcpServer` struct
  - `Settings::load_from_disk(app: &AppHandle) -> Self`：读 `<app_data_dir>/settings.json`
  - `Settings::sanitize(&mut self)`：检查 id 重复，去重
  - `Settings::default()`：空 servers
  - unit tests：minimal deserialize / optional fields / sanitize 重复 id / 文件不存在 → default / JSON 损坏 → default
- `src-tauri/src/lib.rs`：在 `mod` 声明中加 `pub mod settings;`
- 不接入 `setup` 钩子（Round 6 再接）

**验证**：
```bash
cd src-tauri && cargo test settings::
cargo check
```

**Commit message**：`feat(phase3.1): settings.rs — settings.json loader for MCP server config`

---

### Round 2：mcp/types.rs 类型层

**目标**：纯数据结构 + 转换函数，无 IO，无并发。

**改动**：
- 新建 `src-tauri/src/mcp/mod.rs`：模块导出
- 新建 `src-tauri/src/mcp/types.rs`
  - `McpTool`：`{ name, description, input_schema, annotations: Option<Value> }`
  - `McpToolCallResult`：`{ content: String, is_error: bool, structured_content: Option<Value>, artifacts: Vec<String> }`
  - `McpServerState` 枚举：`Connecting | Connected | Error { message } | Disconnected`
  - `McpServerStatePayload`：`{ server_id, state }`，`#[serde(rename_all = "camelCase")]`
  - `tool_definition_from_mcp(server: &ChatMcpServer, tool: McpTool) -> ChatToolDefinition`：照搬 Kivio `mcp_tool_requires_confirmation` 逻辑
  - `parse_tool_result(value: Value) -> McpToolCallResult`：从 JSON-RPC response 提取 content / is_error / structured_content
  - `parse_mcp_name(name: &str) -> Option<(server_id, tool_name)>`：splitn(3, "__")
  - unit tests：
    - `chat_mcp_server_deserialize_minimal`
    - `chat_mcp_server_deserialize_with_optional_fields`
    - `tool_definition_from_mcp_basic`
    - `tool_definition_from_mcp_destructive_hint_sensitive`
    - `tool_definition_from_mcp_readonly_hint_not_sensitive`
    - `tool_definition_from_mcp_open_world_hint_sensitive`
    - `tool_definition_from_mcp_no_annotations_fallback_sensitive`
    - `parse_tool_result_is_error_true`
    - `parse_tool_result_is_error_false`
    - `parse_tool_result_structured_content`
    - `parse_tool_result_image_artifact`
    - `parse_mcp_name_valid`
    - `parse_mcp_name_invalid_no_prefix`
- `src-tauri/src/lib.rs`：加 `pub mod mcp;`

**验证**：
```bash
cd src-tauri && cargo test mcp::types::
cargo check
```

**Commit message**：`feat(phase3.1): mcp/types.rs — MCP protocol types + tool_definition_from_mcp`

---

### Round 3：mcp/client.rs stdio transport

**目标**：单个 stdio MCP server 的 client，含持久 session + reader_task + 单飞门闩 + 死连接重连。

**改动**：
- 新建 `src-tauri/src/mcp/client.rs`
  - `McpSession`：`{ child: tokio::process::Child, stdin: ChildStdin, pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>, server_info: Option<Value>, capabilities: Option<Value> }`
  - `StdioMcpClient`：
    - `new(server: ChatMcpServer, sink: Arc<dyn McpEventSink>, timeout_ms: u64) -> Self`
    - `async fn connect(&self) -> Result<Arc<Mutex<McpSession>>>`：单飞门闩，首次握手 + emit Connecting/Connected/Error
    - `async fn list_tools(&self) -> Result<Vec<McpTool>>`：connect() → tools/list
    - `async fn call_tool(&self, name, args) -> Result<McpToolCallResult>`：connect() → tools/call → parse_tool_result
    - `async fn disconnect(&self)`：take session, drop child
  - 握手流程：`initialize` → `notifications/initialized`
  - reader_task：从 stdout 行读 JSON，按 id 路由到 oneshot；Drop 时 reject 所有 pending
  - stderr_task：tail 最近 20 行（tracing::debug!）
  - 超时：tokio::time::timeout 包每个 request
  - 死连接重连：connect 失败 / list_tools 失败 / call_tool 失败时若是连接相关错误，清旧 session 再试一次
  - `McpEventSink` trait：`emit_server_state(server, state)` / `emit_disconnected(server_id)` / `app_handle() -> Option<&AppHandle>`
- `src-tauri/src/mcp/mod.rs`：`pub mod client;`
- unit tests（mock 子进程，用 `node -e` 或 `python -c` 模拟 MCP server）：
  - `stdio_handshake_then_list_tools`
  - `stdio_call_tool_returns_content`
  - `stdio_dead_connection_reconnects_once`
  - `stdio_timeout_does_not_retry`（30s 太长，测试用 100ms）
  - `stdio_stderr_tail_collected`
- `McpEventSink` 的 `()` impl（test sink）

**验证**：
```bash
cd src-tauri && cargo test mcp::client::
cargo check
```

**Commit message**：`feat(phase3.1): mcp/client.rs — StdioMcpClient with persistent session + reader_task`

---

### Round 4：mcp/manager.rs 多 server 协调

**目标**：McpManager 管 N 个 client，并发 list_tools，状态事件 emit。

**改动**：
- 新建 `src-tauri/src/mcp/manager.rs`
  - `McpManager`：
    - 持有 `Arc<Mutex<HashMap<String, Arc<StdioMcpClient>>>>`（懒初始化）
    - 持有 `Arc<Mutex<HashMap<String, McpServerState>>>`（状态缓存）
    - 持有 `AppHandle`
    - `new(app: AppHandle) -> Self`
    - `async fn list_all_tools(&self, settings: &Settings) -> Vec<ChatToolDefinition>`：并发对 enabled servers 调 list_tools，单 server 失败 warn 跳过
    - `async fn call_tool(&self, server_id, tool_name, args) -> Result<McpToolCallResult>`
    - `async fn disconnect_all(&self)`：drop 所有 client
    - `async fn list_server_states(&self) -> HashMap<String, McpServerState>`
  - `TauriEventSink`：实现 `McpEventSink`，调 `app.emit("mcp-server-state", payload)`
  - `McpManager::get_or_init_client(server: &ChatMcpServer) -> Arc<StdioMcpClient>`
- `src-tauri/src/mcp/mod.rs`：`pub mod manager; pub use manager::McpManager;`
- unit tests：
  - `manager_list_all_tools_concurrent`
  - `manager_list_all_tools_skips_failed_server`
  - `manager_call_tool_routes_to_correct_client`
  - `manager_call_tool_unknown_server_returns_error`

**验证**：
```bash
cd src-tauri && cargo test mcp::manager::
cargo check
```

**Commit message**：`feat(phase3.1): mcp/manager.rs — multi-server coordination + event sink`

---

### Round 5：agent loop 集成 + IPC commands

**目标**：把 MCP 接入 LLM 请求 / dispatch 路径，加 Tauri commands。

**改动**：
- `src-tauri/src/agent/loop_.rs`：
  - `run_round` prepare 阶段：调 `mcp_manager.list_all_tools(&settings)` 合并到 `tool_defs`
  - 需要拿到 `Arc<McpManager>` 和 `Arc<Mutex<Settings>>`（通过 `app.try_state`）
- `src-tauri/src/agent/rounds.rs`：
  - `dispatch_single` 加 `if tool_use.name.starts_with("mcp__")` 分支
  - 解析 server_id / tool_name，调 `mcp_manager.call_tool`
  - 失败 → `ToolOutput { content: format!("MCP tool error: {e}") }`
  - sensitive → 走 host.request_tool_approval（同 native 路径）
  - `dispatch_round` 签名加 `mcp_manager: &Arc<McpManager>` 参数
- `src-tauri/src/ipc/commands.rs`：
  - `list_mcp_servers` 命令：从 `Arc<Mutex<Settings>>` 读
  - `list_mcp_server_states` 命令：从 `Arc<McpManager>` 读
- `src-tauri/src/lib.rs`：
  - `setup` 钩子：`Settings::load_from_disk(app)` → `app.manage(Arc::new(Mutex::new(settings)))`
  - `setup` 钩子：`McpManager::new(app.handle())` → `app.manage(Arc::new(mcp_manager))`
  - `tauri::RunEvent::ExitRequested`：调 `mcp_manager.disconnect_all().await`
  - `invoke_handler!` 加 `list_mcp_servers, list_mcp_server_states`
- contract tests：`src-tauri/tests/ipc_payload_contract.rs` 加：
  - `mcp_server_state_payload_camel_case`
  - `chat_mcp_server_round_trip`

**验证**：
```bash
cd src-tauri && cargo test
cargo check
```

**Commit message**：`feat(phase3.1): integrate MCP into agent loop + add list_mcp_servers/state commands`

---

### Round 6：前端 mcpStore + StatusBar + 事件订阅

**目标**：前端订阅 mcp-server-state 事件，StatusBar 显示状态。

**改动**：
- 新建 `src/types/mcp.ts`：`McpServerState` / `ChatMcpServer` / `McpServerStatePayload` TS 类型
- 新建 `src/stores/mcpStore.ts`：
  - `Map<serverId, McpServerState>`
  - `setState(serverId, state)` / `clear()`
  - 启动时调 `invoke("list_mcp_servers")` + `invoke("list_mcp_server_states")` 初始化
- 修改 `src/hooks/useAgentEvents.ts`：加 `listen("mcp-server-state", ...)`，调 `mcpStore.setState`
- 新建 `src/components/StatusBar.tsx`：
  - 显示 `N connected / M error` 概要
  - hover 展开 server 列表（每行：name + state + error.message）
  - 无 MCP server 时显示 "No MCP servers"
- 修改 `src/App.tsx`：在主区域底部渲染 `<StatusBar />`

**验证**：
```bash
pnpm tsc --noEmit
pnpm build
```

**Commit message**：`feat(phase3.1): frontend — mcpStore + StatusBar + mcp-server-state event subscription`

---

## 验证命令汇总

### 单 round 验证

| Round | 命令 |
|---|---|
| 1 | `cd src-tauri && cargo test settings:: && cargo check` |
| 2 | `cd src-tauri && cargo test mcp::types:: && cargo check` |
| 3 | `cd src-tauri && cargo test mcp::client:: && cargo check` |
| 4 | `cd src-tauri && cargo test mcp::manager:: && cargo check` |
| 5 | `cd src-tauri && cargo test && cargo check` |
| 6 | `pnpm tsc --noEmit && pnpm build` |

### 整体验证（手测 AC1-AC8）

```bash
# 1. 启动 app
pnpm tauri dev

# 2. 写 settings.json
mkdir -p ~/Library/Application\ Support/com.smart-codeagent.dev/
cat > ~/Library/Application\ Support/com.smart-codeagent.dev/settings.json <<'EOF'
{
  "mcp": {
    "servers": [
      {
        "id": "filesystem",
        "name": "Filesystem (/tmp)",
        "enabled": true,
        "transport": "stdio",
        "command": "npx",
        "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
        "env": {},
        "enabledTools": []
      }
    ]
  }
}
EOF

# 3. 重启 app，看 tracing 日志
# 4. 在前端问 "你能读 /tmp 下的文件吗"
# 5. 验证 LLM 调 mcp__filesystem__read_file
# 6. 验证 StatusBar 显示 "1 connected"
# 7. 测试 server 启动失败：把 command 改成 "/no/such/binary"，重启
# 8. 测试死连接：启动后 kill 子进程，再发消息
# 9. 测试退出：关闭 app，ps aux | grep mcp
```

### 全量测试

```bash
cd src-tauri && cargo test
cargo check
cd .. && pnpm tsc --noEmit
pnpm build
```

## 风险文件 / 回滚点

| 文件 | 风险 | 回滚策略 |
|---|---|---|
| `src-tauri/src/agent/loop_.rs` | 改 prepare 阶段，可能影响 Phase 2 流程 | git revert 该文件改动 |
| `src-tauri/src/agent/rounds.rs` | 改 dispatch_single，可能影响 native tool 路径 | git revert 该文件改动 |
| `src-tauri/src/lib.rs` | setup 钩子 + 退出钩子，错则启动失败 | git revert 该文件改动 |
| `src-tauri/src/mcp/client.rs` | reader_task 死锁 / 子进程孤儿 | 删 `mcp/` 目录 + 回滚 loop_.rs/rounds.rs/lib.rs |

## Follow-up checks（task.py start 前）

- [ ] prd.md 6 个 Q 全部决策完毕 ✅
- [ ] design.md 涵盖架构 / 数据流 / 契约 / 权衡 / 风险 ✅
- [ ] implement.md 6 个 round 都有清晰改动 + 验证命令 ✅
- [ ] 用户 review 上述三份文档并 explicitly approve（这一步必须用户做）
- [ ] 检查 Kivio 参考代码可访问：`/Users/shentao/IdeaProjects/codeagent/kivio/src-tauri/src/mcp/`
- [ ] 检查 Phase 2 working tree 干净：`git status`
- [ ] 检查 Phase 2 全量测试通过：`cd src-tauri && cargo test`

## Out of Scope（与 prd.md 一致，这里复述便于实现时核对）

- HTTP / streamable_http transport
- OAuth / connector auth
- Cursor `.mcp.json` 导入
- Tool list 缓存
- Skill runtime
- Sub-agent
- 配置 UI（Phase 3.3）
- `save_settings` / `reload_settings` / `mcp_reload_server` 命令（Phase 3.3）
- `approval_policy` 设置项（Phase 3.3）
- stderr_tail 推前端（Phase 3.3 诊断面板）
