# Phase 3.2 Session Management — 技术设计

> 配套文档：[prd.md](./prd.md)（需求 + 决策记录）、[implement.md](./implement.md)（待生成，round 分解）

## 1. 架构总览

### 1.1 模块边界

```
src-tauri/src/
├── session/                    # 【新】Phase 3.2 会话管理模块
│   ├── mod.rs                  #   公共类型 + re-exports
│   ├── types.rs                #   Conversation / ChatMessage / ConversationListItem
│   ├── store.rs                #   SessionStore（内存缓存 + 写穿磁盘）
│   ├── storage.rs              #   atomic_write + 文件路径 + id 校验
│   └── commands.rs             #   IPC 命令（create/list/get/delete/update/send_message）
├── agent/
│   ├── loop_.rs                # 【重构】AgentLoop struct 删除 → run_agent_loop 自由函数
│   ├── runner.rs               # 【新】per-run SessionRunner（LoopEnv + RunState 局部）
│   ├── rounds.rs               # 【微调】dispatch_round/dispatch_single 签名加 &SessionRunner
│   ├── host.rs                 # 【微调】AgentHost trait 不变；事件 emit 加 conversationId
│   ├── host_impl.rs            # 【重构】TauriHost approvals/ask_users/generations 改 per-conv HashMap
│   └── ...
├── state.rs                    # 【新】AppState（chat_active_replies / chat_active_generations）
├── ipc/
│   ├── commands.rs             # 【重构】send_message 加 conversation_id；approve_tool/answer_ask_user 路由
│   ├── events.rs               # 【微调】所有事件 payload 加 conversationId 字段
│   └── payload.rs              # 【新】ConversationPayload / ChatMessagePayload 等 IPC DTO
├── settings.rs                 # 不变
├── mcp/                        # 不变
└── lib.rs                      # 【微调】setup 加载 SessionStore + AppState；ExitRequested 钩子加 flush

src/                            # 前端
├── stores/
│   ├── sessionStore.ts         # 【新】会话列表 + active session + 生成中集合
│   ├── chatStore.ts            # 【重构】按 conversationId 分桶
│   └── agentStore.ts           # 【重构】approvalRequests/askUserPrompts 改 Record<conv_id, ...>
├── components/
│   ├── SessionList.tsx         # 【新】左侧栏会话列表
│   ├── SessionItem.tsx         # 【新】单条会话（title + spinner + badge）
│   └── chat/                   # 【微调】ChatView 等读 sessionStore.activeSessionId
├── hooks/
│   └── useAgentEvents.ts       # 【重构】事件按 conversationId 路由到对应 session 的 chatStore
├── types/
│   ├── session.ts              # 【新】Conversation / ChatMessage / ConversationListItem TS 类型
│   └── event.ts                # 【微调】payload 加 conversationId
└── App.tsx                     # 【微调】渲染 SessionList 替换占位
```

### 1.2 与 Phase 2 的兼容性

- **保留**：`dispatch_round` / `dispatch_single` / `dispatch_mcp` / `ToolRegistry` / `ToolContext` / `ToolResultBlock` / `ToolUseBlock` / `AgentRunConfig` / `AgentRunResult` / `RoundResponse` / `AgentHost` trait
- **重构**：`AgentLoop` struct 删除（D5）；`TauriHost` 内部 HashMap 改 per-conv；`send_message` / `approve_tool` / `answer_ask_user` / `cancel_run` 命令签名加 `conversation_id`
- **新增**：`SessionStore` / `AppState` / `SessionRunner` / 会话 CRUD 命令 / 事件 payload 加 `conversationId`
- **废弃**：`AgentLoop::new` / `AgentLoop::attach_app` / `AgentLoop::spawn_run` / `AgentLoop::cancel_run` / `AgentLoop::state` 全删

## 2. 文件布局（D3 细化）

```
<app_data_dir>/sessions/
├── conv_abc123/
│   ├── meta.json               # Conversation 元数据（atomic_write）
│   └── messages.jsonl          # 每行一条 ChatMessage（append-only）
├── conv_def456/
│   ├── meta.json
│   └── messages.jsonl
└── index.json                  # 所有会话的 meta 摘要（加速列表加载）
```

### 2.1 与 Kivio 的分歧说明

| 维度 | Kivio | Phase 3.2 |
|---|---|---|
| 文件格式 | 单文件 `conversations/{id}.json`（整个 Conversation） | 拆分 `meta.json` + `messages.jsonl` |
| 写消息策略 | 重写整个文件（atomic_write） | append 一行到 jsonl |
| 改 meta 策略 | 重写整个文件 | 只重写 `meta.json`（几十字节） |
| 列表加载 | 扫 `conversations/*.json` 读每个文件解析 Conversation | 扫 `sessions/*/meta.json` 或读 `index.json` |
| 长会话性能 | 每次写消息重写整个文件（O(n)） | append 一行（O(1)） |

