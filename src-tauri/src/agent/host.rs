//! AgentHost trait：抽象"loop 怎么跟前端 / 持久化层通信"。
//!
//! 借 Kivio `chat/agent/host.rs:10-99` 的 7-method trait，
//! 砍掉了 compaction_status / ask_user_response（Phase 2 ask_user 在 Round 3 末加）。
//!
//! Phase 2 实现：`TauriHost`（用 `AppHandle::emit` + `Mutex<HashMap<id, oneshot::Sender>>`
//! 桥接用户的 approval / ask_user 响应）。

use std::future::Future;
use std::pin::Pin;

use crate::agent::tools::{
    AskUserPromptPayload, AskUserResponseResult, ToolCallRecord,
};

pub type AgentHostFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Loop 调用 host 的全部接口（7 个 + AskUser stub）。
///
/// 借 Kivio host.rs；ask_user_response 拆为 approval / ask_user 两个方法
/// （语义清晰，front-end UI 不同）。
pub trait AgentHost: Send + Sync {
    /// 流式 token 增量（前端 StreamingText 用）
    fn emit_stream_delta(
        &self,
        run_id: &str,
        message_id: &str,
        delta: &str,
        reasoning_delta: Option<&str>,
    );

    /// 流式结束（前端切 streaming → complete 状态）
    fn emit_stream_done(&self, run_id: &str, message_id: &str, reason: &str);

    /// 工具调用记录（前端 ToolCallCard 渲染）
    fn emit_tool_record(&self, run_id: &str, message_id: &str, record: &ToolCallRecord);

    /// 请求用户批准工具调用。返回 true=批准 / false=拒绝。
    fn request_tool_approval<'a>(
        &'a self,
        ctx: &'a crate::agent::tools::ToolContext,
        record: &'a ToolCallRecord,
    ) -> AgentHostFuture<'a, bool>;

    /// 请求用户回答 ask_user 问题。oneshot channel 等用户响应。
    fn request_ask_user<'a>(
        &'a self,
        ctx: &'a crate::agent::tools::ToolContext,
        payload: &'a AskUserPromptPayload,
    ) -> AgentHostFuture<'a, AskUserResponseResult>;

    /// 持久化部分完成的 assistant 消息（崩溃恢复）。
    /// Phase 2 stub：no-op（in-memory 不需要）。
    fn persist_partial_assistant(
        &self,
        run_id: &str,
        message_id: &str,
        records: &[ToolCallRecord],
        api_messages: &[serde_json::Value],
    );

    /// 检查 generation 是否仍激活（用户是否 cancel 了）。
    fn is_generation_active(&self, run_id: &str, generation: u64) -> bool;
}

/// 编译期防呆：让所有 host 都必须实现 `Send + Sync`。
fn _assert_send_sync<T: Send + Sync>() {}
fn _assert_host_send_sync<H: AgentHost>() {
    _assert_send_sync::<H>();
}