//! Agent Loop 公共类型：AgentState 枚举 + Message / Conversation 结构。

use serde::{Deserialize, Serialize};

/// Agent Loop 状态机当前状态。
///
/// 与前端 `src/types/agent.ts` 的 `AgentState` 严格对齐，
/// 序列化时使用 PascalCase 字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum AgentState {
    Idle,
    Prepare,
    Stream,
    Stop,
}

impl AgentState {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentState::Idle => "Idle",
            AgentState::Prepare => "Prepare",
            AgentState::Stream => "Stream",
            AgentState::Stop => "Stop",
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

pub mod loop_;