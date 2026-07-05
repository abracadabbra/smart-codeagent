# Phase 2: 工具系统（Kivio 瘦版：native tools + tool dispatch loop + agent host）

## 背景

Phase 1 跑通了 LLM 流式输出，LLM 是"会说话但不做事"。Phase 2 给它装上手脚：让 LLM 在对话中主动调用 8 个本地工具（Read / Write / Edit / Bash / Glob / Grep / LS / AskUser），把"说"变成"做"。

设计**直接对标 Kivio（`/Users/shentao/IdeaProjects/codeagent/kivio`）的瘦版**：
- **复用**：native tools / `AgentHost` trait / tool dispatch loop / parallel round execution / `ToolCallRecord` / `AskUser` / Approval flow / `persist_partial_assistant` 模式
- **砍掉**：Lens / OCR / Screenshot / Translation / MCP / Skills / External Agents / Knowledge Base / Kivio Code CLI / Compaction Synthesis 真逻辑（Phase 3+ 再说，接口 stub 即可）

PRD 里每条设计决策都标注 `（借 Kivio <file>:<line>）`，方便对照源码。

## 目标

让 LLM 能读、写、编辑本地文件；执行受限 shell 命令（前台 + 后台）；做文件搜索（glob / grep / ls）；主动向用户提问（AskUser）。所有危险操作走 Approval 流，越界时不让 LLM 反复试探，而是 emit `agent:tool_rejected` 让前端展示原因 + 把"permission denied"作为 tool_result 回传。

## Scope（In）

### 1. 8 个内置工具（每个工具独立文件，Kivio files.rs / shell.rs / ask_user.rs 启发）

| 工具 | 输入 | 输出 | 借 |
|---|---|---|---|
| `read_file` | `{ path: string, max_bytes?: number }` | `{ content: string, total_bytes: number, truncated: bool }` | Kivio `files.rs:108` |
| `write_file` | `{ path: string, content: string }` | `{ bytes_written: number }` | Kivio `files.rs:255` |
| `edit_file` | `{ path: string, old_text: string, new_text: string, replace_all?: bool }` | `{ replacements: number }` | Kivio `files.rs:301` |
| `run_command` | `{ command: string, timeout_ms?: number, cwd?: string }` | `{ stdout: string, stderr: string, exit_code: i32, duration_ms: u64 }` | Kivio `shell.rs:63` |
| `bash_output` | `{ background_id: string, wait_ms?: number }` | `{ stdout: string, stderr: string, exit_code: i32, status: string }` | Kivio `shell.rs:537` |
| `kill_background` | `{ background_id: string }` | `{ killed: bool }` | Kivio `shell.rs:617` |
| `glob_files` | `{ pattern: string, cwd?: string }` | `{ paths: string[] }` | Kivio `files.rs:1074` |
| `search_files` | `{ pattern: string, path: string, max_results?: number, case_insensitive?: bool }` | `{ matches: [{ path: string, line: number, content: string }] }` | Kivio `files.rs:1124` |
| `list_dir` | `{ path: string }` | `{ entries: [{ name: string, kind: "file"\|"dir", size: number }] }` | Kivio `files.rs:1016` |
| `ask_user` | `{ questions: AskUserQuestion[] }` | `{ answers: Record<questionId, { selectedOptionIds?: string[], customText?: string }> }` | Kivio `ask_user.rs:46` |

合计 **10 个工具**（Kivio 是 7 个原生工具 + ask_user = 8，Phase 2 把 `kill_background` / `list_background` 留作 Phase 3）

### 2. Tool trait + ChatToolDefinition（Kivio `ChatToolDefinition` 形态）

```rust
// src-tauri/src/agent/tools/mod.rs
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> serde_json::Value;  // JSON Schema
    fn is_sensitive(&self) -> bool { false }       // 敏感工具需要 approval
    fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError>;
}

pub struct ChatToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub source: String,        // "native" 固定，Phase 4 加 "mcp"
    pub server_id: Option<String>,
    pub sensitive: bool,
}

pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}
```

