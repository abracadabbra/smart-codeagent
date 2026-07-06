# Phase 3.2 Session Management

## Goal

为 smart-codeagent 引入多会话（multi-session）管理能力：用户可以创建新会话、在会话之间切换、查看会话列表、自动持久化会话历史，重启应用后能恢复之前的会话。当前是单会话、纯内存、进程退出即丢失的状态。

## User Value

- 用户可以为不同任务建立独立会话（例如"修 bug A" / "加 feature B" / "探索代码 C"），上下文不互相污染
- 重启应用后能恢复历史会话，不会丢失工作进度
- 切换会话时前端 UI 即时反映当前会话的消息和工具记录

## Confirmed Facts（来自代码探查）

### 后端现状

- `AgentLoop.history: Mutex<Vec<Message>>` 是唯一历史源（`src-tauri/src/agent/loop_.rs:34-49`），**进程内单实例**，整个应用共享一份
- `AgentLoop` 通过 `app.manage(Arc::new(AgentLoop::new(...)))` 注册为全局单例（`lib.rs:45-46`）
- **无 `session_id` / `conversation_id` 字段**；多轮对话 = 把 user / assistant tool_calls / tool result / final assistant text 全部 push 进同一个 `history`
- `send_message` 命令签名：`(app, agent, text, assistant_id, run_id, generation)` —— **不接收 history 参数**，前端只传文本 + run_id
- `run_id` 是前端每次发消息时生成的临时标识（`run-${ts}-${rand}`），**不等于 session_id**；一个 session 内会有多个 run_id
- `AgentHost` trait 已预留 `persist_partial_assistant` 方法（`host.rs:52-60`），当前实现只是 emit `EVT_PARTIAL_ASSISTANT` 事件，前端 no-op
- 后端没有任何名为 `Session` / `Conversation` / `ChatSession` / `SessionStore` 的 struct
- Cargo.toml **无 sqlite / rusqlite / sqlx / diesel / sled / rocksdb** 依赖
- 唯一文件持久化是 `settings.json`（仅含 MCP server 配置，冷加载）

### 前端现状

- `chatStore.messages: Message[]` 是扁平数组，**无 sessionId 分组**（`src/stores/chatStore.ts`）
- `Message` 类型（`src/types/message.ts`）字段：`id / role / content / status / createdAt / error` —— **无 sessionId 字段**，不支持 `tool` role
- `App.tsx` 左侧栏是纯占位（`<aside>` 内 "占位 — Phase 3 添加多 Session"）
- 没有 Sidebar / SessionList / SessionSwitcher 等组件
- `useAgentEvents.ts` 订阅 11 个 agent 事件 + 1 个 mcp 事件，消息按 `msgId` 累积到 `chatStore`
- 前端无 IndexedDB / Dexie / localforage 等浏览器端持久化库

### 文档分歧点（待决策）

- **根 PRD §11** 写："会话管理：多会话切换 + **JSONL 持久化** + 历史消息懒加载"
- **`.trellis/spec/backend/database-guidelines.md`** 写："Session persistence (**SQLite via `rusqlite`**) is a Phase 3 deliverable"
- **Kivio 参考实现**（`/Users/shentao/IdeaProjects/codeagent/kivio/src-tauri/src/chat/`）：用文件 + `atomic_write`（3 次重试 + tmp + rename）+ id 前缀校验（`conv_` / `proj_` / `asst_`），看起来是 **JSONL/文件方案**，无 SQLite

### 参考实现（Kivio）

- `chat/types.rs` 已有完整结构可对照：`Conversation`（含 id/title/messages/agent_runtime/created_at/updated_at/pinned 等 20+ 字段）/ `ConversationListItem`（含 preview/message_count 等）/ `ChatMessage`（含 tool_calls/segments/api_messages 等丰富字段）
- `chat/storage.rs`：`atomic_write` + 3 个 id 前缀校验函数
- `chat/commands.rs`：49 个 `#[tauri::command]` 覆盖 conversation/project/assistant/message 全套 CRUD
- 前端 `src/chat/ConversationList.tsx`：会话列表 UI

## Requirements

- TBD（待 brainstorm 收集）

## Acceptance Criteria

- [ ] TBD（待 brainstorm 收集）

## Out of Scope

- TBD（待 brainstorm 收集）

## Decisions

- **D1: 持久化方案 = JSONL 文件**
  - 与根 PRD §11 + Kivio 参考实现对齐
  - 零新依赖（不引入 rusqlite / sqlx）
  - append-only 崩溃恢复友好，天然支持懒加载
  - Phase 3.2 规模（100-1000 会话）足够
  - 反方权衡（未选）：SQLite 查询能力强但 over-engineering；混合方案双写一致性复杂

