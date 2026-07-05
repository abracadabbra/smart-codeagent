//! Agent Loop 主循环：3-phase（ToolLoop / Synthesis / Plain）+ 多轮 round。
//!
//! Phase 2 借 Kivio `loop_.rs:135` 的 `run_agent_loop` 形态 + `AgentPhase` 三段式：
//!
//! ```text
//! Idle → Prepare → ToolLoop (round 1: plan → execute tool_use → loop)
//!               └→ Synthesis (tool 循环结束 → 最终合成)
//!               └→ Plain (无 tool，纯文本)
//!       → Stop → Idle
//! ```
//!
//! Phase 2 砍掉了 Kivio 的 Kivio-only 字段（assistant snapshot / skills / plan mode），
//! 保留核心：host trait 抽象、round 循环、tool execution、approval gate、partial persist。

use crate::agent::host::AgentHost;
use crate::agent::tools::{
    ChatToolDefinition, ToolCallRecord, ToolCallStatus, ToolContext, ToolRegistry,
};
use crate::agent::types::{AgentRunConfig, AgentRunResult, RoundResponse, ToolUseBlock};
use crate::agent::{AgentState, Message};
use crate::config::AnthropicConfig;
use crate::ipc::events::{emit_status, emit_stream_done, emit_tool_record};
use crate::mcp::McpManager;
use crate::providers::anthropic::AnthropicClient;
use crate::providers::{MessagesRequest, Provider, StreamChunk};
use crate::settings::Settings;
use futures::StreamExt;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// 全局共享的 Agent Loop handle。
pub struct AgentLoop {
    app: Mutex<Option<AppHandle>>,
    state: Mutex<AgentState>,
    history: Mutex<Vec<Message>>,
    config: AgentRunConfig,
}

impl AgentLoop {
    pub fn new(config: AgentRunConfig) -> Self {
        Self {
            app: Mutex::new(None),
            state: Mutex::new(AgentState::Idle),
            history: Mutex::new(Vec::new()),
            config,
        }
    }

    pub async fn attach_app(self: &Arc<Self>, handle: AppHandle) {
        let mut slot = self.app.lock().await;
        *slot = Some(handle);
    }

    pub async fn app_handle(&self) -> Option<AppHandle> {
        self.app.lock().await.clone()
    }

    pub async fn current_state(&self) -> AgentState {
        *self.state.lock().await
    }

    pub async fn config(&self) -> &AgentRunConfig {
        &self.config
    }

    /// 收到 send_message command 时调用：跑完一轮完整 loop。
    /// 不阻塞 command 调用 — 用 tokio::spawn 在后台执行。
    pub fn spawn_run(
        self: Arc<Self>,
        text: String,
        assistant_id: String,
        run_id: String,
        generation: u64,
    ) {
        tokio::spawn(async move {
            if let Err(e) = self.run_inner(text, assistant_id, run_id, generation).await {
                tracing::error!("agent loop failed: {e:?}");
            }
        });
    }