**坚持 JSONL 的理由**：
1. **长会话性能**：用户与 AI 长对话可能产生数百条消息 + 大量 tool_records，单文件每次写消息重写整个文件会卡顿；JSONL append 是 O(1)
2. **崩溃恢复友好**：append-only 文件，最后一行可能不完整但前面的安全；atomic_write 整个文件在写一半崩溃会损坏整个会话
3. **meta 独立更新便宜**：改 title/pinned 只重写几十字节的 meta.json，不碰消息文件
4. **`index.json` 加速列表**：1000 个会话时，扫 1000 个 meta.json 仍有 IO 开销；index.json 一次读全部摘要

### 2.2 meta.json 格式

```json
{
  "id": "conv_abc123",
  "title": "帮我看一下 auth 模块",
  "createdAt": 1720000000000,
  "updatedAt": 1720000123000,
  "pinned": false,
  "messageCount": 5
}
```

- `messageCount` 冗余字段，避免列表显示时打开 messages.jsonl 统计
- 写消息时同步更新（写穿）

### 2.3 messages.jsonl 格式

每行一条 ChatMessage JSON：

```jsonl
{"id":"msg_001","role":"user","content":"帮我看一下 auth 模块","toolCalls":null,"toolCallId":null,"toolRecords":null,"createdAt":1720000000000}
{"id":"msg_002","role":"assistant","content":null,"toolCalls":[{"id":"call_xxx","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"src/auth.rs\"}"}}],"toolCallId":null,"toolRecords":[{"runId":"run_xxx","recordId":"rec_xxx","toolName":"read_file","input":{"path":"src/auth.rs"},"output":{"content":"..."},"status":"success","startedAt":1720000001000,"completedAt":1720000001500}],"createdAt":1720000001500}
{"id":"msg_003","role":"tool","content":null,"toolCalls":null,"toolCallId":"call_xxx","toolRecords":null,"createdAt":1720000001500}
```

- `tool_calls` 用 OpenAI 格式（`id` / `type` / `function.name` / `function.arguments`）
- `tool_records` 用前端格式（含完整 input/output）
- serde：`#[serde(skip_serializing_if = "Option::is_none")]` 所有 Option 字段，避免 null 冗余

### 2.4 index.json 格式

```json
[
  {"id":"conv_abc123","title":"帮我看一下 auth 模块","createdAt":1720000000000,"updatedAt":1720000123000,"pinned":false,"messageCount":5},
  {"id":"conv_def456","title":"New Session","createdAt":1720000200000,"updatedAt":1720000200000,"pinned":true,"messageCount":0}
]
```

- 数组形式，每个元素是 ConversationMeta 摘要
- 按 `pinned desc, updatedAt desc` 排序
- **写入策略**：每次 meta 变更（创建/删除/重命名/置顶/首条消息）后全量重写 index.json（atomic_write）
- **加载策略**：启动时优先读 index.json；若不存在则扫 `sessions/*/meta.json` 重建

### 2.5 id 生成（Q14）

- `conversation_id`：后端 `create_session` 时生成 `conv_{uuid_v4}`（如 `conv_550e8400-e29b-41d4-a716-446655440000`）
- `message_id`：后端生成 `msg_{uuid_v4}`
- `run_id`：保持前端生成（`run-{ts}-{rand}`），不改

## 3. 数据模型（D4 细化）

### 3.1 Rust 类型

```rust
// src-tauri/src/session/types.rs

use serde::{Deserialize, Serialize};
use crate::agent::types::ToolCallRecord;
use crate::agent::OpenAiToolCall;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub pinned: bool,
    pub message_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationListItem {
    pub id: String,
    pub title: String,
    pub preview: String,         // 最后一条消息前 100 字符
    pub created_at: i64,
    pub updated_at: i64,
    pub pinned: bool,
    pub message_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: String,            // "user" | "assistant" | "tool"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_records: Option<Vec<ToolCallRecord>>,
    pub created_at: i64,
}
```

### 3.2 TypeScript 类型

```typescript
// src/types/session.ts

export interface Conversation {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  pinned: boolean;
  messageCount: number;
}

export interface ConversationListItem {
  id: string;
  title: string;
  preview: string;
  createdAt: number;
  updatedAt: number;
  pinned: boolean;
  messageCount: number;
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "tool";
  content?: string;
  toolCalls?: OpenAiToolCall[];
  toolCallId?: string;
  toolRecords?: ToolCallRecord[];
  createdAt: number;
}

export interface OpenAiToolCall {
  id: string;
  type: "function";
  function: { name: string; arguments: string };
}
```

## 4. SessionStore 设计（D9 细化）

### 4.1 结构