### 3. AgentLoop 3-phase + 多轮 + round 内并行（Kivio `AgentPhase` / `loop_.rs:135` / `rounds.rs:36` 启发）

```
Idle → Prepare → ToolLoop (round 1: plan → execute tool_use N 个并行 → 收敛结果再次 plan → ...)
                └→ Synthesis (可选：tool 收敛后无再调 tool 的迹象，触发最终合成)
                └→ Plain (无 tool 的对话，纯文本流式)
       → Stop → Idle
```

- **`AgentState`** 加 `ToolLoop` / `Synthesis` / `Plain` 三个变体（前端同步）
- **每轮（round）** = 一次 LLM 调用 + N 个并行 tool 调用（`MAX_PARALLEL_TOOL_CALLS_PER_ROUND = 8`，对齐 Kivio 的 12 但更小）
- **退出条件**：LLM 不再产生 tool_use → 进入 `Synthesis`（如果有 tool 记录）或直接 `Stop`
- **退出安全**：anti-thrashing 计数 Kivio 有但 Phase 2 单轮不需要，留 `// TODO Phase 3` 占位

### 4. AgentHost trait 抽象前端 / 持久化交互（Kivio `host.rs:10` 启发）

```rust
// src-tauri/src/agent/host.rs
pub trait AgentHost: Send + Sync {
    fn emit_stream_delta(&self, run_id: &str, msg_id: &str, text: &str, reasoning: Option<&str>);
    fn emit_stream_done(&self, run_id: &str, msg_id: &str, reason: &str, full: &str);
    fn emit_tool_record(&self, run_id: &str, msg_id: &str, record: &ToolCallRecord);
    fn request_tool_approval(&self, ctx: &ToolExecutionContext, record: &ToolCallRecord) -> BoxFuture<bool>;
    fn request_user_response(&self, ctx: &ToolExecutionContext, prompt: AskUserPromptPayload) -> BoxFuture<AskUserResponseResult>;
    fn persist_partial_assistant(&self, msg_id: &str, records: &[ToolCallRecord], api_messages: &[serde_json::Value]);
    fn is_generation_active(&self, run_id: &str, generation: u64) -> bool;
}
```

Rust 侧实现 `TauriHost`（用 `AppHandle` emit + oneshot channel 等用户响应）；Phase 2 不持久化，`persist_partial_assistant` 走 in-memory `Mutex<HashMap<msg_id, PartialDraft>>`

### 5. 路径沙箱 + Bash 黑名单（Kivio `native_tools/mod.rs:87` / `shell.rs:18` 启发）

**沙箱（无 workspace 边界，Phase 2 简化）**：
- 相对路径 → `current_dir()` 解析
- 绝对路径 → `canonicalize()`（不存在也通过——为了支持 Write 新文件）
- `..` 允许（借 Kivio no-boundary 思路）
- 不存在的路径允许（Write 时正常）

**Bash 黑名单**（Kivio `COMMAND_DENYLIST` 思路，扩大覆盖）：
```rust
const COMMAND_DENYLIST: &[&str] = &[
    "sudo ",
    "rm -rf /",
    "rm -rf /*",
    ":(){ :|:& };:",
    "mkfs.",
    "dd if=/dev/zero",
    "> /dev/sd",
    "shutdown",
    "reboot",
    "halt",
    "poweroff",
    "chmod 777 /",
    ":(){:|:&};:",
];
```
匹配用 `lowered.contains(denied)`，hit 后返回 `Err("command blocked by safety policy")`，loop 转 `tool_rejected` 事件。

### 6. Approval 流（Kivio `execute.rs:200-280` 启发）

```rust
// 工具执行前的 4 级 gate
if tool.is_sensitive() {
    let approved = host.request_tool_approval(ctx, &record).await;
    if !approved { return "tool_rejected" }
}
if tool.is_destructive() {  // Write / Edit / Bash / Kill background
    let approved = host.request_tool_approval(ctx, &record).await;
    if !approved { return "tool_rejected" }
}
```

Phase 2 实现：sensitive + destructive 工具都走 approval；前端用 `agent:approval_request` 弹出 `<ApprovalDialog>`，用户批准/拒绝，结果走 oneshot channel 回传到 `request_tool_approval`。

