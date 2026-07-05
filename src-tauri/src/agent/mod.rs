//! Agent Loop 公共类型：AgentState 枚举 + Message / Conversation 结构 + tools re-exports。

use serde::{Deserialize, Serialize};

/// Agent Loop 状态机当前状态。
///
/// 与前端 `src/types/agent.ts` 的 `AgentState` 严格对齐，
/// 序列化时使用 PascalCase 字符串。
///
/// Phase 2 新增 ToolLoop / Synthesis / Plain 三个变体（借 Kivio `AgentPhase`），
/// 旧的 Idle/Prepare/Stream/Stop 保留向后兼容。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum AgentState {
    Idle,
    Prepare,
    /// 单轮流式输出（无 tool 的对话，Phase 1 的"Stream"语义）
    Stream,
    Stop,
    /// 工具调用循环中：可能多轮，每轮内 ≤ 8 个并行 tool
    ToolLoop,
    /// 工具循环结束后的最终合成（Phase 3 才用到真逻辑，Phase 2 stub）
    Synthesis,
    /// 无 tool 的纯文本对话（Phase 2 stub，等同 Stream 但语义清晰）
    Plain,
}

impl AgentState {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentState::Idle => "Idle",
            AgentState::Prepare => "Prepare",
            AgentState::Stream => "Stream",
            AgentState::Stop => "Stop",
            AgentState::ToolLoop => "ToolLoop",
            AgentState::Synthesis => "Synthesis",
            AgentState::Plain => "Plain",
        }
    }
}

/// 多轮对话中的单条消息（OpenAI chat completions 格式）。
///
/// 三种形态：
/// - 普通文本：`role` + `content: Some("text")`
/// - assistant 工具调用：`role: "assistant"` + `content: None` + `tool_calls: Some([...])`
/// - 工具结果：`role: "tool"` + `content: Some(result)` + `tool_call_id: Some(id)`
///
/// 字段命名说明：
/// - `tool_calls` 和 `tool_call_id` 强制 snake_case（OpenAI API 要求），
///   不跟随 struct 级别的 `rename_all = "camelCase"`。
/// - 前端 IPC payload（Tauri event）用的是 camelCase，但那是另一套类型
///   （见 `ipc/events.rs`），Message 只用于 LLM API 通信。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub role: String,
    /// 文本内容；assistant 调工具时为 None（OpenAI 要求 null）
    #[serde(default)]
    pub content: Option<String>,
    /// OpenAI tool_calls 数组（仅 assistant 调工具时存在）
    /// 注意：字段名强制 snake_case，匹配 OpenAI API 要求
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "tool_calls")]
    pub tool_calls: Option<Vec<OpenAiToolCall>>,
    /// OpenAI tool_call_id（仅 role="tool" 时存在）
    /// 注意：字段名强制 snake_case，匹配 OpenAI API 要求
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "tool_call_id")]
    pub tool_call_id: Option<String>,
}

/// OpenAI chat completions 的 tool_call 对象。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String, // 恒为 "function"
    pub function: OpenAiFunction,
}

/// OpenAI tool_call 内的 function 块。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiFunction {
    pub name: String,
    /// JSON 字符串形式的参数（OpenAI 要求 string，不是 object）
    pub arguments: String,
}

impl Message {
    /// 便捷构造：普通文本消息。
    pub fn text(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// 便捷构造：assistant 工具调用消息。
    pub fn assistant_tool_calls(tool_calls: Vec<OpenAiToolCall>) -> Self {
        Self {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    /// 便捷构造：工具结果消息。
    pub fn tool_result(tool_call_id: &str, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

pub mod host;
pub mod host_impl;
pub mod loop_;
pub mod rounds;
pub mod tools;
pub mod types;

pub use tools::{
    AskUserAnswer, AskUserOption, AskUserPromptPayload, AskUserQuestion, AskUserResponseResult,
    ChatToolDefinition, Tool, ToolCallRecord, ToolCallStatus, ToolContext, ToolError, ToolFuture,
    ToolOutput, ToolRegistry,
};