```rust
// src-tauri/src/session/store.rs

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::session::types::{Conversation, ChatMessage};

pub struct SessionStore {
    /// 内存缓存：conv_id → Conversation（含 messages）
    sessions: RwLock<HashMap<String, ConversationData>>,
    /// sessions/ 目录
    base_dir: PathBuf,
}

pub struct ConversationData {
    pub meta: Conversation,
    pub messages: Vec<ChatMessage>,
    /// 是否已从磁盘加载 messages（懒加载）
    pub loaded: bool,
}

impl SessionStore {
    pub fn new(base_dir: PathBuf) -> Self { ... }

    /// 启动时扫 sessions/*/meta.json + index.json 加载列表
    pub async fn load_index(&self) -> Result<Vec<ConversationListItem>, String> { ... }

    /// 创建新会话（写 meta.json + 更新 index.json）
    pub async fn create_session(&self) -> Result<Conversation, String> { ... }

    /// 获取单个会话 meta（从缓存或磁盘）
    pub async fn get_meta(&self, conv_id: &str) -> Result<Conversation, String> { ... }

    /// 加载会话消息（懒加载，首次访问时从 messages.jsonl 读）
    pub async fn load_messages(&self, conv_id: &str) -> Result<Vec<ChatMessage>, String> { ... }

    /// 懒加载分页：返回最近 N 条 + 总数
    pub async fn load_messages_paged(
        &self,
        conv_id: &str,
        limit: usize,
        before: Option<usize>,  // 游标：第 N 条之前
    ) -> Result<(Vec<ChatMessage>, usize), String> { ... }

    /// 追加消息（append 内存 + append messages.jsonl + 更新 meta.updated_at/message_count）
    pub async fn append_message(
        &self,
        conv_id: &str,
        msg: ChatMessage,
    ) -> Result<(), String> { ... }

    /// 更新 meta（title/pinned）—— atomic_write meta.json + 更新 index.json
    pub async fn update_meta(
        &self,
        conv_id: &str,
        title: Option<&str>,
        pinned: Option<bool>,
    ) -> Result<Conversation, String> { ... }

    /// 删除会话（删目录 + 从缓存移除 + 更新 index.json）
    pub async fn delete_session(&self, conv_id: &str) -> Result<(), String> { ... }

    /// 搜索会话（按 title 模糊匹配）
    pub async fn search_sessions(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ConversationListItem>, String> { ... }

    /// 关闭时 flush 所有脏数据（写穿模式下通常无脏数据，但保险）
    pub async fn flush_all(&self) -> Result<(), String> { ... }
}
```

### 4.2 并发模型

- **`RwLock<HashMap>`**：允许多 session 并行读；写时独占
- **同一 session 同一时间只有一个 run**：由 `AppState.try_reserve_chat_send` 守门，保证单 session 内无并发写
- **跨 session 多 run 并行**：各自操作不同 conv_id，`RwLock` 写锁只持有极短时间（append 一条消息）
- **写穿一致性**：`append_message` 先写磁盘成功再更新内存；`update_meta` 先 atomic_write meta.json 再更新内存 + index.json

### 4.3 崩溃恢复

- `messages.jsonl`：append-only，最后一行可能不完整 → 加载时用 `serde_json::from_str` 逐行解析，失败的行跳过 + `tracing::warn!`
- `meta.json`：atomic_write 保证完整（要么旧版要么新版，不会半写）
- `index.json`：同 meta.json，atomic_write 保证完整；若损坏则扫 `sessions/*/meta.json` 重建

## 5. AppState 设计（多并行核心）

### 5.1 结构

```rust
// src-tauri/src/state.rs

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use crate::agent::AgentState;

pub struct AppState {
    /// 每会话活跃 run 集合（busy 守门 + cancel 判定）
    /// conv_id → set of run_id（Phase 3.2 每会话同时只有一个 run_id）
    pub chat_active_replies: Mutex<HashMap<String, HashSet<String>>>,

    /// 每会话的 generation 计数（cancel 用）
    /// conv_id → generation（单调递增，cancel 时清空）
    pub chat_stream_generations: Mutex<HashMap<String, u64>>,

    /// 每会话活跃 generation 集合（cancel 时清空，run 检查点查询）
    pub chat_active_generations: Mutex<HashMap<String, HashSet<u64>>>,

    /// 每会话的 AgentState（Idle/Running/...）
    pub session_states: Mutex<HashMap<String, AgentState>>,

    /// 每会话的 pending approval（per-session + badge 路由，D8）
    /// conv_id → (approval_id → ApprovalRequest)
    pub pending_approvals: Mutex<HashMap<String, HashMap<String, ApprovalRequest>>>,

    /// 每会话的 pending ask_user
    pub pending_ask_users: Mutex<HashMap<String, HashMap<String, AskUserRequest>>>,
}

impl AppState {
    pub fn try_reserve_chat_send(&self, conv_id: &str, run_id: &str) -> bool { ... }
    pub fn end_chat_reply(&self, conv_id: &str, run_id: &str) { ... }
    pub fn new_run_generation(&self, conv_id: &str) -> u64 { ... }
    pub fn is_generation_active(&self, conv_id: &str, gen: u64) -> bool { ... }
    pub fn cancel_chat_generation(&self, conv_id: &str) { ... }
    pub fn set_session_state(&self, conv_id: &str, state: AgentState) { ... }
    pub fn get_session_state(&self, conv_id: &str) -> AgentState { ... }
}
```

