# Phase 3.1: MCP stdio integration

## Goal

让 smart-codeagent 能加载 MCP（Model Context Protocol）server，把 server 暴露的
tools 合并进现有 ToolRegistry，让 LLM 能像调 native tool 一样调 MCP tool。

用户价值：装一个 MCP server（如 `@modelcontextprotocol/server-filesystem`）后，
LLM 自动获得该 server 提供的能力（如读外部目录、查 GitHub issue、查数据库），
无需改 native 代码。

## 参考

直接对标 Kivio `src-tauri/src/mcp/`（`/Users/shentao/IdeaProjects/codeagent/kivio`）
的瘦版。完整研究报告见 task 目录 `research/kivio-mcp-report.md`。

## Confirmed Facts（已通过代码 / 研究确认）

- smart-codeagent 现有架构：
  - `ToolRegistry`（`agent/tools/mod.rs:128`）持有 `Vec<Arc<dyn Tool>>`，启动时冻结
  - `ChatToolDefinition`（`agent/tools/mod.rs:110`）已设计 `source` + `server_id` 字段，
    Phase 2 注释明写 "Phase 4 加 MCP 时 source='mcp' + server_id 启用"
  - `dispatch_single`（`agent/rounds.rs`）按 `tool.name` 查 `ToolRegistry`，找到就 execute
  - Provider 已切到 OpenAI `/v1/chat/completions`，tools 字段用 `{type:"function",function:{name,description,parameters}}`
- Kivio MCP 核心设计（已研究）：
  - 持久连接池 `McpSession` + 配置指纹 + 单飞门闩（防并发重复握手）
  - stdio transport：`tokio::process::Child` + reader_task 按 JSON-RPC id 路由响应
  - 超时不重试、死连接透明重连一次（保护非幂等工具）
  - `kill_on_drop(true)` + Drop abort tasks + 退出钩子三道防线
  - 统一 `ChatToolDefinition`，dispatch 按 `source` 字段路由
- MCP 协议（2025-06-18）：
  - 握手：`initialize` → `notifications/initialized` → `tools/list`
  - 调用：`tools/call` → `content[]` + `isError`
  - id 命名空间：`mcp__{server_id}__{tool_name}` 防冲突

## Q1 决策：MCP server 配置加载方式

**选**：新建 `settings.json` 体系（Kivio 同款）。

**理由**：单一配置文件承载所有运行时设置，未来 Provider / API Key / Workspace 也都进这里。
比 mcp-servers.json 单独一文件更可扩展，比 .env 扩展更结构化，比 Tauri Store + 前端表单
更轻（Phase 3.1 还没到做 UI 的时候）。

**影响范围**：
- Phase 3.1 需新建 `config/settings.rs`（或扩展 `config.rs`）做 `settings.json` 的
  加载/反序列化/持久化骨架（仅 MCP server 配置部分，Provider 配置仍走 .env）。
- 前端表单 / 设置 UI 留到 Phase 3.3。
- 用户手编辑 `settings.json` 即可加 MCP server（与 Kivio 一致）。

## Out of Scope（Phase 3.1 不做）

- HTTP / streamable_http transport（只做 stdio）
- OAuth / connector auth（只支持无认证 stdio server）
- Cursor `.mcp.json` 导入（用户手填 settings.json）
- Tool list 缓存（每次 list 都拉，反正有持久会话很快）
- Skill runtime 集成（无 skill 系统）
- Sub-agent / mixer / image generation
- 配置 UI（Phase 3.3 设置面板统一做：表单编辑 settings.json、Provider 切换等）
- settings.json 中的非 MCP 字段（Provider 配置、Workspace、主题等 Phase 3.3 再加）

## Open Questions（需要用户决策）

### Q1：MCP server 配置加载方式 ✅ 已决策

新建 `settings.json` 体系（Kivio 同款）。

### Q2：settings.json 的运行时加载策略 ✅ 已决策

**选**：冷加载（对齐 Kivio）。

