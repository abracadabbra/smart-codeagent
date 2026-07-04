//! Tauri 事件的 payload 类型 + emit helper。

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::agent::AgentState;

#[derive(Debug, Clone, Serialize)]
pub struct AgentTokenPayload {
    pub msg_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStatusPayload {
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentErrorPayload {
    pub msg_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentDonePayload {
    pub msg_id: String,
}

/// 事件名常量。前端 `AgentEventBridge` 监听同一组。
pub const EVT_TOKEN: &str = "agent:token";
pub const EVT_STATUS: &str = "agent:status";
pub const EVT_ERROR: &str = "agent:error";
pub const EVT_DONE: &str = "agent:done";

// ---------- emit helpers ----------

/// Phase 1：所有事件先尝试用 AppHandle 发送，没有（极早期边界）就 warn 并吞掉。
fn do_emit<E: Serialize + Clone>(app: Option<&AppHandle>, name: &str, payload: E) {
    let Some(handle) = app else {
        tracing::warn!("emit {name} skipped: AppHandle not attached yet");
        return;
    };
    if let Err(e) = handle.emit(name, payload) {
        tracing::warn!("emit {name} failed: {e}");
    }
}

pub fn emit_token(app: Option<&AppHandle>, msg_id: &str, text: &str) {
    do_emit(
        app,
        EVT_TOKEN,
        AgentTokenPayload {
            msg_id: msg_id.to_string(),
            text: text.to_string(),
        },
    );
}

pub fn emit_status(app: Option<&AppHandle>, state: AgentState) {
    do_emit(
        app,
        EVT_STATUS,
        AgentStatusPayload {
            state: state.as_str().to_string(),
        },
    );
}

pub fn emit_error(app: Option<&AppHandle>, msg_id: &str, message: &str) {
    do_emit(
        app,
        EVT_ERROR,
        AgentErrorPayload {
            msg_id: msg_id.to_string(),
            message: message.to_string(),
        },
    );
}

pub fn emit_done(app: Option<&AppHandle>, msg_id: &str) {
    do_emit(
        app,
        EVT_DONE,
        AgentDonePayload {
            msg_id: msg_id.to_string(),
        },
    );
}