### 5.2 ChatSendReservation（照搬 Kivio）

```rust
struct ChatSendReservation<'a> {
    state: &'a AppState,
    conversation_id: String,
    run_id: String,
}

impl<'a> ChatSendReservation<'a> {
    fn try_acquire(state: &'a AppState, conv_id: &str) -> Option<Self> {
        let run_id = format!("reservation-{}", uuid::Uuid::new_v4());
        if !state.try_reserve_chat_send(conv_id, &run_id) {
            return None;
        }
        Some(Self { state, conversation_id: conv_id.to_string(), run_id })
    }
}

impl Drop for ChatSendReservation<'_> {
    fn drop(&mut self) {
        self.state.end_chat_reply(&self.conversation_id, &self.run_id);
    }
}
```

## 6. run_agent_loop 自由函数设计（D5 细化）

### 6.1 签名

```rust
// src-tauri/src/agent/runner.rs

pub async fn run_agent_loop(
    config: AgentRunConfig<'_>,
    host: &dyn AgentHost,
    executor: &dyn ToolExecutor,
    session: &SessionRunner,
) -> Result<AgentRunResult, String> { ... }
```

### 6.2 SessionRunner（per-run 局部状态）

```rust
pub struct SessionRunner {
    pub conversation_id: String,
    pub run_id: String,
    pub message_id: String,        // 主 assistant 消息 id
    pub history: Vec<ChatMessage>, // 从 SessionStore 加载
    pub generation: u64,           // cancel 判定用
}

impl SessionRunner {
    pub fn new(conv_id: &str, run_id: &str, msg_id: &str, history: Vec<ChatMessage>, gen: u64) -> Self { ... }

    /// 追加 user 消息（写 SessionStore + 写内存 history）
    pub async fn push_user(&mut self, store: &SessionStore, text: &str) -> Result<(), String> { ... }

    /// 追加 assistant 消息（含 tool_calls + tool_records）
    pub async fn push_assistant(&mut self, store: &SessionStore, msg: ChatMessage) -> Result<(), String> { ... }

    /// 追加 tool 结果消息
    pub async fn push_tool(&mut self, store: &SessionStore, msg: ChatMessage) -> Result<(), String> { ... }

    /// history 转 LLM 请求格式（Vec<Message>）
    pub fn to_llm_messages(&self) -> Vec<Message> { ... }
}
```

### 6.3 Phase 2 代码迁移

| Phase 2 函数 | Phase 3.2 改动 |
|---|---|
| `AgentLoop::spawn_run` | 删除 → `ipc/commands.rs::send_message` 内 `tokio::spawn(run_agent_loop(...))` |
| `AgentLoop::cancel_run` | 删除 → `ipc/commands.rs::cancel_run` 调 `AppState.cancel_chat_generation(conv_id)` |
| `AgentLoop::attach_app` | 删除 → `host` 直接从 `app.state::<TauriHost>()` 取 |
| `AgentLoop::state` | 删除 → `AppState.get_session_state(conv_id)` |
| `AgentLoop::history` | 删除 → `SessionRunner.history` |
| `dispatch_round(..., &self.history, ...)` | `dispatch_round(..., &session.history, ...)` |
| `dispatch_single(...)` | 不变 |
| `dispatch_mcp(...)` | 不变 |
| `TauriHost::emit_*` | payload 加 `conversationId` |
| `TauriHost::request_approval` | 改 `request_approval(conv_id, ...)`，存入 `pending_approvals[conv_id]` |
| `TauriHost::request_ask_user` | 同上 |

## 7. IPC 命令清单（Q11）

### 7.1 会话管理命令

```rust
// src-tauri/src/session/commands.rs

#[tauri::command]
pub async fn create_session(app: AppHandle) -> Result<Conversation, String>;

#[tauri::command]
pub async fn list_sessions(app: AppHandle) -> Result<Vec<ConversationListItem>, String>;

#[tauri::command]
pub async fn get_session(app: AppHandle, conversation_id: String) -> Result<Conversation, String>;

#[tauri::command]
pub async fn get_session_messages(
    app: AppHandle,
    conversation_id: String,
    limit: Option<usize>,        // 默认 50
    before: Option<usize>,       // 游标，None 表示从最新开始
) -> Result<SessionMessagesPage, String>;

#[tauri::command]
pub async fn update_session(
    app: AppHandle,
    conversation_id: String,
    title: Option<String>,
    pinned: Option<bool>,
) -> Result<Conversation, String>;

#[tauri::command]
pub async fn delete_session(app: AppHandle, conversation_id: String) -> Result<(), String>;

#[tauri::command]
pub async fn search_sessions(
    app: AppHandle,
    query: String,
    limit: Option<usize>,        // 默认 50
) -> Result<Vec<ConversationListItem>, String>;
```

