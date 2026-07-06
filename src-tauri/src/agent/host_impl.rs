//! TauriHost 实现：用 `AppHandle::emit` + `Mutex<HashMap<id, oneshot::Sender>>`
//! 桥接前端 approval / ask_user 响应。
//!
//! 借 Kivio `commands.rs` 的 `commands::approve_tool` / `answer_ask_user` 模式。
//!
//! Phase 3.2 重构：
//! - `approvals` / `ask_users` 保留（key = approval_id / ask_user_id，用于 resolve 查找 sender）
//! - `generations` 删除 → 移交 `AppState`（per-conv `chat_active_generations`）
//! - 新增 `app_state: Arc<AppState>` → per-conv pending 路由 + is_generation_active 查询
//! - 所有 emit payload 加 `conversationId`（events.rs 已改）
//! - `request_tool_approval` / `request_ask_user` 从 `ctx.conversation_id` 取 conv，
//!   注册到 `AppState.pending_approvals` / `pending_ask_users`
//! - `resolve_approval` / `resolve_ask_user` 加 `conversation_id` 参数，同步清 AppState pending
//! - 删除 `register_generation` / `cancel_generation`（runner / cancel_run 命令直接调 AppState）

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

use crate::agent::host::{AgentHost, AgentHostFuture};
use crate::agent::tools::{
    AskUserPromptPayload, AskUserResponseResult, ToolCallRecord, ToolContext,
};
use crate::ipc::events::{
    emit_stream_delta, emit_stream_done, emit_tool_record, AgentAskUserPromptPayload,
    AgentPartialAssistantPayload, AgentToolRejectedPayload, AgentApprovalRequestPayload,
    EVT_APPROVAL_REQUEST, EVT_ASK_USER_PROMPT, EVT_PARTIAL_ASSISTANT, EVT_TOOL_REJECTED,
};
use crate::state::AppState;

/// 等待中的 approval 请求（key = approval_id）
pub type ApprovalMap = Mutex<HashMap<String, oneshot::Sender<bool>>>;

/// 等待中的 ask_user 请求（key = ask_user_id）
pub type AskUserMap = Mutex<HashMap<String, oneshot::Sender<AskUserResponseResult>>>;

pub struct TauriHost {
    pub app: AppHandle,
    pub app_state: Arc<AppState>,
    pub approvals: ApprovalMap,
    pub ask_users: AskUserMap,
}

impl TauriHost {
    pub fn new(app: AppHandle, app_state: Arc<AppState>) -> Self {
        Self {
            app,
            app_state,
            approvals: Mutex::new(HashMap::new()),
            ask_users: Mutex::new(HashMap::new()),
        }
    }

    /// Tauri command 调：把 approval_id 对应的 sender 取出，发 bool。
    /// 同时从 AppState.pending_approvals 移除（per-conv badge 路由用）。
    pub fn resolve_approval(&self, conversation_id: &str, approval_id: &str, allow: bool) -> bool {
        // 先从 AppState pending 集合移除（即使 sender 已超时，也要清 pending 标记）
        self.app_state.take_pending_approval(conversation_id, approval_id);
        let tx = self.approvals.lock().unwrap().remove(approval_id);
        match tx {
            Some(tx) => tx.send(allow).is_ok(),
            None => false, // 找不到：可能超时 / 重复调用
        }
    }

    /// Tauri command 调：把 ask_user_id 对应的 sender 取出，发答案。
    /// 同时从 AppState.pending_ask_users 移除。
    pub fn resolve_ask_user(
        &self,
        conversation_id: &str,
        ask_user_id: &str,
        response: AskUserResponseResult,
    ) -> bool {
        self.app_state.take_pending_ask_user(conversation_id, ask_user_id);
        let tx = self.ask_users.lock().unwrap().remove(ask_user_id);
        match tx {
            Some(tx) => tx.send(response).is_ok(),
            None => false,
        }
    }
}

impl AgentHost for TauriHost {
    fn emit_stream_delta(
        &self,
        conversation_id: &str,
        run_id: &str,
        message_id: &str,
        delta: &str,
        reasoning_delta: Option<&str>,
    ) {
        emit_stream_delta(
            Some(&self.app),
            conversation_id,
            run_id,
            message_id,
            delta,
            reasoning_delta,
        );
    }

