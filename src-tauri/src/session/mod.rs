//! Phase 3.2 会话管理模块。
//!
//! 负责：
//! - 会话元数据 + 消息历史的持久化（JSONL 文件方案，design.md §2）
//! - 会话 CRUD IPC 命令（Round 4）
//! - 内存缓存 + 写穿磁盘（SessionStore，Round 2）
//!
//! 文件布局：
//! ```text
//! <app_data_dir>/sessions/
//!   conv_abc123/
//!     meta.json          # Conversation 元数据（atomic_write）
//!     messages.jsonl     # 每行一条 ChatMessage（append-only）
//!   index.json           # 所有会话的 meta 摘要（加速列表加载）
//! ```
//!
//! 与 Kivio 的差异：Kivio 用单文件 `conversations/{id}.json` 存整个 Conversation，
//! smart-codeagent 拆分为 `meta.json` + `messages.jsonl` 以支持长会话 append O(1)
//! 和崩溃恢复（design.md §2.1）。

pub mod storage;
pub mod types;

pub use types::{ChatMessage, Conversation, ConversationListItem, SessionMessagesPage};
