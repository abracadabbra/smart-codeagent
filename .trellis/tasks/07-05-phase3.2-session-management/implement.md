# Phase 3.2 Session Management — 实施计划

> 配套文档：[prd.md](./prd.md)（需求 + 决策）、[design.md](./design.md)（技术设计）
>
> 共 **9 个 round**，每个 round 独立 commit + 验证。Round 1-5 后端，Round 6-8 前端，Round 9 收尾。

## Round 依赖图

```
R1 (session/types + storage) ──┐
                               ├─→ R4 (agent/runner + session/commands) ──┐
R2 (session/store) ────────────┤                                          ├─→ R5 (重构 loop_ + host + ipc + lib) ──┐
                               │                                          │                                        ├─→ R9 (收尾)
R3 (state.rs) ─────────────────┘                                          │                                        │
                                                                          │   R6 (前端 types + stores) ───────────┤
                                                                          ├─→ R7 (前端 SessionList) ──────────────┤
                                                                          └─→ R8 (前端 hooks + chat 组件) ────────┘
```

- R1/R2/R3 相互独立，但建议按顺序（R2 用 R1 的类型，R3 独立）
- R4 依赖 R1-R3（SessionRunner 用 ChatMessage + SessionStore + AppState）
- R5 依赖 R4（切换到新 run_agent_loop）
- R6/R7/R8 依赖 R5（前端调用新 IPC）
- R6/R7/R8 之间有顺序：types → stores → components → hooks

---

## Round 1: session/types.rs + session/storage.rs

**目标**：建立 session 模块的基础设施 —— 数据类型 + 文件存储原语。

### 改动清单

| 文件 | 类型 | 说明 |
|---|---|---|
| `src-tauri/src/session/mod.rs` | 新建 | 模块入口 + 公共 re-exports |
| `src-tauri/src/session/types.rs` | 新建 | `Conversation` / `ConversationListItem` / `ChatMessage` / `SessionMessagesPage`（含 `#[serde(rename_all = "camelCase")]`） |
| `src-tauri/src/session/storage.rs` | 新建 | `atomic_write`（3 次重试 + tmp + rename，照搬 Kivio）+ `sessions_dir(app)` / `session_dir(app, conv_id)` / `meta_path` / `messages_path` / `index_path` + `validate_conversation_id`（`conv_` 前缀校验） |
| `src-tauri/src/main.rs` 或 `lib.rs` | 微调 | 加 `mod session;`（仅声明，不注册命令） |

### 验证

```bash
cd src-tauri && cargo test session::types session::storage
cargo check
```

### 单元测试（session/storage.rs 内 `#[cfg(test)]`）

- `atomic_write_writes_file` —— 写新文件
- `atomic_write_overwrites_existing` —— 覆盖写
- `atomic_write_creates_parent_dir` —— 父目录不存在时创建
- `validate_conversation_id_accepts_valid` —— `conv_abc123` 通过
- `validate_conversation_id_rejects_invalid` —— `abc` / `proj_xxx` 拒绝

### Commit

```
feat(phase3.2): session types + storage primitives

- session/types.rs: Conversation / ChatMessage / ConversationListItem / SessionMessagesPage
  与 design.md §3 对齐，#[serde(rename_all = "camelCase")]
- session/storage.rs: atomic_write (3 retries + tmp + rename) + 文件路径 helpers
  + validate_conversation_id (conv_ 前缀校验)
- 照搬 Kivio chat/storage.rs 的 atomic_write 实现
```

---

## Round 2: session/store.rs

**目标**：实现 `SessionStore`（内存缓存 + 写穿磁盘）。

### 改动清单

| 文件 | 类型 | 说明 |
|---|---|---|
| `src-tauri/src/session/store.rs` | 新建 | `SessionStore` struct + `ConversationData` + 全部 CRUD 方法（design.md §4.1） |
| `src-tauri/src/session/mod.rs` | 微调 | 加 `pub mod store;` + re-export `SessionStore` |

### SessionStore 方法清单