- **D2: Session 并发模型 = 多 session 并行（照搬 Kivio）**
  - 不照搬 Kivio 的"同会话内多模型一问多答 fan-out"（Phase 3.2 不需要）
  - 但要支持：跨会话多 run 并存（会话 A 在跑时，可切到会话 B 发新消息）
  - 后端：`AppState` 内 `HashMap<conv_id, HashSet<run_id>>` 分桶，跨会话不互斥
  - 前端：`generatingConversationIds: Set<string>` 跟踪多个并行会话
  - 切换 session 时原 run 后台继续跑，不 cancel、不阻止
  - 影响面（待后续 round 细化）：
    - `AgentLoop` 从单例 `history: Mutex<Vec<Message>>` 重构为无状态自由函数 / 或保留单例但按 session_id 路由
    - IPC 命令（send_message / approve_tool / answer_ask_user / cancel_run）全加 `conversation_id` 参数
    - 前端 `chatStore` 从扁平 `messages: Message[]` 改为按 session 分桶
    - 工具审批 modal / ask_user 卡片要按 conversation_id 路由
    - streaming 快照按 conversation_id 保留

- **D3: 文件布局 = 每会话一目录**
  ```
  <app_data_dir>/sessions/
    conv_abc123/
      meta.json          # { id, title, createdAt, updatedAt, pinned }
      messages.jsonl     # 每行一条 ChatMessage，append-only
    conv_def456/
      ...
    index.json           # 可选：所有会话的 meta 摘要，加速列表加载
  ```
  - meta.json 用 `atomic_write`（tmp + rename），改 title/pinned 只重写小文件
  - messages.jsonl 纯 append-only，崩溃恢复最友好
  - 列表加载：扫 `sessions/*/meta.json` 或读 `index.json`
  - 详情加载：读 `sessions/<conv_id>/messages.jsonl`
  - 隔离性好：一个会话文件损坏不影响其他会话

- **D4: Session 数据模型 = 嵌入 ChatMessage**
  ```rust
  pub struct Conversation {
      pub id: String,                // conv_xxx
      pub title: String,             // 首条用户消息前 50 字符，或 "New Session"
      pub created_at: i64,           // unix millis
      pub updated_at: i64,           // 最后一条消息时间
      pub pinned: bool,              // 置顶
  }

  pub struct ChatMessage {
      pub id: String,                // msg_xxx
      pub role: String,              // user / assistant / tool
      pub content: Option<String>,   // 文本内容
      pub tool_calls: Option<Vec<OpenAiToolCall>>,   // LLM 格式（assistant 发起）
      pub tool_call_id: Option<String>,              // tool 角色消息的关联 id
      pub tool_records: Option<Vec<ToolCallRecord>>,  // 完整记录（前端渲染用）
      pub created_at: i64,
  }
  ```
  - 单文件 `messages.jsonl` append-only，每行一条 ChatMessage
  - 重启后：LLM 请求用 `tool_calls`，前端渲染用 `tool_records`
  - 与 Kivio 模式一致（ChatMessage 嵌入 tool_calls/segments 等丰富字段）
  - 信息冗余可接受（tool_calls 是 tool_records 的子集）

- **D5: AgentLoop 重构 = 无状态自由函数（照搬 Kivio）**
  - `AgentLoop` struct 删除，改为 `pub async fn run_agent_loop(config, host, executor) -> Result<AgentRunResult>`
  - `conversation_id / run_id / message_id / history` 通过 `AgentRunConfig` 入参
  - `LoopEnv / RunState` 都是 per-run 局部变量
  - `app / state` 移到 `AppState`（`HashMap<conv_id, AgentState>`）
  - Phase 2 的 `dispatch_round` / `dispatch_single` / `dispatch_mcp` 等纯函数保留，签名从 `&self.history` 改为 `&session.history`
  - 接受更大工作量（8-10 round）换取未来扩展性

## Requirements

### MVP 必选特性（9 项）

1. **创建新会话** —— 点"+ New Session"按钮，生成 `conv_xxx`，空 messages
2. **切换会话** —— 列表点击切换，加载该会话的 messages
3. **删除会话** —— 删除按钮，硬删（含文件）
4. **自动生成标题** —— 首条用户消息前 50 字符作为 title（具体策略待 Q7）
5. **持久化消息历史** —— 每次 user/assistant/tool 消息 append 到 `messages.jsonl`
6. **重启后恢复会话列表** —— 启动时扫 `sessions/*/meta.json` 加载列表
7. **重启后恢复会话消息** —— 切换到某会话时读 `messages.jsonl`
8. **多 session 并行运行** —— 会话 A 在跑时，可切到会话 B 发新消息
9. **会话内发消息触发 agent loop** —— `send_message(conversation_id, text, ...)` 路由到对应 session

