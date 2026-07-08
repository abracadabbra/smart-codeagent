//! IPC 公共类型 + 事件推送 helper。

pub mod commands;
pub mod events;

pub use events::{AgentDonePayload, AgentErrorPayload, AgentStatusPayload, AgentTokenPayload};