    fn emit_stream_done(&self, conversation_id: &str, run_id: &str, message_id: &str, reason: &str) {
        tracing::info!(
            "emit stream_done: conv={}, run_id={}, msg_id={}, reason={}",
            conversation_id, run_id, message_id, reason
        );
        emit_stream_done(Some(&self.app), conversation_id, run_id, message_id, reason);
    }

    fn emit_tool_record(
        &self,
        conversation_id: &str,
        run_id: &str,
        message_id: &str,
        record: &ToolCallRecord,
    ) {
        tracing::debug!(
            "emit tool_record: conv={}, run_id={}, tool={}, status={:?}, id={}",
            conversation_id, run_id, record.name, record.status, record.id
        );
        emit_tool_record(Some(&self.app), conversation_id, run_id, message_id, record);
    }

    fn request_tool_approval<'a>(
        &'a self,
        ctx: &'a ToolContext,
        record: &'a ToolCallRecord,
    ) -> AgentHostFuture<'a, bool> {
        let approval_id = uuid::Uuid::new_v4().to_string();
        let conv_id = ctx.conversation_id.clone();
        let (tx, rx) = oneshot::channel::<bool>();
        self.approvals.lock().unwrap().insert(approval_id.clone(), tx);
        // 注册到 AppState pending_approvals（per-conv badge 路由用）
        self.app_state.insert_pending_approval(&conv_id, &approval_id);

        let payload = AgentApprovalRequestPayload {
            conversation_id: conv_id.clone(),
            approval_id: approval_id.clone(),
            run_id: ctx.run_id.to_string(),
            msg_id: ctx.message_id.to_string(),
            tool_call_id: ctx.tool_call_id.to_string(),
            tool_name: record.name.clone(),
            arguments: record.arguments.clone(),
            sensitive: record.sensitive,
        };
        tracing::info!(
            "emit approval_request: conv={}, approval_id={}, tool={}, tool_call_id={}",
            conv_id, approval_id, record.name, ctx.tool_call_id
        );
        if let Err(e) = self.app.emit(EVT_APPROVAL_REQUEST, payload) {
            tracing::warn!("emit approval_request failed: {e}");
            self.approvals.lock().unwrap().remove(&approval_id);
            self.app_state.take_pending_approval(&conv_id, &approval_id);
            return Box::pin(async { false });
        }

        Box::pin(async move {
            // 60 秒超时
            match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
                Ok(Ok(allow)) => {
                    tracing::info!(
                        "approval {} resolved: allow={}",
                        approval_id, allow
                    );
                    allow
                }
                Ok(Err(_)) => {
                    tracing::warn!(
                        "approval {} sender dropped without response",
                        approval_id
                    );
                    self.app_state.take_pending_approval(&conv_id, &approval_id);
                    false
                }
                Err(_) => {
                    tracing::warn!(
                        "approval {} timed out (60s) — likely modal didn't show or user didn't respond",
                        approval_id
                    );
                    self.app_state.take_pending_approval(&conv_id, &approval_id);
                    false
                }
            }
        })
    }

    fn request_ask_user<'a>(
        &'a self,
        ctx: &'a ToolContext,
        payload: &'a AskUserPromptPayload,
    ) -> AgentHostFuture<'a, AskUserResponseResult> {
        let ask_user_id = uuid::Uuid::new_v4().to_string();
        let conv_id = ctx.conversation_id.clone();
        let (tx, rx) = oneshot::channel::<AskUserResponseResult>();
        self.ask_users
            .lock()
            .unwrap()
            .insert(ask_user_id.clone(), tx);
        // 注册到 AppState pending_ask_users（per-conv badge 路由用）
        self.app_state.insert_pending_ask_user(&conv_id, &ask_user_id);

        let event_payload = AgentAskUserPromptPayload {
            conversation_id: conv_id.clone(),
            ask_user_id: ask_user_id.clone(),
            run_id: ctx.run_id.to_string(),
            msg_id: ctx.message_id.to_string(),
            tool_call_id: ctx.tool_call_id.to_string(),
            prompt: payload.clone(),
        };
        tracing::info!(
            "emit ask_user_prompt: conv={}, ask_user_id={}, tool_call_id={}",
            conv_id, ask_user_id, ctx.tool_call_id
        );
        if let Err(e) = self.app.emit(EVT_ASK_USER_PROMPT, event_payload) {
            tracing::warn!("emit ask_user_prompt failed: {e}");
            self.ask_users.lock().unwrap().remove(&ask_user_id);
            self.app_state.take_pending_ask_user(&conv_id, &ask_user_id);
            return Box::pin(async { AskUserResponseResult::default() });
        }

        Box::pin(async move {
            // 5 分钟超时（Kivio 同款）
            match tokio::time::timeout(std::time::Duration::from_secs(300), rx).await {
                Ok(Ok(resp)) => {
                    tracing::info!(
                        "ask_user {} resolved: phase={}",
                        ask_user_id, resp.phase
                    );
                    resp
                }
                _ => {
                    tracing::warn!("ask_user {} timed out (300s)", ask_user_id);
                    self.app_state.take_pending_ask_user(&conv_id, &ask_user_id);
                    AskUserResponseResult {
                        phase: "timeout".into(),
                        answers: Default::default(),
                    }
                }
            }
        })
    }

    fn persist_partial_assistant(
        &self,
        conversation_id: &str,
        run_id: &str,
        message_id: &str,
        records: &[ToolCallRecord],
        api_messages: &[serde_json::Value],
    ) {
        tracing::debug!(
            "emit partial_assistant: conv={}, run_id={}, records={}, api_msgs={}",
            conversation_id, run_id, records.len(), api_messages.len()
        );
        let payload = AgentPartialAssistantPayload {
            conversation_id: conversation_id.to_string(),
            run_id: run_id.to_string(),
            msg_id: message_id.to_string(),
            records: records.to_vec(),
            api_messages: api_messages.to_vec(),
        };
        if let Err(e) = self.app.emit(EVT_PARTIAL_ASSISTANT, payload) {
            tracing::debug!("emit partial_assistant no-op or failed: {e}");
        }
    }

    fn is_generation_active(&self, conversation_id: &str, generation: u64) -> bool {
        self.app_state.is_generation_active(conversation_id, generation)
    }
}