### 7.2 重构的 agent 命令

```rust
// src-tauri/src/ipc/commands.rs

#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: String,     // 【新】
    text: String,
    run_id: String,
    generation: u64,             // 【改】前端不再生成，后端 new_run_generation 返回
) -> Result<SendResult, String>;  // SendResult { success: bool, error: Option<String> }

#[tauri::command]
pub async fn cancel_run(
    state: State<'_, AppState>,
    conversation_id: String,     // 【新】
) -> Result<(), String>;

#[tauri::command]
pub async fn approve_tool(
    state: State<'_, AppState>,
    conversation_id: String,     // 【新】
    approval_id: String,
    allow: bool,
) -> Result<(), String>;

#[tauri::command]
pub async fn answer_ask_user(
    state: State<'_, AppState>,
    conversation_id: String,     // 【新】
    ask_user_id: String,
    response: AskUserResponse,
) -> Result<(), String>;
```

### 7.3 保留的命令（不变）

- `list_mcp_servers` / `list_mcp_server_states` —— 不涉及 session

### 7.4 删除的命令

- 无（所有 Phase 1-3.1 命令保留或加 conversation_id 参数）

### 7.5 懒加载分页（Q10）

```rust
pub struct SessionMessagesPage {
    pub messages: Vec<ChatMessage>,
    pub total: usize,
    pub has_more: bool,          // 是否还有更早的消息
}
```

- 默认每页 50 条
- `before: None` → 返回最新 50 条
- `before: Some(50)` → 返回第 50 条之前的 50 条（即第 0-49 条）
- 前端滚动到顶部时调 `get_session_messages(conv_id, 50, Some(current_oldest_index))`

## 8. 事件 payload 设计（D8 细化）

### 8.1 所有事件加 conversationId

```rust
// src-tauri/src/ipc/events.rs

// Phase 1-2 事件 payload 全加 conversationId 字段
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentStreamDeltaPayload {
    pub conversation_id: String,    // 【新】
    pub run_id: String,
    pub msg_id: String,
    pub text: String,
    pub reasoning_delta: Option<String>,
}

// 同样加 conversationId 的：
// AgentStreamDonePayload / AgentToolRecordPayload / AgentApprovalRequestPayload
// AgentAskUserPromptPayload / AgentPartialAssistantPayload / AgentToolRejectedPayload
// AgentTokenPayload / AgentStatusPayload / AgentErrorPayload / AgentDonePayload
```

### 8.2 新增 session 事件

```rust
pub const EVT_SESSION_CREATED: &str = "session:created";
pub const EVT_SESSION_UPDATED: &str = "session:updated";
pub const EVT_SESSION_DELETED: &str = "session:deleted";
pub const EVT_SESSION_STATE: &str = "session:state";  // AgentState per session

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreatedPayload {
    pub conversation: Conversation,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdatedPayload {
    pub conversation: Conversation,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionDeletedPayload {
    pub conversation_id: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatePayload {
    pub conversation_id: String,
    pub state: AgentState,
}
```

## 9. 前端 store 重构（Q12）

### 9.1 sessionStore（新）

```typescript
// src/stores/sessionStore.ts

interface SessionStoreState {
  sessions: ConversationListItem[];           // 列表
  activeSessionId: string | null;             // 当前会话
  generatingIds: Set<string>;                 // 生成中的会话集合
  pendingApprovalIds: Set<string>;            // 有 pending approval 的会话
  searchQuery: string;                        // 搜索框
  loading: boolean;
  error: string | null;

  loadSessions: () => Promise<void>;
  createSession: () => Promise<Conversation>;
  selectSession: (id: string) => Promise<void>;
  deleteSession: (id: string) => Promise<void>;
  renameSession: (id: string, title: string) => Promise<void>;
  togglePin: (id: string) => Promise<void>;
  searchSessions: (query: string) => Promise<void>;
  setActive: (id: string | null) => void;
  markGenerating: (id: string, generating: boolean) => void;
  markPendingApproval: (id: string, pending: boolean) => void;
}
```

### 9.2 chatStore 重构

```typescript
// src/stores/chatStore.ts

interface ChatStoreState {
  // 按 conversationId 分桶
  messagesBySession: Record<string, Message[]>;
  toolRecordsBySession: Record<string, Record<string, Record<string, ToolCallRecord>>>;
  // messagesBySession[convId][runId][recordId]
  currentAssistantIdBySession: Record<string, string | null>;
  hasMoreBySession: Record<string, boolean>;     // 懒加载游标
  oldestIndexBySession: Record<string, number>;  // 已加载的最早 index

  // actions
  loadMessages: (convId: string, messages: Message[], hasMore: boolean) => void;
  prependMessages: (convId: string, messages: Message[]) => void;  // 懒加载更多
  appendUserMessage: (convId: string, text: string) => string;     // 返回 msgId
  prepareAssistantMessage: (convId: string, msgId: string) => void;
  appendStreamDelta: (convId: string, msgId: string, text: string) => void;
  upsertToolRecord: (convId: string, runId: string, record: ToolCallRecord) => void;
  markComplete: (convId: string, msgId: string) => void;
  markError: (convId: string, msgId: string, error: string) => void;
  clearSession: (convId: string) => void;  // 切换或删除时清理
}
```