### 7. Anthropic 协议扩展（Kivio `model/anthropic.rs` 启发）

- `MessagesRequest` 加 `tools: Vec<ChatToolDefinition>` 字段
- 请求 body 加 `"tools": [{ "name", "description", "input_schema" }]`
- SSE 解析扩展：`content_block_start` 类型支持 `tool_use`，`content_block_delta` 类型支持 `input_json_delta`（累积成 `input: serde_json::Value`）
- 当轮 assistant 消息序列化为 `[{type:"text", text:"..."}, {type:"tool_use", id, name, input}]`
- tool 执行的 `tool_result` 作为下一条 user message：`[{type:"tool_result", tool_use_id, content}]`
- `thinking_delta` 保持 Phase 1 的吞掉策略，不 forward

### 8. 前端 Preview Pane（Kivio `ToolCallBlock.tsx` + `AskUserBlock.tsx` 启发）

- 右栏从占位变实 `<PreviewPane />`
- 监听新事件：`agent:tool_record` / `agent:approval_request` / `agent:ask_user_prompt` / `agent:partial_assistant`
- 工具卡片 `<ToolCallCard>`：折叠展开，显示 name / args（syntax highlight JSON）/ status / duration / result preview
- AskUser 卡片 `<AskUserPromptCard>`：选项按钮 + 自定义输入框
- Approval 卡片 `<ApprovalDialog>`：modal 形式（用户必须选择批准/拒绝）

### 9. 多轮对话兼容

- `history: Vec<Message>` 维持 Phase 1 形态
- Loop 内部按 Anthropic schema 把 `tool_use` / `tool_result` 序列化为 multi-block content
- 前端不感知多 block——渲染层只关心文本流 + 工具事件

## Scope（Out）

- Lens / OCR / Screenshot / Translation（**用户明确不要**）
- Kivio Code CLI / TUI（`kivio_code/` 整个目录）
- MCP 客户端 / Server 桥接（Phase 4，trait 已留 source/server_id 字段）
- Skills 系统 / SkillCache（Phase 4）
- External Agents 包装（不在产品形态）
- Knowledge Base / Embedding（Phase 4+）
- Compaction 真逻辑（接口 stub 出 `emit_compaction_status`，实际 no-op）
- Sub-agent / Plan-mode
- `list_background`（杀 ProcessGroup 即可；查询 background 列表留 Phase 3）
- SQLite 持久化（`persist_partial_assistant` in-memory 即可，Phase 3 接 SQLite）
- Context Trim / 重试退避（Phase 3）
- 自定义工具插件机制（trait 留 `Box<dyn Tool>` 但不暴露配置文件加载）

## AC（验收标准，约 15 条）

- **AC1** `src-tauri/src/agent/{loop_,host,execute,rounds,stream,types}.rs` + `src-tauri/src/agent/tools/{mod,read,write,edit,bash,glob,grep,ls,ask_user,path,background}.rs` 共 17 个 Rust 文件
- **AC2** `Tool` trait 定义在 `tools/mod.rs`，含 `name / description / input_schema / is_sensitive / execute`；10 个工具全部实现
- **AC3** `ToolRegistry` 启动时构造，工具 `register` 后冻结；提供 `by_name(&str) -> Option<&dyn Tool>` 与 `definitions() -> Vec<ChatToolDefinition>` 两个查询方法
- **AC4** `AgentState` 加变体 `ToolLoop / Synthesis / Plain`；前端 `src/types/agent.ts` 同步；Rust `as_str()` 加 3 个分支
- **AC5** `AgentLoop` 3-phase：ToolLoop 内支持多轮（每轮 ≤ 8 个并行 tool）；最大轮数限制 = 8（防死循环）
- **AC6** `AgentHost` trait 化：`TauriHost` 实现，覆盖 7 个方法
- **AC7** `MessagesRequest` 加 `tools: Vec<ChatToolDefinition>`；Anthropic 请求 body 序列化 `tools` 字段；SSE 解析 `tool_use` + `input_json_delta` 累积
- **AC8** 路径沙箱：相对/绝对/`..` 都支持；`canonicalize` 失败也通过（Write 新文件）
- **AC9** Bash 黑名单：13 个前缀；每个独立测试
- **AC10** Approval 流：sensitive + destructive 工具都触发 `agent:approval_request`，前端 modal 响应
- **AC11** 新 IPC 事件（**新增 7 个**，共 11 个 payload）：
  - `agent:stream_delta` (替代 phase 1 的 `agent:token`，加 `reasoning_delta` 字段)
  - `agent:stream_done` (替代 phase 1 的 `agent:done`)
  - `agent:tool_record`（**新**）
  - `agent:approval_request`（**新**）
  - `agent:ask_user_prompt`（**新**）
  - `agent:ask_user_response`（**新**，前端→后端）
  - `agent:partial_assistant`（**新**）
  - `agent:compaction_status`（**新** stub，no-op emit）
  - `agent:status`（保留）
  - `agent:error`（保留）
  - `agent:tool_rejected`（**新**，覆盖 path / bash 两种拒绝）