/// 工具被拒后调这个（loops 用来发 `agent:tool_rejected` 事件给前端）。
///
/// Phase 3.2：加 `conversation_id` 参数。
pub fn emit_tool_rejected(
    app: &AppHandle,
    conversation_id: &str,
    run_id: &str,
    message_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    reason: &str,
) {
    let payload = AgentToolRejectedPayload {
        conversation_id: conversation_id.to_string(),
        run_id: run_id.to_string(),
        msg_id: message_id.to_string(),
        tool_call_id: tool_call_id.to_string(),
        tool_name: tool_name.to_string(),
        reason: reason.to_string(),
    };
    if let Err(e) = app.emit(EVT_TOOL_REJECTED, payload) {
        tracing::warn!("emit tool_rejected failed: {e}");
    }
}

/// 反引用类型，防止 unused warning
#[allow(dead_code)]
fn _unused(_: &AgentPartialAssistantPayload, _: &AgentAskUserPromptPayload) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_map_insert_remove() {
        let map: ApprovalMap = Mutex::new(HashMap::new());
        let (tx, _rx) = oneshot::channel::<bool>();
        map.lock().unwrap().insert("id1".into(), tx);
        assert_eq!(map.lock().unwrap().len(), 1);

        let tx2 = map.lock().unwrap().remove("id1");
        assert!(tx2.is_some());
        assert!(map.lock().unwrap().is_empty());
    }

    #[test]
    fn ask_user_map_insert_remove() {
        let map: AskUserMap = Mutex::new(HashMap::new());
        let (tx, _rx) = oneshot::channel::<AskUserResponseResult>();
        map.lock().unwrap().insert("id2".into(), tx);
        assert_eq!(map.lock().unwrap().len(), 1);
    }

    #[test]
    fn app_state_integration_pending_approval() {
        let state = Arc::new(AppState::new());
        state.insert_pending_approval("conv_a", "ap_1");
        assert!(state.has_pending_approval("conv_a"));

        // 模拟 resolve_approval 的 AppState 部分
        let existed = state.take_pending_approval("conv_a", "ap_1");
        assert!(existed);
        assert!(!state.has_pending_approval("conv_a"));
    }
}