### 9.3 agentStore 重构

```typescript
// src/stores/agentStore.ts

interface AgentStoreState {
  // per-session 状态（D8）
  statesBySession: Record<string, AgentState>;
  approvalRequestsBySession: Record<string, AgentApprovalRequestPayload | null>;
  askUserPromptsBySession: Record<string, AgentAskUserPromptPayload | null>;
  lastError: string | null;

  setState: (convId: string, state: AgentState) => void;
  setApprovalRequest: (convId: string, req: AgentApprovalRequestPayload) => void;
  clearApproval: (convId: string) => void;
  setAskUserPrompt: (convId: string, req: AgentAskUserPromptPayload) => void;
  clearAskUser: (convId: string) => void;
  setError: (msg: string | null) => void;
}
```

### 9.4 useAgentEvents 重构

```typescript
// src/hooks/useAgentEvents.ts

// 所有事件 handler 从 payload 取 conversationId，路由到对应 session 的 store
[
  EVT_STREAM_DELTA,
  (e) => {
    const p = e.payload as AgentStreamDeltaPayload;
    const convId = p.conversationId;
    chatStore.getState().prepareAssistantMessage(convId, p.msgId);
    chatStore.getState().appendStreamDelta(convId, p.msgId, p.text);
  },
],
// ... 同样模式

// 新增 session 事件订阅
[
  EVT_SESSION_CREATED,
  (e) => {
    const p = e.payload as SessionCreatedPayload;
    sessionStore.getState().loadSessions();  // 重新拉列表
  },
],
[
  EVT_SESSION_STATE,
  (e) => {
    const p = e.payload as SessionStatePayload;
    sessionStore.getState().markGenerating(p.conversationId, p.state === "Running");
    agentStore.getState().setState(p.conversationId, p.state);
  },
],

// 启动时初始化
void initSessionStore();  // 加载会话列表
void initMcpStore();      // 保留
```

## 10. 前端组件设计

### 10.1 SessionList.tsx

```tsx
function SessionList() {
  const sessions = useSessionStore((s) => s.sessions);
  const activeId = useSessionStore((s) => s.activeSessionId);
  const generatingIds = useSessionStore((s) => s.generatingIds);
  const pendingApprovalIds = useSessionStore((s) => s.pendingApprovalIds);
  const searchQuery = useSessionStore((s) => s.searchQuery);

  const filtered = sessions.filter((s) =>
    s.title.toLowerCase().includes(searchQuery.toLowerCase())
  );
  const sorted = [...filtered].sort((a, b) => {
    if (a.pinned !== b.pinned) return b.pinned ? 1 : -1;
    return b.updatedAt - a.updatedAt;
  });

  return (
    <aside>
      <button onClick={() => createSession()}>+ New Session</button>
      <input value={searchQuery} onChange={(e) => setSearchQuery(e.target.value)} />
      {sorted.map((s) => (
        <SessionItem
          key={s.id}
          session={s}
          active={s.id === activeId}
          generating={generatingIds.has(s.id)}
          pendingApproval={pendingApprovalIds.has(s.id)}
          onSelect={() => selectSession(s.id)}
          onDelete={() => deleteSession(s.id)}
          onRename={(title) => renameSession(s.id, title)}
          onTogglePin={() => togglePin(s.id)}
        />
      ))}
    </aside>
  );
}
```

### 10.2 ApprovalDialog 重构

```tsx
function ApprovalDialog() {
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const approvalRequest = useAgentStore(
    (s) => activeSessionId ? s.approvalRequestsBySession[activeSessionId] : null
  );

  if (!approvalRequest) return null;
  // ... modal 内容不变，但 approve_tool 调用加 conversationId
}
```

### 10.3 ChatView 重构

```tsx
function ChatView() {
  const activeSessionId = useSessionStore((s) => s.activeSessionId);
  const messages = useChatStore((s) =>
    activeSessionId ? s.messagesBySession[activeSessionId] ?? [] : []
  );

  if (!activeSessionId) {
    return <EmptyState onNewSession={() => createSession()} />;
  }

  // ... 渲染 messages
  // 滚动到顶部时触发懒加载
  const onScrollTop = async () => {
    const oldest = useChatStore.getState().oldestIndexBySession[activeSessionId];
    if (useChatStore.getState().hasMoreBySession[activeSessionId]) {
      const page = await invoke("get_session_messages", {
        conversationId: activeSessionId,
        limit: 50,
        before: oldest,
      });
      useChatStore.getState().prependMessages(activeSessionId, page.messages);
    }
  };
}
```

