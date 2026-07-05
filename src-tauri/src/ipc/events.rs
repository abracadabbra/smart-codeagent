//! Tauri 事件的 payload 类型 + emit helper。
//!
//! Phase 2: 11 个事件（agent:token / status / error / done 是 Phase 1 遗产，
//! 本 phase 沿用 + 替换 agent:done → agent:stream_done）。
//! Phase 2 新增 7 个事件：stream_delta / stream_done / tool_record / approval_request /
//! ask_user_prompt / partial_assistant / tool_rejected。
//!
//! 所有 payload 都 `#[serde(rename_all = "camelCase")]`，由 ipc_payload_contract.rs 测试钉死。

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::agent::tools::{AskUserPromptPayload, ToolCallRecord};
use crate::agent::AgentState;

// ============================================================================
// Phase 1 遗产：agent:token / agent:status / agent:error
// ============================================================================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTokenPayload {
    pub msg_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatusPayload {
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentErrorPayload {
    pub msg_id: String,
    pub message: String,
}

// Phase 1 的 agent:done 改名 agent:stream_done；保留向后兼容 stub
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDonePayload {
    pub msg_id: String,
}

pub const EVT_TOKEN: &str = "agent:token";
pub const EVT_STATUS: &str = "agent:status";
pub const EVT_ERROR: &str = "agent:error";
pub const EVT_DONE: &str = "agent:done";

// ============================================================================
// Phase 2 新增事件：stream_delta / stream_done / tool_record / approval_request /
// ask_user_prompt / partial_assistant / tool_rejected
// ============================================================================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStreamDeltaPayload {
    pub run_id: String,
    pub msg_id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_delta: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStreamDonePayload {
    pub run_id: String,
    pub msg_id: String,
    pub reason: String,
    #[serde(default)]
    pub full_text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolRecordPayload {
    pub run_id: String,
    pub msg_id: String,
    pub record: ToolCallRecord,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentApprovalRequestPayload {
    pub approval_id: String,
    pub run_id: String,
    pub msg_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: String,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAskUserPromptPayload {
    pub ask_user_id: String,
    pub run_id: String,
    pub msg_id: String,
    pub tool_call_id: String,
    pub prompt: AskUserPromptPayload,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPartialAssistantPayload {
    pub run_id: String,
    pub msg_id: String,
    pub records: Vec<ToolCallRecord>,
    pub api_messages: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolRejectedPayload {
    pub run_id: String,
    pub msg_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub reason: String,
}

pub const EVT_STREAM_DELTA: &str = "agent:stream_delta";
pub const EVT_STREAM_DONE: &str = "agent:stream_done";
pub const EVT_TOOL_RECORD: &str = "agent:tool_record";
pub const EVT_APPROVAL_REQUEST: &str = "agent:approval_request";
pub const EVT_ASK_USER_PROMPT: &str = "agent:ask_user_prompt";
pub const EVT_PARTIAL_ASSISTANT: &str = "agent:partial_assistant";
pub const EVT_TOOL_REJECTED: &str = "agent:tool_rejected";

// ============================================================================
// emit helpers
// ============================================================================

fn do_emit<E: Serialize + Clone>(app: Option<&AppHandle>, name: &str, payload: E) {
    let Some(handle) = app else {
        tracing::warn!("emit {name} skipped: AppHandle not attached yet");
        return;
    };
    if let Err(e) = handle.emit(name, payload) {
        tracing::warn!("emit {name} failed: {e}");
    }
}

// ---- Phase 1 legacy ----

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

// ---- Phase 2 new ----

pub fn emit_stream_delta(
    app: Option<&AppHandle>,
    run_id: &str,
    msg_id: &str,
    text: &str,
    reasoning: Option<&str>,
) {
    do_emit(
        app,
        EVT_STREAM_DELTA,
        AgentStreamDeltaPayload {
            run_id: run_id.to_string(),
            msg_id: msg_id.to_string(),
            text: text.to_string(),
            reasoning_delta: reasoning.map(|s| s.to_string()),
        },
    );
}

pub fn emit_stream_done(app: Option<&AppHandle>, run_id: &str, msg_id: &str, reason: &str) {
    do_emit(
        app,
        EVT_STREAM_DONE,
        AgentStreamDonePayload {
            run_id: run_id.to_string(),
            msg_id: msg_id.to_string(),
            reason: reason.to_string(),
            full_text: String::new(),
        },
    );
}

pub fn emit_tool_record(app: Option<&AppHandle>, run_id: &str, msg_id: &str, record: &ToolCallRecord) {
    do_emit(
        app,
        EVT_TOOL_RECORD,
        AgentToolRecordPayload {
            run_id: run_id.to_string(),
            msg_id: msg_id.to_string(),
            record: record.clone(),
        },
    );
}