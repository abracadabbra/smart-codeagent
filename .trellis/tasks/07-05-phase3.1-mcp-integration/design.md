# Phase 3.1 Design — MCP stdio integration

> 技术设计文档。配合 [prd.md](./prd.md) 使用；prd 是 what/why，本文是 how。

## 1. 架构边界

### 1.1 新增模块（`src-tauri/src/`）

```
src-tauri/src/
├── settings.rs            ★ 新增：settings.json 加载 + Settings/ChatMcpServer struct
├── mcp/                   ★ 新增：MCP 模块
│   ├── mod.rs             模块导出
│   ├── types.rs           McpTool / McpToolCallResult / tool_definition_from_mcp
│   ├── client.rs          StdioMcpClient + McpSession（持久会话 + reader_task）
│   └── manager.rs         McpManager（多 server 协调 + McpServerState + EventSink）
├── agent/
│   ├── loop_.rs           修改：prepare 阶段合并 MCP tools，dispatch 阶段路由 mcp__ 前缀
│   └── rounds.rs          修改：dispatch_single 加 MCP 分支
├── ipc/
│   └── commands.rs        修改：加 list_mcp_servers / list_mcp_server_states 命令
└── lib.rs                 修改：setup 钩子加载 settings + manage McpManager + 退出钩子
```

### 1.2 前端新增 / 修改（`src/`）

```
src/
├── stores/
│   └── mcpStore.ts        ★ 新增：Zustand store，Map<serverId, McpServerState>
├── hooks/
│   └── useAgentEvents.ts  修改：加 mcp-server-state 事件订阅
├── components/
│   └── StatusBar.tsx      ★ 新增：MCP server 状态概要 + hover 列表
├── types/
│   └── mcp.ts             ★ 新增：McpServerState / ChatMcpServer TS 类型
└── App.tsx                修改：渲染 <StatusBar /> 在主区域底部
```

### 1.3 不动的部分

- `agent/tools/`：native 工具全不动。`ToolRegistry`、`ChatToolDefinition` 字段保持不变（已有 source/server_id 字段）。
- `providers/`：不动。LLM 请求结构未变（仍 OpenAI Chat Completions `tools` 字段）。
- `ipc/events.rs`：仅追加新事件 payload struct，不动现有。

## 2. 数据流

### 2.1 启动流程

```
lib.rs::run setup
  ├── settings::load_from_disk(app) → Arc<Mutex<Settings>>
  │     └── 文件不存在 / JSON 损坏 → Settings::default()
  ├── app.manage(settings)
  ├── McpManager::new(app_handle) → Arc<McpManager>
  │     └── 不在此处预连接（懒初始化，首次 list_tools 时才握手）
  └── app.manage(mcp_manager)

tauri::RunEvent::ExitRequested
  └── mcp_manager.disconnect_all().await
        └── 对每个 session: drop child（kill_on_drop 触发 SIGTERM）
```

### 2.2 单轮 LLM 请求流程（prepare 阶段）

```
loop_.rs::run_round prepare
  ├── native_defs = ToolRegistry::definitions()
  ├── mcp_defs = mcp_manager.list_all_tools(&settings).await
  │     ├── for each enabled server:
  │     │     ├── client = mcp_manager.get_or_init_client(server_id)
  │     │     │     └── 懒初始化：StdioMcpClient::new(server) + emit Connecting
  │     │     ├── tools = client.list_tools().await
  │     │     │     ├── 首次：connect() 握手 → emit Connecting → emit Connected
  │     │     │     ├── 后续：复用 session，仅一次 tools/list JSON-RPC
  │     │     │     └── Err → tracing::warn! 跳过 + emit Error
  │     │     └── tool_definition_from_mcp(server, tool) for each tool
  │     │           └── 计算 sensitive（annotations 三 hint + fallback）
  │     └── Vec<ChatToolDefinition>
  ├── all_defs = native_defs + mcp_defs
  └── provider.stream_chat(req with tools=all_defs)
```

### 2.3 tool call dispatch 流程

