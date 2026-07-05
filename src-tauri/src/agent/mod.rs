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

/// 多轮对话中的单条消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub role: String,
    pub content: String,
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