- `new(base_dir: PathBuf) -> Self`
- `load_index() -> Result<Vec<ConversationListItem>>` —— 启动时扫 index.json 或 sessions/*/meta.json
- `create_session() -> Result<Conversation>` —— 生成 conv_{uuid}，写 meta.json + index.json
- `get_meta(conv_id) -> Result<Conversation>`
- `load_messages(conv_id) -> Result<Vec<ChatMessage>>` —— 首次访问时从 messages.jsonl 读
- `load_messages_paged(conv_id, limit, before) -> Result<SessionMessagesPage>` —— 懒加载分页
- `append_message(conv_id, msg) -> Result<()>` —— append 内存 + append messages.jsonl + 更新 meta
- `update_meta(conv_id, title, pinned) -> Result<Conversation>` —— atomic_write meta.json + 更新 index.json
- `delete_session(conv_id) -> Result<()>` —— 删目录 + 从缓存移除 + 更新 index.json
- `search_sessions(query, limit) -> Result<Vec<ConversationListItem>>` —— 按 title 模糊匹配
- `flush_all() -> Result<()>` —— 写穿模式下通常 no-op

### 验证

```bash
cd src-tauri && cargo test session::store
```

### 单元测试（用 `tempfile::TempDir` 隔离文件系统）

- `create_session_writes_meta_and_index`
- `load_index_from_empty_dir` —— 空目录返回空 vec
- `load_index_from_existing_sessions`
- `append_message_persists_to_jsonl`
- `append_message_updates_meta_count_and_updated_at`
- `load_messages_returns_all_messages`
- `load_messages_paged_returns_latest_n`
- `update_meta_renames_title`
- `update_meta_toggles_pinned`
- `delete_session_removes_directory_and_cache`
- `search_sessions_filters_by_title`
- `load_messages_skips_corrupted_jsonl_line` —— 崩溃恢复（最后一行损坏跳过 + warn）

**注意**：需在 `Cargo.toml` `[dev-dependencies]` 加 `tempfile = "3"`。

### Commit

```
feat(phase3.2): SessionStore — 内存缓存 + 写穿磁盘

- session/store.rs: SessionStore struct + ConversationData
- 内存缓存 RwLock<HashMap<conv_id, ConversationData>>
- 写穿一致性：append_message 先写磁盘再更新内存
- 懒加载：load_messages_paged 支持游标分页（默认 50 条）
- 崩溃恢复：messages.jsonl 逐行解析，损坏行跳过 + warn
- 12 个单元测试覆盖 CRUD + 分页 + 崩溃恢复
```

---

## Round 3: state.rs（AppState 多并行核心）

**目标**：实现 `AppState` + `ChatSendReservation`（照搬 Kivio 的多并行守门机制）。

### 改动清单

| 文件 | 类型 | 说明 |
|---|---|---|
| `src-tauri/src/state.rs` | 新建 | `AppState` struct + `ChatSendReservation` + 全部方法（design.md §5） |
| `src-tauri/src/main.rs` 或 `lib.rs` | 微调 | 加 `mod state;` |

### AppState 字段

- `chat_active_replies: Mutex<HashMap<String, HashSet<String>>>` —— busy 守门
- `chat_stream_generations: Mutex<HashMap<String, u64>>` —— generation 计数
- `chat_active_generations: Mutex<HashMap<String, HashSet<u64>>>` —— cancel 判定
- `session_states: Mutex<HashMap<String, AgentState>>` —— per-session AgentState
- `pending_approvals: Mutex<HashMap<String, HashMap<String, ApprovalRequest>>>` —— per-session 审批
- `pending_ask_users: Mutex<HashMap<String, HashMap<String, AskUserRequest>>>` —— per-session ask_user

### 方法清单

- `try_reserve_chat_send(conv_id, run_id) -> bool` —— busy 守门
- `end_chat_reply(conv_id, run_id)` —— 释放
- `new_run_generation(conv_id) -> u64` —— 新 generation
- `is_generation_active(conv_id, gen) -> bool` —— cancel 检查点
- `cancel_chat_generation(conv_id)` —— 清空该会话所有 generation
- `set_session_state(conv_id, state)` / `get_session_state(conv_id) -> AgentState`
- `insert_pending_approval(conv_id, approval_id, req)` / `take_pending_approval(conv_id, approval_id)`
- `insert_pending_ask_user(conv_id, ask_user_id, req)` / `take_pending_ask_user(conv_id, ask_user_id)`

### 验证

```bash
cd src-tauri && cargo test state
```

### 单元测试

- `try_reserve_chat_send_succeeds_on_idle_session`
- `try_reserve_chat_send_fails_on_busy_session` —— 同会话第二个 reserve 失败
- `try_reserve_chat_send_independent_across_sessions` —— 会话 A busy 不影响 B
- `end_chat_reply_releases_slot`
- `cancel_chat_generation_clears_all_generations_for_session`
- `cancel_chat_generation_is_per_conversation` —— cancel A 不影响 B
- `new_run_generation_increments` —— 单调递增
- `is_generation_active_returns_false_after_cancel`
- `set_get_session_state_round_trip`
- `pending_approval_insert_and_take`

### Commit

```
feat(phase3.2): AppState — 多 session 并行核心

- state.rs: AppState struct，照搬 Kivio 的 chat_active_replies / generations 分桶模式
- ChatSendReservation（Drop guard）—— 原子 busy 检查 + 占槽位，防 TOCTOU
- pending_approvals / pending_ask_users per-session HashMap（D8 路由）
- 10 个单元测试覆盖 reserve / cancel / generation / per-session 隔离
```

---

## Round 4: agent/runner.rs + session/commands.rs

**目标**：实现 `SessionRunner` + `run_agent_loop` 自由函数 + 会话 CRUD IPC 命令。**纯新增，不破坏 Phase 2 的 AgentLoop**。

### 改动清单

| 文件 | 类型 | 说明 |
|---|---|---|
| `src-tauri/src/agent/runner.rs` | 新建 | `SessionRunner` struct + `run_agent_loop` 自由函数（从 Phase 2 的 `AgentLoop::spawn_run` 迁移逻辑） |
| `src-tauri/src/agent/mod.rs` | 微调 | 加 `pub mod runner;` + re-export |
| `src-tauri/src/agent/rounds.rs` | 微调 | `dispatch_round` / `dispatch_single` 签名加 `session: &SessionRunner` 参数（替代 `&self.history`） |
| `src-tauri/src/session/commands.rs` | 新建 | 7 个会话 CRUD 命令（design.md §7.1） |
| `src-tauri/src/session/mod.rs` | 微调 | 加 `pub mod commands;` |

### SessionRunner 字段

```rust
pub struct SessionRunner {
    pub conversation_id: String,
    pub run_id: String,
    pub message_id: String,
    pub history: Vec<ChatMessage>,
    pub generation: u64,
}
```

### run_agent_loop 签名

```rust
pub async fn run_agent_loop(
    config: AgentRunConfig<'_>,
    host: &dyn AgentHost,
    session: &mut SessionRunner,
    session_store: &SessionStore,
    app_state: &AppState,
    mcp_manager: Option<&Arc<McpManager>>,
    settings: &Settings,
) -> Result<AgentRunResult, String>
```

### 会话 CRUD 命令（session/commands.rs）

- `create_session(app) -> Result<Conversation, String>`
- `list_sessions(app) -> Result<Vec<ConversationListItem>, String>`
- `get_session(app, conversation_id) -> Result<Conversation, String>`
- `get_session_messages(app, conversation_id, limit, before) -> Result<SessionMessagesPage, String>`
- `update_session(app, conversation_id, title, pinned) -> Result<Conversation, String>`
- `delete_session(app, conversation_id) -> Result<(), String>`
- `search_sessions(app, query, limit) -> Result<Vec<ConversationListItem>, String>`

**注意**：这些命令**不注册到 invoke_handler**（等 Round 5 一起注册）。

### 验证

```bash
cd src-tauri && cargo test agent::runner session::commands
cargo check  # 应通过（Phase 2 AgentLoop 仍在）
```

### 单元测试

- `SessionRunner::new_initializes_fields`
- `SessionRunner::push_user_appends_to_history`
- `SessionRunner::to_llm_messages_converts_format`
- `run_agent_loop_with_mock_host` —— 用 mock host 跑一个简单 round
- 会话 CRUD 命令的 happy path 测试（用 `tempfile` + mock AppHandle 或直接调 SessionStore）

### Commit

```
feat(phase3.2): SessionRunner + run_agent_loop + session CRUD commands

- agent/runner.rs: SessionRunner (per-run 局部状态) + run_agent_loop 自由函数
  从 Phase 2 AgentLoop::spawn_run 迁移逻辑，改为接收 &mut SessionRunner
- agent/rounds.rs: dispatch_round/dispatch_single 签名加 &SessionRunner
- session/commands.rs: 7 个会话 CRUD 命令（create/list/get/messages/update/delete/search）
- 纯新增，不破坏 Phase 2 AgentLoop（Round 5 切换）
- 命令暂不注册到 invoke_handler（Round 5 一起注册）
```

---

## Round 5: 重构 agent/loop_.rs + host_impl.rs + ipc/commands.rs + events.rs + lib.rs

**目标**：一次性切换到新架构 —— 删除 `AgentLoop`，重构 `TauriHost` 为 per-conv，重构 IPC 命令加 `conversation_id`，所有事件 payload 加 `conversationId`，注册新命令到 `invoke_handler`。

### 改动清单

| 文件 | 类型 | 说明 |
|---|---|---|
| `src-tauri/src/agent/loop_.rs` | 重写 | 删除 `AgentLoop` struct 及所有方法（`new` / `attach_app` / `spawn_run` / `cancel_run` / `state`） |
| `src-tauri/src/agent/host.rs` | 微调 | trait 方法加 `conversation_id: &str` 参数（`emit_stream_delta` / `request_approval` / `request_ask_user` 等） |
| `src-tauri/src/agent/host_impl.rs` | 重构 | `approvals` / `ask_users` / `generations` 改 per-conv HashMap（用 `AppState`）；所有 emit payload 加 `conversationId` |
| `src-tauri/src/ipc/commands.rs` | 重构 | `send_message` 加 `conversation_id` + 改用 `run_agent_loop`；`cancel_run` / `approve_tool` / `answer_ask_user` 加 `conversation_id`；删除 `State<'_, Arc<AgentLoop>>` 参数 |
| `src-tauri/src/ipc/events.rs` | 微调 | 所有 payload struct 加 `conversation_id: String` 字段；新增 `EVT_SESSION_CREATED` / `EVT_SESSION_UPDATED` / `EVT_SESSION_DELETED` / `EVT_SESSION_STATE` + 对应 payload |
| `src-tauri/src/lib.rs` | 微调 | setup 加 `SessionStore::new` + `load_index` + `AppState::new` + `app.manage`；`invoke_handler` 加 7 个会话命令；`ExitRequested` 钩子加 `session_store.flush_all()` |
| `src-tauri/src/agent/mod.rs` | 微调 | 删除 `pub use loop_::AgentLoop;`，加 `pub use runner::{SessionRunner, run_agent_loop};` |

### send_message 新签名

```rust
#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    session_store: State<'_, Arc<SessionStore>>,
    conversation_id: String,
    text: String,
    run_id: String,
) -> Result<SendResult, String>
```

- 前端不再传 `generation`（后端 `new_run_generation` 生成）
- 前端不再传 `assistant_id`（后端生成 `msg_{uuid}`）
- 返回 `SendResult { success: bool, error: Option<String> }` —— busy 时 `success: false`

### 验证

```bash
cd src-tauri && cargo test
cargo check
```

### 验证点

- 所有现有测试通过（contract tests 要更新 payload 加 conversationId）
- `cargo check` 无错误
- 手动跑 `cargo run` 确认应用能启动（setup 不 panic）

### Commit

```
refactor(phase3.2): switch to run_agent_loop + per-session state

- agent/loop_.rs: 删除 AgentLoop struct（spawn_run/cancel_run/state 全删）
- agent/runner.rs: run_agent_loop 成为唯一入口
- agent/host.rs + host_impl.rs: trait 方法加 conversation_id；
  approvals/ask_users/generations 移到 AppState per-conv HashMap
- ipc/commands.rs: send_message 加 conversation_id，改用 run_agent_loop；
  cancel_run/approve_tool/answer_ask_user 加 conversation_id
- ipc/events.rs: 所有 payload 加 conversationId；
  新增 EVT_SESSION_CREATED/UPDATED/DELETED/STATE
- lib.rs: setup 加 SessionStore + AppState；invoke_handler 加 7 个会话命令；
  ExitRequested 钩子加 flush_all
- 更新 contract tests 覆盖新 payload
```

---

## Round 6: 前端 types/session.ts + stores 重构

**目标**：前端类型定义 + store 重构（sessionStore 新建 + chatStore/agentStore 按 conv_id 分桶）。

### 改动清单

| 文件 | 类型 | 说明 |
|---|---|---|
| `src/types/session.ts` | 新建 | `Conversation` / `ConversationListItem` / `ChatMessage` / `SessionMessagesPage` / `OpenAiToolCall` TS 类型（design.md §3.2） |
| `src/types/event.ts` | 微调 | 所有 payload 加 `conversationId: string`；新增 `SessionCreatedPayload` / `SessionUpdatedPayload` / `SessionDeletedPayload` / `SessionStatePayload` + 事件常量 |
| `src/stores/sessionStore.ts` | 新建 | `useSessionStore` + `initSessionStore()`（design.md §9.1） |
| `src/stores/chatStore.ts` | 重构 | `messagesBySession` / `toolRecordsBySession` / `currentAssistantIdBySession` / `hasMoreBySession` / `oldestIndexBySession`（design.md §9.2） |
| `src/stores/agentStore.ts` | 重构 | `statesBySession` / `approvalRequestsBySession` / `askUserPromptsBySession`（design.md §9.3） |

### 验证

```bash
pnpm tsc --noEmit
```

### 注意事项

- chatStore 重构会破坏现有组件（ChatView 等读 `messages`），但本 round **不修组件**（Round 8 修）
- 本 round 验证可能失败（组件报错），只需保证 `tsc` 在 store 文件本身无错误
- 可临时在 ChatView 加 `// @ts-ignore` 或读 `messagesBySession[""] ?? []` 让 tsc 过

### Commit

```
feat(phase3.2): frontend types + stores — sessionStore + chatStore/agentStore per-session

- types/session.ts: Conversation/ChatMessage/ConversationListItem TS 类型
- types/event.ts: 所有 payload 加 conversationId；新增 session 事件 payload
- stores/sessionStore.ts: useSessionStore + initSessionStore()
  sessions/activeSessionId/generatingIds/pendingApprovalIds/searchQuery
- stores/chatStore.ts: 重构为按 conversationId 分桶
  messagesBySession/toolRecordsBySession/currentAssistantIdBySession
  + 懒加载游标 hasMoreBySession/oldestIndexBySession
- stores/agentStore.ts: 重构为 per-session
  statesBySession/approvalRequestsBySession/askUserPromptsBySession
- 组件适配留到 Round 8
```

---

## Round 7: 前端 SessionList + SessionItem 组件

**目标**：实现左侧栏会话列表 UI。

### 改动清单

| 文件 | 类型 | 说明 |
|---|---|---|
| `src/components/SessionList.tsx` | 新建 | 会话列表（design.md §10.1）：New Session 按钮 + 搜索框 + 排序（pinned desc, updatedAt desc）+ 列表渲染 |
| `src/components/SessionItem.tsx` | 新建 | 单条会话：title + spinner（generating）+ 红点 badge（pendingApproval）+ 双击重命名 + 右键菜单（删除/置顶） |
| `src/App.tsx` | 微调 | 渲染 `<SessionList />` 替换占位 `<aside>` |

### SessionItem 交互

- 单击：`selectSession(id)`
- 双击标题：进入编辑模式，回车确认 `renameSession(id, newTitle)`
- 右键菜单 / 三点按钮：删除（需确认）+ 置顶切换
- `generating` 时显示 spinner（`animate-spin`）
- `pendingApproval` 时显示红点 badge
- `active` 时高亮背景

### 验证

```bash
pnpm tsc --noEmit
pnpm build
```

### Commit

```
feat(phase3.2): SessionList + SessionItem 组件

- components/SessionList.tsx: 左侧栏会话列表
  New Session 按钮 + 搜索框 + 排序（pinned desc, updatedAt desc）
- components/SessionItem.tsx: 单条会话
  单击切换 + 双击重命名 + 删除/置顶菜单
  generating 显示 spinner，pendingApproval 显示红点 badge
- App.tsx: 渲染 SessionList 替换占位 aside
```

---

## Round 8: 前端 useAgentEvents 重构 + ChatView/ApprovalDialog/InputBar 微调

**目标**：事件按 `conversationId` 路由 + 适配 chat 组件读 `activeSessionId`。

### 改动清单

| 文件 | 类型 | 说明 |
|---|---|---|
| `src/hooks/useAgentEvents.ts` | 重构 | 所有事件 handler 从 payload 取 `conversationId`，路由到对应 session 的 chatStore；新增 session 事件订阅；启动时调 `initSessionStore()`；`sendMessage` 改签名（加 `conversationId`，去 `assistantId`/`generation`） |
| `src/components/chat/ChatView.tsx` | 微调 | 读 `sessionStore.activeSessionId`；从 `chatStore.messagesBySession[activeId]` 取消息；滚动到顶部触发懒加载；无 active session 时显示空状态 |
| `src/components/chat/InputBar.tsx` | 微调 | `sendMessage` 调用加 `conversationId`；busy 时禁用输入 |
| `src/components/chat/ApprovalDialog.tsx` | 微调 | 读 `sessionStore.activeSessionId`；从 `agentStore.approvalRequestsBySession[activeId]` 取；`approve_tool` 调用加 `conversationId` |
| `src/components/chat/AskUserPromptCard.tsx` | 微调 | 同 ApprovalDialog |
| `src/App.tsx` | 微调 | 启动时 `initSessionStore()` + 自动选中最近会话（design.md §10.4） |

### useAgentEvents 重构要点

```typescript
// 所有事件 handler 加 convId 路由
[
  EVT_STREAM_DELTA,
  (e) => {
    const p = e.payload as AgentStreamDeltaPayload;
    const convId = p.conversationId;
    chatStore.getState().prepareAssistantMessage(convId, p.msgId);
    chatStore.getState().appendStreamDelta(convId, p.msgId, p.text);
  },
],
// ... 所有事件同样模式

// 新增 session 事件订阅
[EVT_SESSION_CREATED, () => sessionStore.getState().loadSessions()],
[EVT_SESSION_UPDATED, () => sessionStore.getState().loadSessions()],
[EVT_SESSION_DELETED, (e) => {
  const p = e.payload as SessionDeletedPayload;
  chatStore.getState().clearSession(p.conversationId);
  sessionStore.getState().loadSessions();
}],
[EVT_SESSION_STATE, (e) => {
  const p = e.payload as SessionStatePayload;
  sessionStore.getState().markGenerating(p.conversationId, p.state === "Running");
  agentStore.getState().setState(p.conversationId, p.state);
}],

// sendMessage 新签名
export interface SendMessageArgs {
  conversationId: string;  // 【新】
  text: string;
  runId: string;
  // 去掉 assistantId 和 generation
}
```

### 验证

```bash
pnpm tsc --noEmit
pnpm build
```

### Commit

```
feat(phase3.2): useAgentEvents 按 conversationId 路由 + chat 组件适配

- hooks/useAgentEvents.ts: 所有事件 handler 按 payload.conversationId 路由
  新增 session 事件订阅（created/updated/deleted/state）
  sendMessage 改签名（加 conversationId，去 assistantId/generation）
- components/chat/ChatView.tsx: 读 activeSessionId + 懒加载 + 空状态
- components/chat/InputBar.tsx: sendMessage 加 conversationId
- components/chat/ApprovalDialog.tsx: 读 activeSessionId 对应的 approvalRequest
- components/chat/AskUserPromptCard.tsx: 同上
- App.tsx: 启动时 initSessionStore + 自动选中最近会话
```

---

## Round 9: 端到端冒烟 + 收尾

**目标**：全量验证 + 手动冒烟测试 + 文档更新。

### 验证清单

```bash
# 后端
cd src-tauri && cargo test
cargo clippy --all-targets -- -D warnings

# 前端
cd .. && pnpm tsc --noEmit
pnpm build

# 端到端冒烟（手动）
pnpm tauri dev
```

### 手动冒烟测试（对照 AC1-AC12）

- [ ] AC1: 启动后左侧栏显示会话列表（首次为空 + New Session 按钮）
- [ ] AC2: 点 New Session 创建会话，title="New Session"
- [ ] AC3: 发首条消息，title 自动更新为消息前 50 字符
- [ ] AC4: 切换会话加载 messages（懒加载最近 50 条）
- [ ] AC5: 会话 A 跑时切到 B 发消息，A 后台继续（B 不阻塞）
- [ ] AC6: 重启应用，列表恢复，切换能加载历史
- [ ] AC7: 会话 A 触发审批，切到 B 时 A 显示 badge，切回 A 显示 modal
- [ ] AC8: 置顶会话顶部显示；双击重命名
- [ ] AC9: 搜索框按标题过滤
- [ ] AC10: 删除会话需确认，硬删文件
- [ ] AC11: cargo test + tsc + build 全绿
- [ ] AC12: contract tests 覆盖新 payload

### 文档更新

- 更新 `PRD.md` §11 Phase 3.2 标记完成
- 更新 `.trellis/spec/backend/database-guidelines.md`（JSONL 方案落地说明）
- 可选：更新 `README.md` 截图

### Commit

```
chore(phase3.2): end-to-end smoke + docs

- 全量 cargo test + pnpm tsc + pnpm build 通过
- 手动冒烟测试覆盖 AC1-AC12
- 更新 PRD.md 标记 Phase 3.2 完成
- 更新 database-guidelines.md 说明 JSONL 落地
```

---

## 风险与缓解

| 风险 | Round | 缓解 |
|---|---|---|
| Round 5 重构影响面大，编译失败 | R5 | 先跑 `cargo check` 定位错误；必要时拆成两个 commit（先 host/events，再 commands/lib） |
| Round 8 前端事件路由遗漏 conversationId | R8 | contract tests 强制所有 payload 含 conversationId |
| JSONL 崩溃恢复未覆盖 | R2 | `load_messages_skips_corrupted_jsonl_line` 测试 |
| 多并行下 run_agent_loop 竞态 | R4-R5 | `ChatSendReservation` 守门 + `is_generation_active` 检查点 |
| 前端 store 重构破坏现有组件 | R6-R8 | R6 只改 store（组件临时 ts-ignore），R8 统一适配 |

## 回滚策略

每个 round 独立 commit，可 `git revert <commit>`：

- R1-R3 失败 → revert，不影响 Phase 2
- R4 失败 → revert，Phase 2 AgentLoop 仍在
- R5 失败 → revert，恢复 Phase 2 AgentLoop（可能需手动解 conflict）
- R6-R8 失败 → revert，后端已就绪，前端回退到 Phase 3.1 状态

## 未决问题（实施时定）

- Q16: mock host 的实现细节（Round 4 测试用）
- Q17: SessionStore 的 LRU 淘汰策略（Phase 3.2 全缓存，不实现 LRU）
- Q18: clippy 是否阻塞 CI（建议是，但 Round 9 再修）