    async fn run_inner(
        self: Arc<Self>,
        user_text: String,
        assistant_id: String,
        run_id: String,
        generation: u64,
    ) -> anyhow::Result<()> {
        let app = self.app_handle().await;

        // 1. Idle → Prepare
        self.transition(AgentState::Prepare).await;
        emit_status(app.as_ref(), AgentState::Prepare);

        // 2. 构造消息历史（追加用户消息）
        {
            let mut history = self.history.lock().await;
            history.push(Message::text("user", user_text));
        }

        // 3. 构造 provider + tool registry
        let anthropic_cfg = AnthropicConfig::from_env();
        let provider = AnthropicClient::new(anthropic_cfg);
        let tools = self.build_tool_registry();
        let mut tool_defs = tools.definitions();

        // 4. host：复用 lib.rs 在 setup 时 manage 的单例 TauriHost。
        //    不能每次新建——否则 approve_tool / answer_ask_user 命令通过 try_state
        //    拿到的是另一个实例，oneshot sender 永远等不到 resolve。
        let host: Arc<dyn AgentHost> = match app.as_ref() {
            Some(handle) => {
                let h: Arc<crate::agent::host_impl::TauriHost> = handle
                    .try_state::<Arc<crate::agent::host_impl::TauriHost>>()
                    .map(|s| s.inner().clone())
                    .ok_or_else(|| anyhow::anyhow!("TauriHost not managed"))?;
                h.register_generation(&run_id, generation);
                h as Arc<dyn AgentHost>
            }
            None => {
                warn!("no AppHandle attached; loop cannot emit events");
                return Ok(());
            }
        };

        // 4.5 Phase 3.1: 合并 MCP server 暴露的工具到 tool_defs
        let mcp_manager: Option<Arc<McpManager>> = app.as_ref().and_then(|handle| {
            handle
                .try_state::<Arc<McpManager>>()
                .map(|s| s.inner().clone())
        });
        let settings_state: Option<Arc<Mutex<Settings>>> = app.as_ref().and_then(|handle| {
            handle
                .try_state::<Arc<Mutex<Settings>>>()
                .map(|s| s.inner().clone())
        });
        if let (Some(mcp_mgr), Some(settings_state)) =
            (mcp_manager.as_ref(), settings_state.as_ref())
        {
            let settings = settings_state.lock().await;
            let mcp_defs = mcp_mgr.list_all_tools(&settings).await;
            info!(
                "merged {} MCP tool(s) into tool_defs (total={})",
                mcp_defs.len(),
                tool_defs.len() + mcp_defs.len()
            );
            tool_defs.extend(mcp_defs);
        }

        // 5. Phase 2 状态机：先把 tool 循环跑完，再决定是否需要 synthesis
        self.transition(AgentState::ToolLoop).await;
        emit_status(app.as_ref(), AgentState::ToolLoop);

        let mut all_tool_records: Vec<ToolCallRecord> = Vec::new();
        let mut final_text = String::new();
        let mut round: u32 = 0;

        loop {
            if round >= self.config.max_tool_rounds {
                warn!("hit max_tool_rounds={}", self.config.max_tool_rounds);
                break;
            }
            round += 1;

            // 检查 generation 是否被取消
            if !host.is_generation_active(&run_id, generation) {
                debug!("generation cancelled at round {round}");
                break;
            }

            // 跑一轮：调 LLM + 处理 tool_use
            let snapshot = {
                let history = self.history.lock().await;
                history.clone()
            };

            info!(
                "round {round} start: requesting LLM (history len={})",
                snapshot.len()
            );

            let req = MessagesRequest {
                model: self.config.model.clone(),
                max_tokens: self.config.max_tokens,
                messages: snapshot,
                system: Some(self.config.system_prompt.clone()),
                stream: true,
                tools: tool_defs.clone(),
            };

            let stream_result = provider.stream_chat(req).await;
            let mut stream = match stream_result {
                Ok(s) => s,
                Err(e) => {
                    self.emit_error_and_stop(&app, &assistant_id, &format!("request failed: {e}"))
                        .await;
                    return Err(anyhow::anyhow!(e));
                }
            };

            // 累积本轮响应：text + tool_use
            let round_resp = Self::consume_stream(
                &mut stream,
                &app,
                &run_id,
                &assistant_id,
                &host,
                round,
                &mut all_tool_records,
            )
            .await;

            info!(
                "round {round} done: text_len={}, tool_uses={}, stop_reason={:?}",
                round_resp.text.len(),
                round_resp.tool_uses.len(),
                round_resp.stop_reason
            );

            // 如果有 tool_use 派发它们
            if !round_resp.tool_uses.is_empty() {
                let ctx = ToolContext {
                    run_id: run_id.clone(),
                    message_id: assistant_id.clone(),
                    tool_call_id: String::new(), // single-tool 时由 execute 填
                    round,
                    generation,
                };

                info!(
                    "round {round}: dispatching {} tool(s): {:?}",
                    round_resp.tool_uses.len(),
                    round_resp.tool_uses.iter().map(|t| t.name.as_str()).collect::<Vec<_>>()
                );

                let tool_results = crate::agent::rounds::dispatch_round(
                    &tools,
                    mcp_manager.as_ref(),
                    &tool_defs,
                    &host,
                    &ctx,
                    &round_resp.tool_uses,
                )
                .await;

                info!(
                    "round {round}: dispatch done, results={}",
                    tool_results.len()
                );

                // 把 assistant tool_use + user tool_result 推回 history
                Self::append_tool_round_to_history(
                    &self.history,
                    &round_resp.tool_uses,
                    &tool_results,
                )
                .await;

                // 持久化 partial
                let api_msgs = build_tool_round_api_messages(
                    &round_resp.text,
                    &round_resp.tool_uses,
                    &tool_results,
                );
                host.persist_partial_assistant(
                    &run_id,
                    &assistant_id,
                    &all_tool_records,
                    &api_msgs,
                );

                // OpenAI stop_reason="tool_calls" 时继续下一轮；
                // stop_reason="stop" 且无 tool_use → 上面 else 分支 break。
                // 这里只在 LLM 既没输出文本又没调工具时退出（异常情况）。
                if round_resp.text.is_empty()
                    && round_resp.tool_uses.is_empty()
                {
                    warn!("round {round}: empty response (no text, no tool_use), breaking");
                    break;
                }
                continue; // 进入下一轮
            } else {
                // 没有 tool_use：纯文本回复 → 本轮就是 final
                final_text = round_resp.text;
                break;
            }
        }

        // 6. Stop → Idle
        emit_stream_done(app.as_ref(), &run_id, &assistant_id, "end_turn");
        self.transition(AgentState::Stop).await;
        emit_status(app.as_ref(), AgentState::Stop);

        // 7. 把 final assistant 文本推回 history
        if !final_text.is_empty() {
            let mut history = self.history.lock().await;
            history.push(Message::text("assistant", &final_text));
        }

        let result = AgentRunResult {
            final_text,
            tool_records: all_tool_records,
            ask_user_response: None,
            rounds: round,
        };
        info!(
            "agent loop completed: rounds={}, tool_calls={}",
            result.rounds,
            result.tool_records.len()
        );

        self.transition(AgentState::Idle).await;
        emit_status(app.as_ref(), AgentState::Idle);
        Ok(())
    }