**Kivio 做法（已查证）**：
- 启动时 `load_settings_from_disk()` 读 `<app_data_dir>/settings.json` 一次入内存（`Settings` struct in AppState）。
- 没有 fs watcher。
- 配置变更只通过 `save_settings` Tauri command（前端表单触发）走 `apply_settings`（sanitize → 写内存 → 持久化）。
- `mcp_reload_server` 命令只丢会话池中某 server 的 session（重连按钮用），**不**重读 settings.json。

**Phase 3.1 映射**（无 UI）：
- 启动时读 `settings.json` 入内存 `McpSettings` struct。
- 用户改 settings.json → 重启 app 生效（Phase 3.1 唯一变更路径）。
- 不暴露 `reload_mcp_servers` / `reload_settings` command（Phase 3.3 设置面板自然会带出 `save_settings`）。
- 不引入 `notify` crate / fs watcher。

### Q3：MCP tool list 拉取时机 ✅ 已决策

**选**：每轮拉取（Kivio 同款）。

**Kivio 做法（已查证）**：
- `chat/agent/loop_.rs` 每轮 prepare 时调 `mcp::registry::list_enabled_tool_defs(app, state)`。
- 该函数对每个 enabled server 调 `state.mcp_list_tools(server)` → `client.list_tools()` → `session.request("tools/list", ...)`。
- 因为 session 是持久的（`McpSession` 连接池 + 单飞门闩），每轮只是一次本地 stdio JSON-RPC 往返，几十毫秒。
- **没有**单独的 tool list 缓存层。

**Phase 3.1 映射**：
- `agent/rounds.rs::dispatch` 每轮 prepare 阶段，遍历 `McpSettings.servers` 中 enabled 项，逐个调 `client.list_tools()` 拿 tool 列表。
- 拿到的 tool 与 native `ToolRegistry.definitions()` 合并后塞进 LLM 请求的 `tools` 字段。
- Tool 名用 `mcp__{server_id}__{tool_name}` 命名空间防冲突（与 Kivio 一致）。
- 不做缓存（Phase 3.1 已在 Out of Scope）。

### Q4：MCP tool 是否走 approval 流 ✅ 已决策

**选**：MCP annotations 智能判定（Kivio 同款）。

**Kivio 做法（已查证）**（`mcp/types.rs::mcp_tool_requires_confirmation` + `chat/agent/execute.rs::tool_requires_approval`）：
- `ChatToolDefinition.sensitive` 字段由 `mcp_tool_requires_confirmation(tool)` 计算：
  - `destructiveHint == true` → sensitive
  - `openWorldHint == true` → sensitive
  - `readOnlyHint == false` → sensitive
  - `readOnlyHint == true` → 不 sensitive
  - 无 annotations → fallback 到 `looks_sensitive_tool(&tool.name)` 启发式（按 tool 名关键词猜）
- dispatch 阶段 `tool_requires_approval(settings, tool)`：
  - `approval_policy = "auto"` → 永不 approve（Phase 3.1 不实现这个 policy）
  - `approval_policy = "always_confirm"` → 永远 approve（Phase 3.1 不实现）
  - 默认 → 看 `tool.sensitive` 标志

**Phase 3.1 映射**：
- `mcp::types::tool_definition_from_mcp()` 计算 `sensitive`：照搬 Kivio 的 `mcp_tool_requires_confirmation` 逻辑（destructive/openWorld/readOnly 三 hint + fallback 保守走 approval）。
- 简化：不实现 `approval_policy` 设置项（Phase 3.3 加），Phase 3.1 固定走默认分支（看 sensitive 标志）。
- MCP tool 与 native 工具共用 `agent::approval` 流（即 Phase 2 已有的 `request_tool_approval` 路径）。

### Q5：MCP server 启动失败 / tool call 失败兑底 ✅ 已决策

**选**：Kivio 同款。