- **AC12** Tauri Command 新增 2 个：`approve_tool(approval_id, allow: bool)` / `answer_ask_user(approval_id, answers: HashMap)`——通过 oneshot channel 回传到 host trait 的 await
- **AC13** `tests/ipc_payload_contract.rs` 扩展到 **13 个 payload**（新增 8 个，全过）
- **AC14** 单测（每个工具 + 关键组件独立测）：
  - `loop_tests.rs`（Kivio 同名）：3-phase 转换 + round 循环 + parallel tool
  - `tool_read.rs` / `tool_write.rs` / `tool_edit.rs` / `tool_bash.rs` / `tool_glob.rs` / `tool_grep.rs` / `tool_ls.rs` / `tool_ask_user.rs`
  - `tool_path.rs`：沙箱 4 场景
  - `tool_bash_denylist.rs`：13 个前缀
  - `tool_registry.rs`：注册 / 查找 / 未知工具
  - `host.rs`：`is_generation_active` / `persist_partial_assistant`
- **AC15** **手测 E2E**（你拍板，5 个 prompt 至少过 4 个）：
  - "Read the file src-tauri/src/lib.rs and summarize in 3 sentences" → LLM 调 read_file → 流式输出总结
  - "Write a hello.txt with content 'hi'" → LLM 调 write_file → 用户在 modal 点批准 → 文件落盘
  - "Find all TODO comments in src/" → LLM 调 search_files → 输出 grep 结果
  - "Run `cargo --version` and tell me the version" → LLM 调 run_command → 输出 cargo 版本
  - "Delete the project" → LLM 调 run_command `rm -rf /` → 前端看到 `tool_rejected` 卡片

## 改动文件

### Rust 后端（17 个新文件 + 7 个改动）