```
rounds.rs::dispatch_single(tool_use)
  ├── if tool_use.name.starts_with("mcp__"):
  │     ├── (server_id, tool_name) = parse_mcp_name(tool_use.name)
  │     ├── mcp_manager.call_tool(server_id, tool_name, tool_use.input).await
  │     │     ├── client = clients.get(server_id) → None → Err("server not configured")
  │     │     └── client.call_tool(tool_name, args)
  │     │           ├── 死连接 → 透明重连一次 → 重试一次
  │     │           ├── 超时 30s → Err
  │     │           └── Ok(McpToolCallResult { content, is_error, structured_content })
  │     ├── if tool_def.sensitive: host.request_tool_approval (与 native destructive 同路径)
  │     ├── Ok(res) → ToolOutput { content: res.content,
  │     │                        structured: if res.is_error {Some({"isError":true})} else {res.structured_content} }
  │     └── Err(e) → ToolOutput { content: format!("MCP tool error: {e}") }
  │
  └── else: 走原 ToolRegistry 路径（Phase 2 不变）
```

### 2.4 mcp-server-state 事件流

```
状态变化点（在 StdioMcpClient / McpManager 内）:
  ├── connect() 开始握手 → emit_server_state(Connecting)
  ├── 握手成功              → emit_server_state(Connected)
  ├── 握手失败              → emit_server_state(Error { message })
  └── session drop / disconnect_all → emit_disconnected(server_id)

AppHandle.emit("mcp-server-state", { serverId, state })
  ↓
useAgentEvents.ts::listen("mcp-server-state")
  ↓
mcpStore.setState(serverId, state)
  ↓
StatusBar.tsx 读取 mcpStore，重渲染
```

## 3. 关键数据契约

### 3.1 settings.json 文件格式（示例）

```json
{
  "mcp": {
    "servers": [
      {
        "id": "filesystem",
        "name": "Filesystem (tmp)",
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
```

- `enabledTools: []` 表示全启用（与 Kivio 一致）。
- 文件位置：`<app_data_dir>/settings.json`，macOS = `~/Library/Application Support/com.smart-codeagent.dev/settings.json`。

### 3.2 Tauri commands（新增 2 个）

```rust
#[tauri::command]
pub async fn list_mcp_servers(
    settings: State<'_, Arc<Mutex<Settings>>>,
) -> Result<Vec<ChatMcpServer>, String> { ... }

#[tauri::command]
pub async fn list_mcp_server_states(
    mcp: State<'_, Arc<McpManager>>,
) -> Result<HashMap<String, McpServerState>, String> { ... }
```

注册到 `tauri::generate_handler!`。