**Kivio 做法（已查证）**：
- `mcp/registry.rs::list_enabled_tool_defs`：对每个 enabled server 调 `mcp_list_tools`，`Err(err)` → 仅 `eprintln!("MCP server {} failed while listing tools: {err}")` 并跳过该 server，**不让整轮 prepare 失败**。该 server 的 tools 不进 LLM 请求（LLM 自然看不到也调不到）。
- `mcp/client.rs::call_tool`：JSON-RPC `Err` → 整个 `call_tool()` 返回 `Err(String)`，dispatch 阶段把 error 写进 `tool_result` 回给 LLM，LLM 自己决定是否重试。
- MCP `isError: true`：`parse_tool_result` 提取 `is_error = true`，但 content 仍然回给 LLM（让 LLM 看到 server 自报的错误内容）。
- 死连接透明重连一次（Kivio `McpSession` 已有），仍失败 → 走 tool call 失败路径。
- stderr 收集到 `stderr_tail`（最近 20 行）用于诊断（Phase 3.1 可选，不必须）。

**Phase 3.1 映射**：
- `agent/rounds.rs::prepare`：对每个 enabled server 调 `client.list_tools()`，`Err` → `tracing::warn!` 跳过，整轮继续。
- `agent::dispatch` 处理 MCP tool call 失败：构造 `ToolOutput { content: format!("MCP tool error: {err}") }` 回给 LLM（与 Phase 2 native tool 失败路径一致）。
- MCP `isError: true` → content 正常回，`structured` 可选塞 `{"isError": true}` 标志（前端可显示 error icon，Phase 3.1 不强求）。
- stderr_tail：Phase 3.1 不实现（Phase 3.3 设置面板做诊断面板时再加）。

### Q6：是否推 mcp-server-state 事件给前端 ✅ 已决策

**选**：推事件 + StatusBar 显示。

**Kivio 做法（已查证）**（`mcp/manager.rs::McpEventSink`）：
- `McpServerState` 枚举：`Connecting | Connected | Error { message } | Disconnected`，`#[serde(tag = "kind", rename_all = "lowercase")]`。
- `McpEventSink` trait：`emit_server_state(server, state)` / `emit_disconnected(server_id)`。
- 生产实现用 `AppHandle::emit("mcp-server-state", { serverId, state })`；测试用 `()` 空实现。
- 状态变化点：握手开始 → Connecting；握手成功 → Connected；握手失败 → Error；session drop / reap idle → Disconnected。

**Phase 3.1 映射**：
- 后端 `mcp::manager` 在状态变化时 emit `mcp-server-state` 事件，payload `{ serverId, state }`。
- 前端 `useAgentEvents.ts` 加订阅 `mcp-server-state`，把 server 状态塞 `mcpStore`（新建 Zustand store）。
- StatusBar 显示 MCP server 状态：`N connected / M error` 概要，hover 展开 server 列表。
- 详细列表 + 重连按钮：留 Phase 3.3 设置面板。

## Requirements

### R1：settings.json 加载（Q1 + Q2）

- 新建 `src-tauri/src/settings.rs`：定义 `Settings` struct（仅 MCP 字段，Provider 仍走 .env）。
  ```rust
  pub struct Settings {
      pub mcp: McpSettings,
  }
  pub struct McpSettings {
      pub servers: Vec<ChatMcpServer>,
  }
  pub struct ChatMcpServer {
      pub id: String,
      pub name: String,
      #[serde(default = "default_true")]
      pub enabled: bool,
      #[serde(default = "default_stdio")]
      pub transport: String, // Phase 3.1 固定 "stdio"
      pub command: String,
      #[serde(default)]
      pub args: Vec<String>,
      #[serde(default)]
      pub env: HashMap<String, String>,
      #[serde(default, skip_serializing_if = "Option::is_none")]
      pub cwd: Option<String>,
      /// 空表示全启用；非空表示白名单（按 MCP tool.name 过滤）
      #[serde(default)]
      pub enabled_tools: Vec<String>,
  }
  ```
- 启动时（`lib.rs::run` setup 钩子）调 `settings::load_from_disk()` 读 `<app_data_dir>/settings.json` 入 `Arc<Mutex<Settings>>`，managed state。
- 文件不存在 / JSON 损坏 → 用 `Settings::default()`（空 servers 列表），不 panic。
- `app_data_dir` 用 `app.path().app_data_dir()` 解析（macOS：`~/Library/Application Support/<bundle_id>/`）。
- 文件不存在时**不**自动创建空文件（避免开发期干扰手编辑）。

### R2：MCP client + 持久会话池（Q3 + Q5）