### 10.4 首次启动行为（Q13）

```tsx
// App.tsx
useEffect(() => {
  void (async () => {
    await initSessionStore();  // 加载会话列表
    const sessions = useSessionStore.getState().sessions;
    if (sessions.length === 0) {
      // 无会话 → 显示空状态（不自动创建，等用户点 New Session）
      useSessionStore.getState().setActive(null);
    } else {
      // 自动选中最近的会话
      const latest = sessions.reduce((a, b) => a.updatedAt > b.updatedAt ? a : b);
      await useSessionStore.getState().selectSession(latest.id);
    }
  })();
}, []);
```

## 11. 数据流：关键场景

### 11.1 创建新会话

```
[用户点 New Session]
  → sessionStore.createSession()
  → invoke("create_session")
  → 后端 SessionStore.create_session()
    → 生成 conv_{uuid}
    → 写 sessions/conv_{uuid}/meta.json（atomic_write）
    → 更新 index.json（atomic_write）
    → 加入内存缓存
    → emit EVT_SESSION_CREATED
  → 前端 sessionStore.setActive(conv_id)
  → ChatView 显示空状态
```

### 11.2 发送首条消息

```
[用户输入 "帮我看一下 auth 模块" + 回车]
  → 前端生成 run_id = run-{ts}-{rand}
  → chatStore.appendUserMessage(conv_id, text) → 返回 user_msg_id
  → chatStore.prepareAssistantMessage(conv_id, assistant_msg_id)
  → invoke("send_message", { conversation_id, text, run_id })
  → 后端:
    → ChatSendReservation::try_acquire(state, conv_id)
      → 失败（busy）→ 返回 { success: false, error: "busy" } → 前端提示
      → 成功 → 继续
    → generation = state.new_run_generation(conv_id)
    → history = SessionStore.load_messages(conv_id)
    → 如果是首条消息 → SessionStore.update_meta(conv_id, title=text[..50])
    → SessionStore.append_message(conv_id, user_msg)
    → session = SessionRunner::new(conv_id, run_id, assistant_msg_id, history, generation)
    → tokio::spawn(run_agent_loop(config, host, executor, &session))
  → 前端不等 spawn 返回，立即返回成功
  → run_agent_loop 内部:
    → 每个 round append assistant + tool messages
    → emit EVT_STREAM_DELTA / EVT_TOOL_RECORD / EVT_SESSION_STATE
    → 完成后 emit EVT_STREAM_DONE + EVT_SESSION_STATE(Idle)
```

### 11.3 切换会话

```
[用户点击会话 B]
  → sessionStore.selectSession(B)
  → invoke("get_session_messages", { conversation_id: B, limit: 50, before: null })
  → 后端 SessionStore.load_messages_paged(B, 50, None)
    → 返回最新 50 条 + total + has_more
  → chatStore.loadMessages(B, messages, has_more)
  → sessionStore.setActive(B)
  → ChatView 渲染 messages（会话 A 的 run 后台继续跑，事件按 conv_id 路由）
```

### 11.4 工具审批（per-session + badge）

```
[会话 A 的 run_agent_loop 触发 destructive tool]
  → host.request_approval(conv_id=A, approval_id, tool_name, args)
    → state.pending_approvals[A].insert(approval_id, req)
    → emit EVT_APPROVAL_REQUEST { conversationId: A, ... }
  → 前端 useAgentEvents 收到:
    → agentStore.setApprovalRequest(A, req)
    → sessionStore.markPendingApproval(A, true)
  → 如果 activeSessionId == A:
    → ApprovalDialog 显示 modal
  → 如果 activeSessionId != A:
    → 不显示 modal
    → SessionItem(A) 显示红点 badge
  [用户切到会话 A]
    → sessionStore.selectSession(A)
    → ApprovalDialog 检测 approvalRequestsBySession[A] → 显示 modal
  [用户点批准]
    → invoke("approve_tool", { conversation_id: A, approval_id, allow: true })
    → 后端 state.pending_approvals[A].remove(approval_id)
    → 唤醒等待的 oneshot
    → run_agent_loop 继续
```

### 11.5 重启恢复

```
[应用启动]
  → lib.rs setup:
    → SessionStore::new(app_data_dir/sessions)
    → SessionStore.load_index() → 扫 index.json 或 sessions/*/meta.json
    → AppState::new()
    → app.manage(Arc::new(SessionStore))
    → app.manage(Arc::new(AppState))
  → 前端 initSessionStore():
    → invoke("list_sessions") → 返回 Vec<ConversationListItem>
    → sessionStore.loadSessions()
    → 如果有会话 → selectSession(最新的)
    → 如果无会话 → setActive(null) + 显示空状态
```

