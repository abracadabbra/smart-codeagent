//! TauriHost 实现：用 `AppHandle::emit` + `Mutex<HashMap<id, oneshot::Sender>>`
//! 桥接前端 approval / ask_user 响应。
//!
//! 借 Kivio `commands.rs` 的 `commands::approve_tool` / `answer_ask_user` 模式。
//!
//! Round 3 末了。前端 emit 事件 → 后端 host 收集 oneshot sender →
//! 用户调 command approve_tool / answer_ask_user → 后端把结果通过 sender 传回 host。

use std::collections::HashMap;
use std::sync::Mutex;

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

/// 等待中的 approval 请求（key = approval_id）
pub type ApprovalMap = Mutex<HashMap<String, oneshot::Sender<bool>>>;

/// 等待中的 ask_user 请求（key = ask_user_id）
pub type AskUserMap = Mutex<HashMap<String, oneshot::Sender<AskUserResponseResult>>>;

pub struct TauriHost {
    pub app: AppHandle,
    pub approvals: ApprovalMap,
    pub ask_users: AskUserMap,
    /// 每 run_id + generation 的活跃 generation 计数（0 = 已取消）。
    pub generations: Mutex<HashMap<String, u64>>,
}

impl TauriHost {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            approvals: Mutex::new(HashMap::new()),
            ask_users: Mutex::new(HashMap::new()),
            generations: Mutex::new(HashMap::new()),
        }
    }

    /// Tauri command 调：把 approval_id 对应的 sender 取出，发 bool。
    pub fn resolve_approval(&self, approval_id: &str, allow: bool) -> bool {
        let tx = self.approvals.lock().unwrap().remove(approval_id);
        match tx {
            Some(tx) => tx.send(allow).is_ok(),
            None => false, // 找不到：可能超时 / 重复调用
        }
    }

    /// Tauri command 调：把 ask_user_id 对应的 sender 取出，发答案。
    pub fn resolve_ask_user(&self, ask_user_id: &str, response: AskUserResponseResult) -> bool {
        let tx = self.ask_users.lock().unwrap().remove(ask_user_id);
        match tx {
            Some(tx) => tx.send(response).is_ok(),
            None => false,
        }
    }

    /// 注册新的 generation（命令入口调）。返回 generation 编号。
    pub fn register_generation(&self, run_id: &str, generation: u64) {
        self.generations.lock().unwrap().insert(run_id.to_string(), generation);
    }

    /// 标记 generation 取消（cancel command 调）。
    pub fn cancel_generation(&self, run_id: &str) {
        self.generations.lock().unwrap().remove(run_id);
    }
}

impl AgentHost for TauriHost {
    fn emit_stream_delta(
        &self,
        run_id: &str,
        message_id: &str,
        delta: &str,
        reasoning_delta: Option<&str>,
    ) {
        emit_stream_delta(Some(&self.app), run_id, message_id, delta, reasoning_delta);
    }

    fn emit_stream_done(&self, run_id: &str, message_id: &str, reason: &str) {
        emit_stream_done(Some(&self.app), run_id, message_id, reason);
    }

    fn emit_tool_record(&self, run_id: &str, message_id: &str, record: &ToolCallRecord) {
        emit_tool_record(Some(&self.app), run_id, message_id, record);
    }

    fn request_tool_approval<'a>(
        &'a self,
        ctx: &'a ToolContext,
        record: &'a ToolCallRecord,
    ) -> AgentHostFuture<'a, bool> {
        let approval_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel::<bool>();
        self.approvals.lock().unwrap().insert(approval_id.clone(), tx);

        let payload = AgentApprovalRequestPayload {
            approval_id: approval_id.clone(),
            run_id: ctx.run_id.to_string(),
            msg_id: ctx.message_id.to_string(),
            tool_call_id: ctx.tool_call_id.to_string(),
            tool_name: record.name.clone(),
            arguments: record.arguments.clone(),
            sensitive: record.sensitive,
        };
        if let Err(e) = self.app.emit(EVT_APPROVAL_REQUEST, payload) {
            tracing::warn!("emit approval_request failed: {e}");
            self.approvals.lock().unwrap().remove(&approval_id);
            return Box::pin(async { false });
        }

        Box::pin(async move {
            // 60 秒超时
            match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
                Ok(Ok(allow)) => allow,
                Ok(Err(_)) => false, // sender dropped
                Err(_) => {
                    tracing::warn!("approval {} timed out", approval_id);
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
        let (tx, rx) = oneshot::channel::<AskUserResponseResult>();
        self.ask_users
            .lock()
            .unwrap()
            .insert(ask_user_id.clone(), tx);

        let event_payload = AgentAskUserPromptPayload {
            ask_user_id: ask_user_id.clone(),
            run_id: ctx.run_id.to_string(),
            msg_id: ctx.message_id.to_string(),
            tool_call_id: ctx.tool_call_id.to_string(),
            prompt: payload.clone(),
        };
        if let Err(e) = self.app.emit(EVT_ASK_USER_PROMPT, event_payload) {
            tracing::warn!("emit ask_user_prompt failed: {e}");
            self.ask_users.lock().unwrap().remove(&ask_user_id);
            return Box::pin(async { AskUserResponseResult::default() });
        }

        Box::pin(async move {
            // 5 分钟超时（Kivio 同款）
            match tokio::time::timeout(std::time::Duration::from_secs(300), rx).await {
                Ok(Ok(resp)) => resp,
                _ => AskUserResponseResult {
                    phase: "timeout".into(),
                    answers: Default::default(),
                },
            }
        })
    }

    fn persist_partial_assistant(
        &self,
        run_id: &str,
        message_id: &str,
        records: &[ToolCallRecord],
        api_messages: &[serde_json::Value],
    ) {
        let payload = AgentPartialAssistantPayload {
            run_id: run_id.to_string(),
            msg_id: message_id.to_string(),
            records: records.to_vec(),
            api_messages: api_messages.to_vec(),
        };
        if let Err(e) = self.app.emit(EVT_PARTIAL_ASSISTANT, payload) {
            tracing::debug!("emit partial_assistant no-op or failed: {e}");
        }
    }

    fn is_generation_active(&self, run_id: &str, generation: u64) -> bool {
        self.generations
            .lock()
            .unwrap()
            .get(run_id)
            .copied()
            .map(|g| g == generation)
            .unwrap_or(false)
    }
}

/// 工具被拒后调这个（loops 用来发 `agent:tool_rejected` 事件给前端）。
pub fn emit_tool_rejected(
    app: &AppHandle,
    run_id: &str,
    message_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    reason: &str,
) {
    let payload = AgentToolRejectedPayload {
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
}