### MVP 可选特性（4 项，已选）

- **A. 置顶会话（pin）** —— 列表置顶显示，meta.json 的 pinned 字段
- **B. 重命名会话** —— 双击标题编辑，atomic_write meta.json
- **C. 懒加载消息** —— 切换会话时只加载最近 N 条，滚动到顶再加载更多
- **D. 会话搜索** —— 按标题搜索（不含消息内容搜索）

## Out of Scope

- folder / project_id / set_id / assistant_id 等 Kivio 高级字段（Phase 3.3+）
- 多模型一问多答 fan-out（Phase 3.3+）
- agent skill / plan / todo（Phase 3.3+）
- knowledge_base（Phase 3.3+）
- 全文搜索 / 消息内容搜索（Phase 3.3+）
- 软删除 + 回收站（Phase 3.3+）
- 会话导出 / 导入（Phase 3.3+）

## Decisions（续）

- **D6: MVP 特性清单 = 9 必选 + 4 可选**
  - 必选：创建/切换/删除会话、持久化消息、重启恢复、多 session 并行、会话内发消息
  - 可选：置顶（pin）/ 重命名 / 懒加载消息 / 会话搜索（按标题）

- **D7: 标题生成 = 首条用户消息前 50 字符截取**
  - 零额外 API 调用
  - 超过 50 字符加 `…` 省略号
  - 用户可后续通过"重命名"修改
  - 实现时机：`send_message` 时检查会话是否首条消息，是则更新 title + atomic_write meta.json

- **D8: 工具审批/ask_user 路由 = per-session + badge**
  - 后端：`approvals: Mutex<HashMap<conv_id, HashMap<approval_id, ApprovalRequest>>>`
  - 事件 payload 加 `conversationId` 字段
  - 前端：`agentStore.approvalRequests: Record<conv_id, AgentApprovalRequestPayload>`
  - UI：active session 才显示 modal；非 active session 在 ConversationList 显示红点 badge
  - ask_user 同理：`askUserPrompts: Record<conv_id, AgentAskUserPromptPayload>`

- **D9: SessionStore = 内存缓存 + 写穿磁盘**
  ```rust
  pub struct SessionStore {
      sessions: RwLock<HashMap<conv_id, Conversation>>,  // 内存缓存
      base_dir: PathBuf,
  }
  ```
  - 读：先查缓存，miss 则从磁盘加载入缓存
  - 写消息：append 内存 + append 磁盘 messages.jsonl
  - 写 meta：更新内存 + atomic_write meta.json
  - 同一 session 同一时间只有一个 run（无并发写）
  - 跨 session 多 run 并行（RwLock 允许多读）

## Acceptance Criteria

- [ ] AC1: 启动应用后，左侧栏显示会话列表（从 `sessions/*/meta.json` 加载），无会话时显示"New Session"按钮
- [ ] AC2: 点"+ New Session"创建新会话，生成 `conv_xxx`，空 messages，title="New Session"
- [ ] AC3: 在会话内发首条消息，title 自动更新为消息前 50 字符
- [ ] AC4: 切换会话时加载该会话的 messages（懒加载最近 N 条）
- [ ] AC5: 会话 A 在跑 agent loop 时，可切到会话 B 发新消息，A 后台继续跑
- [ ] AC6: 重启应用后，会话列表恢复，切换会话能加载历史消息
- [ ] AC7: 工具审批 modal 只在 active session 显示；非 active session 的 pending approval 在列表显示 badge
- [ ] AC8: 置顶会话在列表顶部显示；重命名会话双击标题编辑
- [ ] AC9: 按标题搜索会话（不含消息内容）
- [ ] AC10: 删除会话硬删（含文件），需确认
- [ ] AC11: `cargo test` 全过 + `pnpm tsc --noEmit` + `pnpm build` 全绿
- [ ] AC12: IPC 契约测试覆盖所有新增 payload（Conversation / ChatMessage / 事件加 conversationId）

## Open Questions（可在 design.md 定）

- Q10: 懒加载分页参数（每页 N 条，建议 50）—— 在 design.md 定
- Q11: IPC 命令完整清单 —— 在 design.md 定
- Q12: 前端 store 重构 shape —— 在 design.md 定
- Q13: 首次启动行为（无 session 时显示空状态）—— 在 design.md 定
- Q14: conversation_id 生成（后端 create_session 时生成 `conv_<uuid>`）—— 在 design.md 定