### 11.6 应用退出

```
[ExitRequested]
  → tauri::async_runtime::block_on(async {
      → mcp_manager.disconnect_all()  // Phase 3.1 已有
      → session_store.flush_all()     // Phase 3.2 新增（写穿模式下通常 no-op）
    })
```

## 12. 兼容性与迁移

### 12.1 Phase 2 代码迁移清单

| 文件 | 改动类型 | 说明 |
|---|---|---|
| `src-tauri/src/agent/loop_.rs` | 重写 | `AgentLoop` struct 删除 → `run_agent_loop` 自由函数 |
| `src-tauri/src/agent/rounds.rs` | 微调 | `dispatch_round` 签名加 `&SessionRunner` |
| `src-tauri/src/agent/host.rs` | 微调 | trait 方法加 `conversation_id` 参数 |
| `src-tauri/src/agent/host_impl.rs` | 重构 | HashMap 改 per-conv |
| `src-tauri/src/ipc/commands.rs` | 重构 | 4 个命令加 `conversation_id` |
| `src-tauri/src/ipc/events.rs` | 微调 | 所有 payload 加 `conversationId` |
| `src-tauri/src/lib.rs` | 微调 | setup 加 SessionStore + AppState |
| `src/hooks/useAgentEvents.ts` | 重构 | 事件按 conv_id 路由 |
| `src/stores/chatStore.ts` | 重构 | 按 conv_id 分桶 |
| `src/stores/agentStore.ts` | 重构 | per-session 状态 |
| `src/components/chat/ApprovalDialog.tsx` | 微调 | 读 activeSessionId |
| `src/components/chat/ChatView.tsx` | 微调 | 读 activeSessionId + 懒加载 |
| `src/components/chat/InputBar.tsx` | 微调 | send_message 加 conversation_id |

### 12.2 数据迁移

- Phase 3.2 前：无 sessions/ 目录 → 首次启动创建空目录
- Phase 3.2 前：无持久化 history → 旧内存 history 丢失（可接受，Phase 2 是开发版）
- **不提供自动迁移**：Phase 2 是开发版，无生产数据需保护

### 12.3 测试策略

- **后端 unit tests**：
  - `SessionStore` CRUD + 并发 + 崩溃恢复
  - `AppState` try_reserve / cancel / generation
  - `run_agent_loop` 用 mock host 跑完整流程
- **IPC contract tests**：所有新 payload 序列化 + camelCase
- **前端**：`pnpm tsc --noEmit` + `pnpm build`
- **不写 e2e**：Phase 3.2 规模不值得

## 13. 重要 Trade-offs

### 13.1 JSONL vs 单文件 JSON（已决策 D3）
- 选 JSONL：长会话 append O(1)，崩溃恢复友好
- 代价：meta 和 messages 分文件，需 index.json 加速列表

### 13.2 无状态自由函数 vs AgentLoop 单例（已决策 D5）
- 选自由函数：未来扩展性好，与 Kivio 对齐
- 代价：Phase 2 代码重写量大，8-10 round

### 13.3 内存缓存 vs 纯磁盘（已决策 D9）
- 选内存缓存：agent loop 性能好
- 代价：内存占用（可控，活跃 session < 10），需写穿一致性

### 13.4 per-session 审批 vs 全局（已决策 D8）
- 选 per-session：不打断多并行工作流
- 代价：用户可能忽略 badge，会话 A 的 run 卡住

## 14. 操作与 Rollback

### 14.1 风险点

1. **`AgentLoop` 重构影响面大** —— Phase 2 的 `spawn_run` / `cancel_run` / `state` 全删，调用点全改
   - 缓解：implement.md 分多个 round，每 round 跑 `cargo test`
2. **前端 store 重构影响所有组件** —— chatStore / agentStore shape 变
   - 缓解：先做后端 + IPC，前端单独 round
3. **JSONL 崩溃恢复** —— 最后一行可能损坏
   - 缓解：加载时跳过损坏行 + warn
4. **多并行下事件路由错误** —— 事件 payload 漏 conversationId
   - 缓解：contract tests 强制所有 payload 含 conversationId

### 14.2 Rollback 策略

- 每个 round 独立 commit，可 `git revert` 单个 round
- 关键 rollback 点：
  - Round 1-2（session 模块）失败 → revert，不影响 Phase 2
  - Round 3-4（agent 重构）失败 → revert，但需手动恢复 Phase 2 的 AgentLoop
  - Round 5-6（IPC + 事件）失败 → revert，前端不受影响
  - Round 7-8（前端）失败 → revert，后端已就绪

## 15. 未决问题（留到 implement.md）

- Q15: 每个 round 的具体改动文件清单 + 验证命令
- Q16: mock host 的实现细节（用于 run_agent_loop 测试）
- Q17: SessionStore 的 LRU 淘汰策略（Phase 3.2 可先不实现，全缓存）