```
src-tauri/src/
├── lib.rs                       # 加 pub mod agent::tools; pub mod agent::{host,execute,rounds,stream,types};
├── config.rs                    # 不变
├── agent/
│   ├── mod.rs                   # 改：加 re-exports + AgentState 新变体
│   ├── loop_.rs                 # 改：3-phase 状态机 + round 循环（保留历史 API）
│   ├── host.rs                  # NEW: AgentHost trait + TauriHost impl
│   ├── execute.rs               # NEW: ToolExecutor trait + execute_tool_call + approval gate
│   ├── rounds.rs                # NEW: run_tool_round + parallel join + round limit
│   ├── stream.rs                # NEW: AgentStreamSink + ToolCallDraftTracker（Kivio 启发）
│   ├── types.rs                 # NEW: AgentRunConfig/Result, ToolCallRecord, ToolCallStatus, ToolExecutionContext
│   └── tools/
│       ├── mod.rs               # NEW: Tool trait + ToolRegistry + ChatToolDefinition + ToolContext
│       ├── read.rs              # NEW: ReadTool
│       ├── write.rs             # NEW: WriteTool
│       ├── edit.rs              # NEW: EditTool
│       ├── bash.rs              # NEW: BashTool（含 background 启动）
│       ├── background.rs        # NEW: BackgroundCommand 状态机 + bash_output / kill_background
│       ├── glob.rs              # NEW: GlobTool
│       ├── grep.rs              # NEW: GrepTool
│       ├── ls.rs                # NEW: LsTool
│       ├── ask_user.rs          # NEW: AskUserTool
│       ├── path.rs              # NEW: resolve_tool_path + canoniclize + `..` 处理
│       └── deny_list.rs         # NEW: Bash 黑名单 + host python install 检测
├── providers/
│   ├── mod.rs                   # 改：MessagesRequest 加 tools 字段
│   └── anthropic.rs             # 改：请求 body 加 tools；SSE 解析 tool_use + input_json_delta
└── ipc/
    ├── mod.rs                   # 不变
    ├── commands.rs              # 改：加 approve_tool / answer_ask_user 命令
    └── events.rs                # 改：加 7 个新 payload struct

src-tauri/tests/
├── ipc_payload_contract.rs      # 改：从 5 个测试扩到 13 个
├── loop_tests.rs                # NEW: 3-phase + round 循环
└── tools/
    ├── mod.rs                   # NEW: re-exports
    ├── read.rs                  # NEW
    ├── write.rs                 # NEW
    ├── edit.rs                  # NEW
    ├── bash.rs                  # NEW
    ├── background.rs            # NEW
    ├── glob.rs                  # NEW
    ├── grep.rs                  # NEW
    ├── ls.rs                    # NEW
    ├── ask_user.rs              # NEW
    ├── path.rs                  # NEW
    ├── deny_list.rs             # NEW
    └── registry.rs              # NEW
```

### 前端（7 个新文件 + 5 个改动）

```
src/
├── types/
│   ├── agent.ts                 # 改：加 ToolLoop / Synthesis / Plain
│   ├── message.ts               # 不变
│   ├── tool.ts                  # NEW: ChatToolDefinition / ToolCallRecord / AskUserQuestion / ApprovalRequest
│   └── event.ts                 # NEW: 11 个 event payload 的 TS 类型集中地
├── hooks/
│   └── useAgentEvents.ts        # 改：订阅 11 个事件；保持 camelCase 兼容
├── stores/
│   ├── chatStore.ts             # 改：加 toolRecords slice + addToolRecord / upsertToolRecord action
│   └── agentStore.ts            # 不变
├── components/
│   ├── AgentEventBridge.tsx     # 不变
│   ├── chat/
│   │   ├── ChatView.tsx         # 不变
│   │   ├── ToolCallCard.tsx     # NEW: 折叠展开的工具调用卡片
│   │   ├── ToolResultCard.tsx   # NEW: 工具结果卡片（复用 ToolCallCard 折叠部分）
│   │   ├── AskUserPromptCard.tsx # NEW: 用户提问卡片
│   │   └── StreamingText.tsx    # 改：可选显示 reasoning
│   └── PreviewPane.tsx          # NEW: 右栏实组件（替换占位）
└── App.tsx                      # 改：用 PreviewPane 替换右栏占位
```

### Cargo deps

```toml
# Cargo.toml 新增
tempfile = "3"  # [dev-dependencies]
walkdir = "2"   # ls / glob 用
glob = "1"      # glob_files 用
regex = "1"     # search_files 用
```

## 设计细节（Kivio 借鉴索引）

### A. LoopEnv / RunState 拆分（Kivio `loop_.rs:19-89`）

```rust
pub struct AgentLoop {
    app: Mutex<Option<AppHandle>>,
    state: Mutex<AgentState>,
    history: Mutex<Vec<Message>>,
    pending_approvals: Arc<Mutex<HashMap<ApprovalId, oneshot::Sender<bool>>>>,
    pending_ask_users: Arc<Mutex<HashMap<AskUserId, oneshot::Sender<AskUserResponseResult>>>>,
}

pub(crate) struct LoopEnv<'a> {
    pub(crate) config: &'a AnthropicConfig,
    pub(crate) provider: &'a dyn Provider,
    pub(crate) tools: &'a ToolRegistry,
    pub(crate) host: &'a dyn AgentHost,
    pub(crate) app: Option<&'a AppHandle>,
}

pub(crate) struct RunState {
    pub(crate) history: Vec<Message>,
    pub(crate) assistant_id: String,
    pub(crate) tool_records: Vec<ToolCallRecord>,
    pub(crate) round: u32,
}
```