- 新建 `src-tauri/src/mcp/{mod,types,client,manager}.rs`。
- `mcp::types::McpTool`：MCP server 返回的 tool 描述（name/description/input_schema/annotations）。
- `mcp::types::McpToolCallResult`：`{ content: String, is_error: bool, structured_content: Option<Value> }`。
- `mcp::types::tool_definition_from_mcp(server, tool) -> ChatToolDefinition`：照搬 Kivio 计算 `sensitive`（destructiveHint/openWorldHint/readOnlyHint 三 hint + fallback 保守走 approval）。Tool 名 `mcp__{server_id}__{tool_name}`。
- `mcp::client::StdioMcpClient`：
  - `tokio::process::Child` + `kill_on_drop(true)`
  - `connect() -> Result<Arc<Mutex<McpSession>>>`：单飞门闩（`OnceCell` / `tokio::sync::OnceCell`），并发只握手一次
  - `list_tools() -> Result<Vec<McpTool>>`：调 `tools/list` JSON-RPC
  - `call_tool(name, args) -> Result<McpToolCallResult>`：调 `tools/call` JSON-RPC
  - 握手流程：`initialize` → `notifications/initialized` → 后续 RPC
  - reader_task：从 stdout 按行读 JSON，按 id 路由到 oneshot sender
  - stderr_task：收集最近 20 行 `stderr_tail`（仅日志，不推前端）
  - 超时：默认 30s（connect / list / call），不重试（保护非幂等工具）
  - 死连接透明重连一次：connect 失败时清旧 session 再连一次
  - `McpEventSink` trait：状态变化时调 `emit_server_state` / `emit_disconnected`
- `mcp::manager::McpManager`：
  - 持有 `HashMap<server_id, Arc<StdioMcpClient>>`（懒初始化）
  - `list_all_tools(settings) -> Vec<ChatToolDefinition>`：遍历 enabled servers，并发调 `list_tools`，单 server 失败仅 `tracing::warn!` 跳过
  - `call_tool(server_id, tool_name, args) -> Result<McpToolCallResult>`：定位 client → `call_tool`
- 退出钩子（`tauri::RunEvent::ExitRequested`）：drop 所有 client，触发 `kill_on_drop`。

### R3：agent loop 集成（Q3 + Q4 + Q5）

- `agent/loop_.rs::run_round`：
  - prepare 阶段调 `mcp_manager.list_all_tools(&settings)`，与 `ToolRegistry::definitions()` 合并后塞 LLM 请求 `tools` 字段。
  - dispatch 阶段：tool_use.name 以 `mcp__` 开头 → 走 `mcp_manager.call_tool`；否则走原 `ToolRegistry` 路径。
- MCP tool call 失败 → 构造 `ToolOutput { content: format!("MCP tool error: {err}") }`，与 Phase 2 native tool 失败路径一致。
- MCP `isError: true` → content 正常回，`structured: Some({"isError": true})`（前端可显示 error icon）。
- MCP tool 走 approval 流：sensitive=true 时调 `host.request_tool_approval`（与 native destructive 工具同路径，Phase 2 已实现）。

### R4：mcp-server-state 事件 + StatusBar 显示（Q6）

- `mcp/manager.rs::McpServerState` 枚举：`Connecting | Connected | Error { message } | Disconnected`，`#[serde(tag = "kind", rename_all = "lowercase")]`。
- 状态变化点：握手开始 → Connecting；握手成功 → Connected；握手失败 → Error；session drop → Disconnected。
- 后端 emit 事件名 `mcp-server-state`，payload `{ serverId: String, state: McpServerState }`。
- 前端 `useAgentEvents.ts` 加订阅 `mcp-server-state`。
- 新建 `src/stores/mcpStore.ts`（Zustand）：`Map<serverId, McpServerState>`。
- 新建 `src/components/StatusBar.tsx`：显示 `N connected / M error` 概要；hover 展开列表（每行：name + state + error.message）。

### R5：Tauri command：拉取当前 MCP 配置 / 状态（轻量）

