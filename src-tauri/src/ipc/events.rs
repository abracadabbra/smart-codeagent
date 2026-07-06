//! Tauri 事件的 payload 类型 + emit helper。
//!
//! Phase 2: 11 个事件（agent:token / status / error / done 是 Phase 1 遗产，
//! 本 phase 沿用 + 替换 agent:done → agent:stream_done）。
//! Phase 2 新增 7 个事件：stream_delta / stream_done / tool_record / approval_request /
//! ask_user_prompt / partial_assistant / tool_rejected。
//! Phase 3.2: 所有 payload 加 `conversationId` 字段（前端按 conv 路由）；
//! 新增 4 个 session 事件（created / updated / deleted / state）。
//!
//! 所有 payload 都 `#[serde(rename_all = "camelCase")]`，由 ipc_payload_contract.rs 测试钉死。

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::agent::tools::{AskUserPromptPayload, ToolCallRecord};
use crate::agent::AgentState;
use crate::session::types::Conversation;

// ============================================================================
// Phase 1 遗产：agent:token / agent:status / agent:error / agent:done
// Phase 3.2: 全部加 conversation_id 字段
// ============================================================================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTokenPayload {
    pub conversation_id: String,
    pub msg_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatusPayload {
    pub conversation_id: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentErrorPayload {
    pub conversation_id: String,
    pub msg_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDonePayload {
    pub conversation_id: String,
    pub msg_id: String,
}

pub const EVT_TOKEN: &str = "agent:token";
pub const EVT_STATUS: &str = "agent:status";
pub const EVT_ERROR: &str = "agent:error";
pub const EVT_DONE: &str = "agent:done";

// ============================================================================
// Phase 2 新增事件：stream_delta / stream_done / tool_record / approval_request /
// ask_user_prompt / partial_assistant / tool_rejected
// Phase 3.2: 全部加 conversation_id 字段
// ============================================================================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStreamDeltaPayload {
    pub conversation_id: String,
    pub run_id: String,
    pub msg_id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_delta: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStreamDonePayload {
    pub conversation_id: String,
    pub run_id: String,
    pub msg_id: String,
    pub reason: String,
    #[serde(default)]
    pub full_text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolRecordPayload {
    pub conversation_id: String,
    pub run_id: String,
    pub msg_id: String,
    pub record: ToolCallRecord,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentApprovalRequestPayload {
    pub conversation_id: String,
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
    pub conversation_id: String,
    pub ask_user_id: String,
    pub run_id: String,
    pub msg_id: String,
    pub tool_call_id: String,
    pub prompt: AskUserPromptPayload,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPartialAssistantPayload {
    pub conversation_id: String,
    pub run_id: String,
    pub msg_id: String,
    pub records: Vec<ToolCallRecord>,
    pub api_messages: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolRejectedPayload {
    pub conversation_id: String,
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
// Phase 3.2 新增：session 事件（created / updated / deleted / state）
// ============================================================================

pub const EVT_SESSION_CREATED: &str = "session:created";
pub const EVT_SESSION_UPDATED: &str = "session:updated";
pub const EVT_SESSION_DELETED: &str = "session:deleted";
pub const EVT_SESSION_STATE: &str = "session:state";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreatedPayload {
    pub conversation: Conversation,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdatedPayload {
    pub conversation: Conversation,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDeletedPayload {
    pub conversation_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatePayload {
    pub conversation_id: String,
    pub state: AgentState,
}

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

// ---- Phase 1 legacy（加 conversation_id 参数） ----

pub fn emit_token(app: Option<&AppHandle>, conversation_id: &str, msg_id: &str, text: &str) {
    do_emit(
        app,
        EVT_TOKEN,
        AgentTokenPayload {
            conversation_id: conversation_id.to_string(),
            msg_id: msg_id.to_string(),
            text: text.to_string(),
        },
    );
}

pub fn emit_status(app: Option<&AppHandle>, conversation_id: &str, state: AgentState) {
    do_emit(
        app,
        EVT_STATUS,
        AgentStatusPayload {
            conversation_id: conversation_id.to_string(),
            state: state.as_str().to_string(),
        },
    );
}

pub fn emit_error(app: Option<&AppHandle>, conversation_id: &str, msg_id: &str, message: &str) {
    do_emit(
        app,
        EVT_ERROR,
        AgentErrorPayload {
            conversation_id: conversation_id.to_string(),
            msg_id: msg_id.to_string(),
            message: message.to_string(),
        },
    );
}

pub fn emit_done(app: Option<&AppHandle>, conversation_id: &str, msg_id: &str) {
    do_emit(
        app,
        EVT_DONE,
        AgentDonePayload {
            conversation_id: conversation_id.to_string(),
            msg_id: msg_id.to_string(),
        },
    );
}

// ---- Phase 2 new（加 conversation_id 参数） ----

pub fn emit_stream_delta(
    app: Option<&AppHandle>,
    conversation_id: &str,
    run_id: &str,
    msg_id: &str,
    text: &str,
    reasoning: Option<&str>,
) {
    do_emit(
        app,
        EVT_STREAM_DELTA,
        AgentStreamDeltaPayload {
            conversation_id: conversation_id.to_string(),
            run_id: run_id.to_string(),
            msg_id: msg_id.to_string(),
            text: text.to_string(),
            reasoning_delta: reasoning.map(|s| s.to_string()),
        },
    );
}

pub fn emit_stream_done(
    app: Option<&AppHandle>,
    conversation_id: &str,
    run_id: &str,
    msg_id: &str,
    reason: &str,
) {
    do_emit(
        app,
        EVT_STREAM_DONE,
        AgentStreamDonePayload {
            conversation_id: conversation_id.to_string(),
            run_id: run_id.to_string(),
            msg_id: msg_id.to_string(),
            reason: reason.to_string(),
            full_text: String::new(),
        },
    );
}

pub fn emit_tool_record(
    app: Option<&AppHandle>,
    conversation_id: &str,
    run_id: &str,
    msg_id: &str,
    record: &ToolCallRecord,
) {
    do_emit(
        app,
        EVT_TOOL_RECORD,
        AgentToolRecordPayload {
            conversation_id: conversation_id.to_string(),
            run_id: run_id.to_string(),
            msg_id: msg_id.to_string(),
            record: record.clone(),
        },
    );
}

// ---- Phase 3.2 session 事件 ----

pub fn emit_session_created(app: Option<&AppHandle>, conversation: &Conversation) {
    do_emit(
        app,
        EVT_SESSION_CREATED,
        SessionCreatedPayload {
            conversation: conversation.clone(),
        },
    );
}

pub fn emit_session_updated(app: Option<&AppHandle>, conversation: &Conversation) {
    do_emit(
        app,
        EVT_SESSION_UPDATED,
        SessionUpdatedPayload {
            conversation: conversation.clone(),
        },
    );
}

pub fn emit_session_deleted(app: Option<&AppHandle>, conversation_id: &str) {
    do_emit(
        app,
        EVT_SESSION_DELETED,
        SessionDeletedPayload {
            conversation_id: conversation_id.to_string(),
        },
    );
}

pub fn emit_session_state(app: Option<&AppHandle>, conversation_id: &str, state: AgentState) {
    do_emit(
        app,
        EVT_SESSION_STATE,
        SessionStatePayload {
            conversation_id: conversation_id.to_string(),
            state,
        },
    );
}