- `LoopEnv` 不可变引用（启动时构造一次）
- `RunState` 每轮可变
- 状态转移函数纯函数化 `step(env, state) -> Result<StepOutcome, AgentError>`，**易测**

### B. ToolExecutor trait 抽象（Kivio `execute.rs:24-32`）

```rust
pub type ToolExecutorFuture<'a> = Pin<Box<dyn Future<Output = Result<ToolOutput, String>> + Send + 'a>>;

pub trait ToolExecutor: Send + Sync {
    fn call<'a>(
        &'a self,
        ctx: &'a ToolExecutionContext<'a>,
        tool: &'a dyn Tool,
        arguments: serde_json::Value,
    ) -> ToolExecutorFuture<'a>;
}
```

Phase 2 只有一个实现（`NativeExecutor` 直接调 `tool.execute()`），但留接口方便 Phase 4 加 MCP executor。

### C. ProviderToolsUnsupported 兜底（Kivio `loop_.rs:242-247`）

如果 provider 返回 400/拒绝 `tools` 字段，自动 patch system prompt 走 fallback：
```
"tools are not supported by this provider; answer in plain text"
```

实现：`AgentLoop::run_inner` 捕获 Anthropic 400 with `tools` 相关错误，向 `history` 注入一个 user message + 重启一轮（不带 tools）。Phase 2 stub：不实现重试，只 log warn。

### D. 持久化 partial assistant（Kivio `loop_.rs:232-238` / `host.rs:58`）

每个 round 后 `host.persist_partial_assistant(msg_id, tool_records, api_messages)`。Phase 2 实现 `TauriHost::persist_partial_assistant` 走 `Mutex<HashMap<msg_id, PartialDraft>>`（in-memory）。Phase 3 接 SQLite 时换 impl。

### E. ToolCallRecord 完整字段（Kivio `chat/types.rs:203`）

```rust
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub source: String,            // "native"
    pub server_id: Option<String>, // None for native
    pub arguments: String,         // 原始 JSON 字符串（保留格式）
    pub status: ToolCallStatus,    // Pending / Running / Success / Error / Cancelled / Skipped
    pub result_preview: Option<String>, // 前 N 字节
    pub error: Option<String>,
    pub duration_ms: Option<u64>,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub round: u32,
    pub sensitive: bool,
    pub artifacts: Vec<ToolArtifact>, // 文件路径列表（Write / Edit 记录）
    pub structured_content: Option<serde_json::Value>, // AskUser 的答案结构
}
```

### F. Bash 黑名单（Kivio `shell.rs:18-30` 思路，扩大）

```rust
const COMMAND_DENYLIST: &[&str] = &[
    "sudo ", "rm -rf /", "rm -rf /*", ":(){ :|:& };:",
    "mkfs.", "dd if=/dev/zero", "> /dev/sd",
    "shutdown", "reboot", "halt", "poweroff",
    "chmod 777 /",
];

const HOST_PYTHON_PACKAGE_INSTALL_PATTERNS: &[&str] = &[
    "pip install", "pip3 install",
    "python -m pip install", "python3 -m pip install",
    "uv pip install",
];
```

Python install 走 secondary gate（默认拒绝，需要显式 `allow_host_python_package_install: true`）。

### G. Approval 实现（Kivio `execute.rs:200-280` + `host.rs:68`）