    /// 消费单个 stream，累积 text + tool_use。
    async fn consume_stream(
        stream: &mut crate::providers::TokenStream,
        app: &Option<AppHandle>,
        run_id: &str,
        message_id: &str,
        host: &Arc<dyn AgentHost>,
        round: u32,
        all_records: &mut Vec<ToolCallRecord>,
    ) -> RoundResponse {
        let mut text = String::new();
        let mut tool_uses: Vec<ToolUseBlock> = Vec::new();
        let mut stop_reason: Option<String> = None;
        let mut current_tool: Option<usize> = None; // index in tool_uses
        let mut started_at = chrono::Utc::now().timestamp();

        while let Some(chunk_res) = stream.next().await {
            match chunk_res {
                Ok(chunk) => match chunk {
                    StreamChunk::Text(delta) => {
                        debug!(
                            "SSE text delta: len={}, content={:?}",
                            delta.len(),
                            if delta.len() > 80 { &delta[..80] } else { &delta }
                        );
                        text.push_str(&delta);
                        host.emit_stream_delta(run_id, message_id, &delta, None);
                    }
                    StreamChunk::ToolUseStart { id, name } => {
                        info!("SSE tool_use start: id={}, name={}", id, name);
                        tool_uses.push(ToolUseBlock {
                            id: id.clone(),
                            name: name.clone(),
                            input: serde_json::Value::Null,
                            input_raw: String::new(),
                        });
                        current_tool = Some(tool_uses.len() - 1);
                        started_at = chrono::Utc::now().timestamp();

                        // emit ToolCallRecord (Pending)
                        let record = ToolCallRecord {
                            id: tool_uses.last().unwrap().id.clone(),
                            name: tool_uses.last().unwrap().name.clone(),
                            source: "native".into(),
                            server_id: None,
                            arguments: String::new(),
                            status: ToolCallStatus::Pending,
                            result_preview: None,
                            error: None,
                            duration_ms: None,
                            started_at: Some(started_at),
                            completed_at: None,
                            round,
                            sensitive: false, // execute 后会刷新
                            artifacts: vec![],
                            structured_content: None,
                        };
                        host.emit_tool_record(run_id, message_id, &record);
                        all_records.push(record);
                    }
                    StreamChunk::ToolUseInputDelta(delta) => {
                        // 只在 debug 级别打，避免参数太大刷屏
                        if let Some(idx) = current_tool {
                            debug!(
                                "SSE tool_use input delta: tool_idx={}, delta_len={}, raw_so_far={}",
                                idx,
                                delta.len(),
                                tool_uses[idx].input_raw.len() + delta.len()
                            );
                            tool_uses[idx].input_raw.push_str(&delta);
                        } else {
                            warn!("SSE tool_use input delta but no current_tool (delta_len={})", delta.len());
                        }
                    }
                    StreamChunk::ToolUseEnd => {
                        info!("SSE tool_use end: parsing {} accumulated tool_use(s)", tool_uses.len());
                        // OpenAI 对所有 tool_calls 只 emit 一个 ToolUseEnd。
                        // 解析所有已累积 input_raw 但还没 parse 的 tool_use。
                        current_tool = None;
                        for (i, tu) in tool_uses.iter_mut().enumerate() {
                            if tu.input.is_null() && !tu.input_raw.is_empty() {
                                match serde_json::from_str(&tu.input_raw) {
                                    Ok(v) => {
                                        info!(
                                            "tool_use[{}] parsed: id={}, name={}, input={}",
                                            i, tu.id, tu.name, v
                                        );
                                        tu.input = v;
                                    }
                                    Err(e) => {
                                        warn!(
                                            "tool_use[{}] input parse failed: id={}, name={}, raw={:?}, err={}",
                                            i, tu.id, tu.name, tu.input_raw, e
                                        );
                                    }
                                }
                            }
                        }
                    }
                    StreamChunk::Done { stop_reason: sr } => {
                        info!("SSE done: stop_reason={:?}", sr);
                        stop_reason = sr;
                        break;
                    }
                },
                Err(e) => {
                    warn!("stream error: {e}");
                    break;
                }
            }
        }

        RoundResponse {
            text,
            tool_uses,
            stop_reason,
        }
    }