### 3.3 Tauri event payload

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]   // ← ipc-contracts.md 强制
pub struct McpServerStatePayload {
    pub server_id: String,            // → "serverId"
    pub state: McpServerState,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum McpServerState {
    Connecting,
    Connected,
    Error { message: String },
    Disconnected,
}
```

前端 TS：

```typescript
type McpServerState =
  | { kind: "connecting" }
  | { kind: "connected" }
  | { kind: "error"; message: string }
  | { kind: "disconnected" };

interface McpServerStatePayload {
  serverId: string;
  state: McpServerState;
}
```

### 3.4 MCP tool 命名空间

- 格式：`mcp__{server_id}__{tool_name}`（与 Kivio 一致）。
- server_id 取自 `ChatMcpServer.id`，要求用户保证全局唯一（sanitize 时检查重复）。
- 解析：splitn(3, "__")，得到 ["mcp", server_id, tool_name]。

### 3.5 MCP JSON-RPC 包（标准协议，2025-06-18）

```json
// initialize request
{ "jsonrpc": "2.0", "id": 1, "method": "initialize",
  "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "smart-codeagent", "version": "0.1.0" } } }

// initialize response (server → client)
{ "jsonrpc": "2.0", "id": 1, "result": { "protocolVersion": "...", "capabilities": {...}, "serverInfo": {...} } }

// initialized notification (client → server)
{ "jsonrpc": "2.0", "method": "notifications/initialized" }

// tools/list request
{ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }

// tools/call request
{ "jsonrpc": "2.0", "id": 3, "method": "tools/call",
  "params": { "name": "read_file", "arguments": { "path": "/tmp/foo.txt" } } }
```

## 4. 关键设计权衡

### 4.1 单飞门闩 vs 双重检查

- **选**：`tokio::sync::OnceCell<Arc<Mutex<McpSession>>>` per server。
- **理由**：第一个 awaiter 跑握手，其他 await 同一个 future。比 Mutex + 双重检查更标准、更不易出错。
- **代价**：OnceCell 失败后无法重试（必须 take + 重建）。Phase 3.1 用 `Mutex<Option<Arc<Mutex<McpSession>>>>` 替代以支持死连接重连。

### 4.2 reader_task 单线程 vs 多线程

- **选**：单 reader_task per session，按 JSON-RPC id 路由到 `HashMap<u64, oneshot::Sender>`。
- **理由**：MCP 协议是单工的，一个 reader 足够。多 reader 会乱序。
- **代价**：reader_task panic 后所有 in-flight oneshot 永久 pending。需在 reader_task Drop 时统一 reject 所有 pending。

### 4.3 超时不重试 vs 重试一次

- **选**：超时不重试（与 Kivio 一致）。
- **理由**：保护非幂等工具。`tools/call` 可能写数据库、删文件，盲目重试 = 数据损坏。
- **代价**：网络瞬时抖动会让用户重试。可接受（用户可以再发一次消息触发下一轮）。

### 4.4 死连接透明重连一次

- **选**：list_tools / call_tool 失败时，如果是连接相关错误（broken pipe / child exited），清旧 session 再连一次，再失败才返回 Err。
- **理由**：用户感知不到 server 进程崩溃，重连一次是 UX 兜底。
- **代价**：第一次 call_tool 失败后延迟翻倍（30s 超时 + 重连 + 30s 超时）。可接受。

### 4.5 lazy 连接 vs 启动时预连接

- **选**：lazy（首次 list_tools 时才握手）。
- **理由**：app 启动快，禁用的 server 不消耗资源，用户改配置不立即触发握手。
- **代价**：第一轮 LLM 请求延迟较高（每个 enabled server 多一次握手 100-500ms）。可接受。

## 5. 兼容性 & 迁移

### 5.1 向前兼容

- Phase 2 的 native 工具流不变。`ToolRegistry` / `ChatToolDefinition` / `dispatch_round` 接口签名兼容。
- 现有 13 个 contract tests + 63 个 lib tests 全部不能挂。

### 5.2 settings.json 不存在时的行为

- 文件不存在 → `Settings::default()`（空 servers）→ MCP 不工作，native 工具正常。
- JSON 损坏 → 同上 + tracing::warn。
- **不**自动创建空文件（避免开发期干扰）。

### 5.3 Rollback

- 实现分 6 个 round（见 implement.md），每个 round 独立可 commit。
- 若某 round 出问题，`git reset --hard <prev_round_commit>` 即可回退到上一个稳定点。
- 最坏情况：删 `mcp/` 整个目录 + 回滚 `loop_.rs` / `rounds.rs` / `lib.rs` 改动 → 回到 Phase 2 终态。

## 6. 风险点

### 6.1 子进程孤儿问题

- **场景**：app panic 退出，`kill_on_drop` 未触发。
- **缓解**：`tauri::RunEvent::ExitRequested` 钩子显式 drop 所有 client；macOS/Linux 下 `kill_on_drop` 走 SIGTERM，子进程默认会响应。
- **残留风险**：app 被 SIGKILL 杀掉时仍有孤儿。文档建议用户重启 OS 或手动 `pkill -f <command>`。

### 6.2 settings.json 并发写

- **场景**：Phase 3.1 用户手编辑文件时 app 还在读（每轮 prepare 都读内存 settings）。
- **缓解**：只在启动时读一次入内存，后续 prepare 读内存中的 `Arc<Mutex<Settings>>`。用户改文件后需重启 app 生效（Q2 决策）。

### 6.3 MCP server id 重复

- **场景**：用户在 settings.json 配两个同 id 的 server。
- **缓解**：`Settings::sanitize()` 检查 id 重复，重复时取第一个 + tracing::warn 跳过其他。

### 6.4 reader_task 死锁

- **场景**：reader_task 持 `HashMap<u64, oneshot::Sender>` 的锁时被 await 中断。
- **缓解**：用 `tokio::sync::Mutex` 保护 HashMap，临界区内只做 insert/remove，不做 await。

### 6.5 LLM 输出 mcp__ tool name 但 settings 已变

- **场景**：第一轮拉到 `mcp__filesystem__read_file`，用户重启 app 删了 filesystem server，第二轮 LLM（基于历史）又输出该 tool name。
- **缓解**：dispatch 时找不到 server → 返回 `ToolOutput { content: "MCP server 'filesystem' is no longer configured" }` 让 LLM 自适应。不 panic。