```rust
// in execute.rs
if tool.is_sensitive() || is_destructive(tool.name()) {
    record.status = ToolCallStatus::Pending;
    host.emit_tool_record(ctx.run_id, ctx.message_id, &record);

    let approved = host.request_tool_approval(ctx, &record).await;
    if !approved {
        record.status = ToolCallStatus::Cancelled;
        record.error = Some("user denied approval".into());
        host.emit_tool_record(ctx.run_id, ctx.message_id, &record);
        return (record, "user denied approval".into());
    }
}

// in TauriHost impl
async fn request_tool_approval(&self, ctx, record) -> bool {
    let (tx, rx) = oneshot::channel();
    let id = Uuid::new_v4().to_string();
    self.pending_approvals.lock().insert(id.clone(), tx);
    self.emit_approval_request(&id, record);
    rx.await.unwrap_or(false)
}
```

前端：监听 `agent:approval_request`，弹出 `<ApprovalDialog>`，用户点批准/拒绝，调 `approve_tool(approval_id, allow)` 命令回传。

## 影响分析（fix-impact 前置）

| 维度 | 影响 |
|---|---|
| 直接调用方 | `commands::send_message` 不变；新增 `approve_tool` / `answer_ask_user` 走独立 oneshot channel |
| `AgentLoop` 状态机 | Phase 1 的 4 状态 → Phase 2 的 6 状态（加 `ToolLoop / Synthesis / Plain`）；前端同步 |
| 数据结构 | `Message { role, content }` 不变；新增 `ToolCallRecord / AskUserPromptPayload / AskUserResponseResult` 类型 |
| `MessagesRequest` | 加 `tools: Vec<ChatToolDefinition>` 可选字段（Phase 1 调用全兼容——默认空 vec） |
| 协议 | `deepseek-v4-flash` 已声明 `supports_tools=true`（curl 实测）；Anthropic 协议 `tool_use` / `input_json_delta` 已实测 |
| 前端 | `useAgentEvents` 从 4 个事件扩到 11 个；`chatStore` 加 `toolRecords: Map<run_id, ToolCallRecord[]>` slice |
| 错误路径 | tool 越界 → `agent:tool_rejected` + tool_result "permission denied"；LLM 拿到后继续对话 |
| 性能 | tool 执行用 `tokio::task::spawn_blocking` 包同步 IO；并行 round 用 `futures::future::join_all` 上限 8 |

## 测试策略

1. **合约测试**（必跑，CI 阻断）：`ipc_payload_contract.rs` 扩到 **13 个 payload**，全过
2. **Loop 单测**（必跑）：`loop_tests.rs` 模拟 3-phase 转换 + round 循环 + parallel tool（用 mock provider）
3. **工具单测**（必跑）：每个工具独立测 + 路径沙箱 + Bash 黑名单 + registry（合计 ≥ 60 个测试）
4. **Host 单测**（必跑）：`is_generation_active` / `persist_partial_assistant` round-trip
5. **手测 E2E**（你拍板）：5 个 prompt，至少 Read + Bash 黑名单这两个必过；其它能过越多越好
6. **cargo check / tsc --noEmit** 零 error 零 warning

## 完成定义（Definition of Done）

- [ ] 17 个新 Rust 文件落地
- [ ] 7 个新前端文件落地
- [ ] `tests/ipc_payload_contract.rs` 13 个测试全过
- [ ] `loop_tests.rs` + `tests/tools/*.rs` 全部通过
- [ ] `cargo check` + `cargo clippy -- -D warnings` 零警告
- [ ] `npm run lint` + `tsc --noEmit` 零错误（顺手补上 phase 1 没建的 `eslint.config.js`）
- [ ] 手测 5 个 prompt 至少过 4 个
- [ ] Trellis `task.py archive phase2-tool-system` 成功

## 时间估算（参考，非 AC）

- 协议层扩展（MessagesRequest + Anthropic SSE）：半天
- Tool trait + Registry + 10 个工具实现：2 天
- AgentHost + TauriHost + oneshot 桥接：1 天
- AgentLoop 3-phase + rounds + parallel：1 天
- Bash 黑名单 + 路径沙箱 + background：1 天
- Approval 流 + AskUser 流：1 天
- 前端 PreviewPane + 11 个事件订阅 + 卡片组件：1 天
- 测试（合约 + 单测 + 循环测试）：1 天
- 手测 + 修 bug：1 天
- **合计 ≈ 9 个工作日**，按 Trellis 一个大 phase 不拆