- `list_mcp_servers()`：返回 `Vec<ChatMcpServer>`（来自内存 settings）。
- `list_mcp_server_states()`：返回 `Map<serverId, McpServerState>`（前端启动时拉一次拿初值，之后靠事件增量更新）。
- 不实现 `save_settings` / `reload_settings` / `mcp_reload_server`（留 Phase 3.3）。

## Acceptance Criteria

### AC1：手编 settings.json 加载生效

- 在 `<app_data_dir>/settings.json` 写入一个 stdio server 配置（如 `npx @modelcontextprotocol/server-filesystem /tmp`）。
- 启动 app → tracing 日志显示 `[mcp] server <id> connecting → connected`。
- `list_mcp_servers` 命令返回该 server。
- settings.json 不存在 / JSON 损坏 → app 正常启动，`list_mcp_servers` 返回空数组。

### AC2：MCP tool 进入 LLM tool 列表

- 启用 `@modelcontextprotocol/server-filesystem` 后，问 LLM "你能读 /tmp 下的文件吗"。
- 后端 tracing 日志显示 prepare 阶段拉到 N 个 MCP tools，与 native tools 合并塞进 LLM 请求。
- LLM 输出的 tool_call 名以 `mcp__filesystem__` 开头。

### AC3：MCP tool 可执行 + 结果回 LLM

- LLM 调 `mcp__filesystem__read_file` → 后端 dispatch 走 `mcp_manager.call_tool` → 返回 content。
- 后端 tracing 显示 `[mcp] call_tool server=<id> tool=<name> is_error=false`。
- LLM 收到 tool_result 后继续生成回复。

### AC4：sensitive MCP tool 走 approval

- 配置一个 destructive MCP tool（如自定义 echo server 在 annotations 标 `destructiveHint: true`）。
- LLM 调它 → 前端 ApprovalDialog 弹窗（与 native Write/Edit 同路径）。
- 用户 approve → 执行；reject → tool_result 写 "Tool call was not approved"，LLM 继续。

### AC5：server 启动失败不影响整轮

- 配置一个不存在的 command（如 `/no/such/binary`）。
- 启动后 tracing 显示 `[mcp] server <id> error: No such file or directory`。
- emit `mcp-server-state` 事件 payload `{ kind: "error", message: "..." }`。
- StatusBar 显示 `1 error`，hover 看到错误信息。
- 该 server 的 tools 不进 LLM 请求；其他 server / native 工具正常工作。

### AC6：tool call 失败回 LLM

- 配置一个会返回 `isError: true` 的 MCP tool（如 server 内部 assert）。
- LLM 调它 → tool_result content 为 server 返回的错误文本。
- LLM 收到后能继续生成（如告知用户"该工具调用失败"）。

### AC7：死连接透明重连

- 启动一个 MCP server，确认握手成功。
- 在 OS 层面 kill 该子进程（模拟死连接）。
- 下一轮 LLM 调该 server 的 tool → 后端 tracing 显示 `connection lost, reconnecting...` → 重连成功 → tool 正常执行。

### AC8：app 退出干净

- 启用 2 个 MCP server，确认都 Connected。
- 关闭 app → tracing 显示 `[mcp] disconnect_all: dropping 2 sessions`。
- `ps aux | grep mcp` 确认无孤儿子进程。

### AC9：测试

- `cargo test` 全绿（Phase 2 已有 63 lib + 13 contract tests 不能挂）。
- 新增 MCP 模块至少 8 个 unit test：
  - `chat_mcp_server_deserialize_minimal`
  - `chat_mcp_server_deserialize_with_optional_fields`
  - `tool_definition_from_mcp_basic`
  - `tool_definition_from_mcp_destructive_hint_sensitive`
  - `tool_definition_from_mcp_readonly_hint_not_sensitive`
  - `parse_tool_result_is_error_true`
  - `parse_tool_result_is_error_false`
  - `parse_tool_result_structured_content`
- 集成测试（mock stdio MCP server）：`stdio_handshake_then_list_tools`、`stdio_call_tool_returns_content`、`stdio_dead_connection_reconnects_once`。

### AC10：cargo check 干净 + 无新 warning

- `cargo check` 0 error。
- 新代码无 warning（dead_code / unused_imports 等）；Phase 2 遗留的 11 个 pre-existing warnings 不动。
