//! Phase 3.2 会话数据模型。
//!
//! 与 `design.md §3` 对齐。所有 struct 使用 `#[serde(rename_all = "camelCase")]`
//! 匹配前端 TS 类型（`src/types/session.ts`）。
//!
//! - `Conversation`：会话元数据（持久化在 `sessions/<conv_id>/meta.json`）
//! - `ConversationListItem`：列表项（含 preview，用于左侧栏）
//! - `ChatMessage`：单条消息（持久化在 `sessions/<conv_id>/messages.jsonl`，每行一条）
//! - `SessionMessagesPage`：懒加载分页响应

use serde::{Deserialize, Serialize};

use crate::agent::{OpenAiToolCall, ToolCallRecord};

/// 会话元数据。
///
/// 持久化在 `<app_data_dir>/sessions/<conv_id>/meta.json`，
/// 通过 `atomic_write` 原子更新（改 title/pinned 时只重写这个小文件）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    /// 会话 id，格式 `conv_{uuid_v4}`（如 `conv_550e8400-e29b-41d4-a716-446655440000`）
    pub id: String,
    /// 标题。首条用户消息前 50 字符截取；无消息时为 "New Session"
    pub title: String,
    /// 创建时间（unix millis）
    pub created_at: i64,
    /// 最后更新时间（最后一条消息时间）
    pub updated_at: i64,
    /// 是否置顶（列表按 pinned desc, updatedAt desc 排序）
    pub pinned: bool,
    /// 消息总数（冗余字段，避免列表显示时打开 messages.jsonl 统计）
    pub message_count: usize,
}

/// 会话列表项（左侧栏用）。
///
/// 比 `Conversation` 多一个 `preview` 字段（最后一条消息前 100 字符），
/// 用于列表预览。列表加载时从 `index.json` 或 `sessions/*/meta.json` 读取。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationListItem {
    pub id: String,
    pub title: String,
    /// 最后一条消息前 100 字符（无消息时为空字符串）
    pub preview: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub pinned: bool,
    pub message_count: usize,
}

/// 单条聊天消息（持久化在 `messages.jsonl`，每行一条）。
///
/// 三种形态（与 `crate::agent::Message` 对齐）：
/// - 普通文本：`role: "user" | "assistant"` + `content: Some("text")`
/// - assistant 工具调用：`role: "assistant"` + `content: None` + `tool_calls: Some([...])`
/// - 工具结果：`role: "tool"` + `content: Some(result)` + `tool_call_id: Some(id)`
///
/// 比 `crate::agent::Message` 多 `id` / `tool_records` / `created_at` 字段：
/// - `id`：消息 id（`msg_{uuid}`），用于前端 React key + 事件路由
/// - `tool_records`：完整工具记录（含 input/output），前端渲染工具卡片用；
///   `tool_calls` 是 LLM 格式（只有 name+arguments），`tool_records` 是超集
/// - `created_at`：消息时间戳
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    /// 消息 id，格式 `msg_{uuid_v4}`
    pub id: String,
    /// "user" | "assistant" | "tool"
    pub role: String,
    /// 文本内容；assistant 调工具时为 None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// OpenAI 格式 tool_calls（仅 assistant 调工具时存在）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAiToolCall>>,
    /// OpenAI 格式 tool_call_id（仅 role="tool" 时存在）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// 完整工具记录（前端渲染工具卡片用，含 input/output）
    /// 与 `tool_calls` 的关系：tool_records 是超集（含 result_preview/error/duration 等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_records: Option<Vec<ToolCallRecord>>,
    /// 消息时间戳（unix millis）
    pub created_at: i64,
}

impl ChatMessage {
    /// 便捷构造：用户文本消息。
    pub fn user(id: impl Into<String>, content: impl Into<String>, created_at: i64) -> Self {
        Self {
            id: id.into(),
            role: "user".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            tool_records: None,
            created_at,
        }
    }

    /// 便捷构造：assistant 纯文本消息（无工具调用）。
    pub fn assistant_text(
        id: impl Into<String>,
        content: impl Into<String>,
        created_at: i64,
    ) -> Self {
        Self {
            id: id.into(),
            role: "assistant".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            tool_records: None,
            created_at,
        }
    }
}

/// 懒加载分页响应（`get_session_messages` 命令返回值）。
///
/// - `messages`：本页消息（按时间正序，最早 → 最晚）
/// - `total`：会话总消息数
/// - `has_more`：是否还有更早的消息可加载
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessagesPage {
    pub messages: Vec<ChatMessage>,
    pub total: usize,
    pub has_more: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_serializes_camel_case() {
        let conv = Conversation {
            id: "conv_abc".into(),
            title: "test".into(),
            created_at: 1720000000000,
            updated_at: 1720000123000,
            pinned: false,
            message_count: 5,
        };
        let json = serde_json::to_string(&conv).unwrap();
        assert!(json.contains("\"createdAt\""));
        assert!(json.contains("\"updatedAt\""));
        assert!(json.contains("\"messageCount\""));
        assert!(!json.contains("\"created_at\""));
    }

    #[test]
    fn chat_message_skips_none_fields() {
        let msg = ChatMessage::user("msg_1", "hello", 1720000000000);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"content\""));
        assert!(!json.contains("\"toolCalls\""));
        assert!(!json.contains("\"toolCallId\""));
        assert!(!json.contains("\"toolRecords\""));
    }

    #[test]
    fn chat_message_round_trip() {
        let msg = ChatMessage {
            id: "msg_1".into(),
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAiToolCall {
                id: "call_x".into(),
                call_type: "function".into(),
                function: crate::agent::OpenAiFunction {
                    name: "read_file".into(),
                    arguments: "{\"path\":\"a.rs\"}".into(),
                },
            }]),
            tool_call_id: None,
            tool_records: None,
            created_at: 1720000000000,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: ChatMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "msg_1");
        assert_eq!(back.role, "assistant");
        assert!(back.content.is_none());
        assert!(back.tool_calls.is_some());
        let tc = back.tool_calls.unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].function.name, "read_file");
    }

    #[test]
    fn session_messages_page_serializes_camel_case() {
        let page = SessionMessagesPage {
            messages: vec![],
            total: 100,
            has_more: true,
        };
        let json = serde_json::to_string(&page).unwrap();
        assert!(json.contains("\"hasMore\""));
        assert!(!json.contains("\"has_more\""));
    }
}