    /// 把 tool_use + tool_result 追加到 history（OpenAI chat completions 格式）。
    ///
    /// OpenAI 格式：
    /// - assistant message: `{"role":"assistant","content":null,"tool_calls":[...]}`
    /// - 每个 tool_result 是独立的 tool message:
    ///   `{"role":"tool","tool_call_id":"...","content":"..."}`
    async fn append_tool_round_to_history(
        history: &Mutex<Vec<Message>>,
        tool_uses: &[ToolUseBlock],
        tool_results: &[crate::agent::rounds::ToolResultBlock],
    ) {
        use crate::agent::{OpenAiFunction, OpenAiToolCall};

        let mut history = history.lock().await;

        // 1. assistant message with tool_calls
        let tool_calls: Vec<OpenAiToolCall> = tool_uses
            .iter()
            .map(|tu| OpenAiToolCall {
                id: tu.id.clone(),
                call_type: "function".into(),
                function: OpenAiFunction {
                    name: tu.name.clone(),
                    // OpenAI 要求 arguments 是 JSON 字符串
                    arguments: serde_json::to_string(&tu.input).unwrap_or_else(|_| "{}".into()),
                },
            })
            .collect();
        history.push(Message::assistant_tool_calls(tool_calls));

        // 2. 每个 tool_result 是独立的 tool message
        for tr in tool_results {
            let body = match &tr.kind {
                crate::agent::rounds::ToolResultKind::Success { content } => content.clone(),
                crate::agent::rounds::ToolResultKind::Error { message } => message.clone(),
                crate::agent::rounds::ToolResultKind::Denied { reason } => {
                    format!("permission denied: {reason}")
                }
            };
            history.push(Message::tool_result(&tr.tool_use_id, body));
        }
    }

    async fn emit_error_and_stop(
        &self,
        app: &Option<AppHandle>,
        assistant_id: &str,
        message: &str,
    ) {
        use crate::ipc::events::emit_error;
        emit_error(app.as_ref(), assistant_id, message);
        self.transition(AgentState::Stop).await;
        emit_status(app.as_ref(), AgentState::Stop);
        self.transition(AgentState::Idle).await;
        emit_status(app.as_ref(), AgentState::Idle);
    }

    /// 构造工具注册表。Phase 2 内置 10 个工具。
    pub fn build_tool_registry(&self) -> ToolRegistry {
        use crate::agent::tools::{
            ask_user::AskUserTool, background::{BashOutputTool, KillBackgroundTool},
            bash::BashTool, edit::EditTool, glob::GlobTool, grep::GrepTool, ls::LsTool,
            read::ReadTool, write::WriteTool,
        };

        ToolRegistry::new()
            .register(ReadTool)
            .register(WriteTool)
            .register(EditTool)
            .register(BashTool)
            .register(BashOutputTool)
            .register(KillBackgroundTool)
            .register(GlobTool)
            .register(GrepTool)
            .register(LsTool)
            .register(AskUserTool)
    }

    async fn transition(&self, new_state: AgentState) {
        let mut s = self.state.lock().await;
        *s = new_state;
    }
}

/// 构造 tool round 的 API 消息（OpenAI 格式，部分持久化用，
/// host 拿到后 emit `agent:partial_assistant`）。
fn build_tool_round_api_messages(
    text: &str,
    tool_uses: &[ToolUseBlock],
    tool_results: &[crate::agent::rounds::ToolResultBlock],
) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    if !text.is_empty() {
        out.push(serde_json::json!({
            "role": "assistant",
            "content": text,
        }));
    }
    if !tool_uses.is_empty() {
        // assistant message with tool_calls (OpenAI format)
        out.push(serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": tool_uses.iter().map(|tu| serde_json::json!({
                "id": tu.id,
                "type": "function",
                "function": {
                    "name": tu.name,
                    "arguments": serde_json::to_string(&tu.input).unwrap_or_else(|_| "{}".into()),
                },
            })).collect::<Vec<_>>(),
        }));
        // each tool_result as a separate tool message (OpenAI format)
        for tr in tool_results {
            let body = match &tr.kind {
                crate::agent::rounds::ToolResultKind::Success { content } => content.clone(),
                crate::agent::rounds::ToolResultKind::Error { message } => message.clone(),
                crate::agent::rounds::ToolResultKind::Denied { reason } => {
                    format!("permission denied: {reason}")
                }
            };
            out.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": tr.tool_use_id,
                "content": body,
            }));
        }
    }
    out
}

/// 反引用类型防止 unused warning
#[allow(dead_code)]
fn _ensure_types_used() {}