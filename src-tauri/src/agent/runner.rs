//! `SessionRunner` + `run_agent_loop` —— Phase 3.2 无状态自由函数入口。
//!
//! design.md §6 的实现。与 Phase 2 的 `AgentLoop` 并存（Round 4 纯新增，
//! Round 5 切换后删除 AgentLoop）。
//!
//! 关键差异（vs Phase 2 `AgentLoop::run_inner`）：
//! - `&self.history: Mutex<Vec<Message>>` → `session.history: Vec<ChatMessage>`（无锁，per-run owned）
//! - `self.transition(state)` → `app_state.set_session_state(conv_id, state)`
//! - `host.is_generation_active(run_id, gen)` → `app_state.is_generation_active(conv_id, gen)`（per-conv）
//! - 每条消息 push 时同时持久化到 SessionStore（写穿）
//! - LLM 请求用 `session.to_llm_messages()` 转换 history → `Vec<Message>`

use std::sync::Arc;

use crate::agent::host::AgentHost;
use crate::agent::tools::{ToolCallRecord, ToolCallStatus, ToolContext, ToolRegistry};
use crate::agent::types::{AgentRunConfig, AgentRunResult, RoundResponse, ToolUseBlock};
use crate::agent::{AgentState, Message, OpenAiFunction, OpenAiToolCall};
use crate::config::AnthropicConfig;
use crate::ipc::events::{emit_error, emit_status, emit_stream_done};
use crate::mcp::McpManager;
use crate::providers::anthropic::AnthropicClient;
use crate::providers::{MessagesRequest, Provider, StreamChunk};
use crate::session::store::SessionStore;
use crate::session::types::ChatMessage;
use crate::settings::Settings;
use crate::state::AppState;
use futures::StreamExt;
use tauri::AppHandle;
use tracing::{debug, info, warn};

/// per-run 局部状态。
///
/// design.md §6.2。每次 `send_message` 命令创建一个，传给 `run_agent_loop`。
/// run 结束后 drop（不跨 run 复用）。
pub struct SessionRunner {
    pub conversation_id: String,
    pub run_id: String,
    pub message_id: String,
    pub history: Vec<ChatMessage>,
    pub generation: u64,
}

impl SessionRunner {
    pub fn new(
        conversation_id: String,
        run_id: String,
        message_id: String,
        history: Vec<ChatMessage>,
        generation: u64,
    ) -> Self {
        Self {
            conversation_id,
            run_id,
            message_id,
            history,
            generation,
        }
    }

    /// 追加 user 消息（写 SessionStore + 写内存 history）。
    pub async fn push_user(
        &mut self,
        store: &SessionStore,
        text: &str,
    ) -> Result<(), String> {
        let msg = ChatMessage::user(
            format!("msg_{}", uuid::Uuid::new_v4()),
            text,
            chrono::Utc::now().timestamp_millis(),
        );
        store.append_message(&self.conversation_id, msg.clone()).await?;
        self.history.push(msg);
        Ok(())
    }

    /// 追加 assistant 消息（含 tool_calls + tool_records）。
    pub async fn push_assistant(
        &mut self,
        store: &SessionStore,
        msg: ChatMessage,
    ) -> Result<(), String> {
        store
            .append_message(&self.conversation_id, msg.clone())
            .await?;
        self.history.push(msg);
        Ok(())
    }

    /// 追加 tool 结果消息。
    pub async fn push_tool(
        &mut self,
        store: &SessionStore,
        msg: ChatMessage,
    ) -> Result<(), String> {
        store
            .append_message(&self.conversation_id, msg.clone())
            .await?;
        self.history.push(msg);
        Ok(())
    }

    /// history 转 LLM 请求格式（`Vec<Message>`）。
    ///
    /// 丢弃 ChatMessage 的 `id` / `tool_records` / `created_at`（不进 LLM API）。
    pub fn to_llm_messages(&self) -> Vec<Message> {
        self.history
            .iter()
            .map(|m| Message {
                role: m.role.clone(),
                content: m.content.clone(),
                tool_calls: m.tool_calls.clone(),
                tool_call_id: m.tool_call_id.clone(),
            })
            .collect()
    }
}

/// 构造工具注册表（Phase 2 内置 10 个工具）。
///
/// 从 `AgentLoop::build_tool_registry` 迁移，逻辑不变。
fn build_tool_registry() -> ToolRegistry {
    use crate::agent::tools::{
        ask_user::AskUserTool,
        background::{BashOutputTool, KillBackgroundTool},
        bash::BashTool,
        edit::EditTool,
        glob::GlobTool,
        grep::GrepTool,
        ls::LsTool,
        read::ReadTool,
        write::WriteTool,
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

/// Phase 3.2 主入口：无状态自由函数。
///
/// design.md §6.1。从 Phase 2 `AgentLoop::run_inner` 迁移逻辑：
/// - `&self.history` → `&mut session.history`（无锁）
/// - `self.transition` → `app_state.set_session_state`
/// - 每条消息 push 时同时持久化到 SessionStore
///
/// `app`：用于 `emit_status` / `emit_error` / `emit_stream_done`（host trait 不含这些）。
/// `None` 时跳过 emit（测试用）。
#[allow(clippy::too_many_arguments)]
pub async fn run_agent_loop(
    config: AgentRunConfig,
    host: Arc<dyn AgentHost>,
    app: Option<AppHandle>,
    session: &mut SessionRunner,
    session_store: &SessionStore,
    app_state: &AppState,
    mcp_manager: Option<&Arc<McpManager>>,
    settings: &Settings,
) -> Result<AgentRunResult, String> {
    let conv_id = session.conversation_id.clone();
    let run_id = session.run_id.clone();
    let assistant_id = session.message_id.clone();
    let generation = session.generation;

    // 1. Idle → Prepare
    app_state.set_session_state(&conv_id, AgentState::Prepare);
    emit_status(app.as_ref(), &conv_id, AgentState::Prepare);

    // 2. 构造 provider + tool registry
    let anthropic_cfg = match AnthropicConfig::from_env() {
        Ok(cfg) => cfg,
        Err(e) => {
            let msg = format!("配置错误: {e}. 请在设置中配置 LLM_API_KEY 或在 .env 文件中设置.");
            emit_error(app.as_ref(), &conv_id, &assistant_id, &msg);
            app_state.set_session_state(&conv_id, AgentState::Idle);
            emit_status(app.as_ref(), &conv_id, AgentState::Idle);
            return Err(msg);
        }
    };
    let provider = AnthropicClient::new(anthropic_cfg);
    let tools = build_tool_registry();
    let mut tool_defs = tools.definitions();

    // 3. 合并 MCP tools（Phase 3.1）
    if let Some(mcp_mgr) = mcp_manager {
        let mcp_defs = mcp_mgr.list_all_tools(settings).await;
        tool_defs.extend(mcp_defs);
    }

    // 4. ToolLoop
    app_state.set_session_state(&conv_id, AgentState::ToolLoop);
    emit_status(app.as_ref(), &conv_id, AgentState::ToolLoop);

    let mut all_tool_records: Vec<ToolCallRecord> = Vec::new();
    let mut final_text = String::new();
    let mut round: u32 = 0;

    loop {
        if round >= config.max_tool_rounds {
            warn!("hit max_tool_rounds={}", config.max_tool_rounds);
            break;
        }
        round += 1;

        // cancel 检查点（per-conv generation）
        if !app_state.is_generation_active(&conv_id, generation) {
            debug!("generation cancelled at round {round}");
            break;
        }

        let snapshot = session.to_llm_messages();
        info!(
            "round {round} start: requesting LLM (history len={})",
            snapshot.len()
        );

        let req = MessagesRequest {
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            messages: snapshot,
            system: Some(config.system_prompt.clone()),
            stream: true,
            tools: tool_defs.clone(),
        };

        let stream_result = provider.stream_chat(req).await;
        let mut stream = match stream_result {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("request failed: {e}");
                emit_error(app.as_ref(), &conv_id, &assistant_id, &msg);
                app_state.set_session_state(&conv_id, AgentState::Stop);
                emit_status(app.as_ref(), &conv_id, AgentState::Stop);
                app_state.set_session_state(&conv_id, AgentState::Idle);
                emit_status(app.as_ref(), &conv_id, AgentState::Idle);
                return Err(msg);
            }
        };

        let round_resp = consume_stream(
            &mut stream,
            &app,
            &conv_id,
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

        if !round_resp.tool_uses.is_empty() {
            let ctx = ToolContext {
                conversation_id: conv_id.clone(),
                run_id: run_id.clone(),
                message_id: assistant_id.clone(),
                tool_call_id: String::new(),
                round,
                generation,
            };

            info!(
                "round {round}: dispatching {} tool(s): {:?}",
                round_resp.tool_uses.len(),
                round_resp
                    .tool_uses
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
            );

            let tool_results = crate::agent::rounds::dispatch_round(
                &tools,
                mcp_manager,
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

            // 持久化：assistant tool_use message + 每个 tool_result
            let assistant_msg = ChatMessage {
                id: format!("msg_{}", uuid::Uuid::new_v4()),
                role: "assistant".into(),
                content: if round_resp.text.is_empty() {
                    None
                } else {
                    Some(round_resp.text.clone())
                },
                tool_calls: Some(
                    round_resp
                        .tool_uses
                        .iter()
                        .map(|tu| OpenAiToolCall {
                            id: tu.id.clone(),
                            call_type: "function".into(),
                            function: OpenAiFunction {
                                name: tu.name.clone(),
                                arguments: serde_json::to_string(&tu.input)
                                    .unwrap_or_else(|_| "{}".into()),
                            },
                        })
                        .collect(),
                ),
                tool_call_id: None,
                tool_records: None,
                created_at: chrono::Utc::now().timestamp_millis(),
            };
            session
                .push_assistant(session_store, assistant_msg)
                .await?;

            for tr in &tool_results {
                let body = match &tr.kind {
                    crate::agent::rounds::ToolResultKind::Success { content } => content.clone(),
                    crate::agent::rounds::ToolResultKind::Error { message } => message.clone(),
                    crate::agent::rounds::ToolResultKind::Denied { reason } => {
                        format!("permission denied: {reason}")
                    }
                };
                let tool_msg = ChatMessage {
                    id: format!("msg_{}", uuid::Uuid::new_v4()),
                    role: "tool".into(),
                    content: Some(body),
                    tool_calls: None,
                    tool_call_id: Some(tr.tool_use_id.clone()),
                    tool_records: None,
                    created_at: chrono::Utc::now().timestamp_millis(),
                };
                session.push_tool(session_store, tool_msg).await?;
            }

            // persist partial（host trait stub，Phase 3.2 暂 no-op）
            let api_msgs = build_tool_round_api_messages(
                &round_resp.text,
                &round_resp.tool_uses,
                &tool_results,
            );
            host.persist_partial_assistant(&conv_id, &run_id, &assistant_id, &all_tool_records, &api_msgs);

            if round_resp.text.is_empty() && round_resp.tool_uses.is_empty() {
                warn!("round {round}: empty response (no text, no tool_use), breaking");
                break;
            }
            continue;
        } else {
            final_text = round_resp.text;
            break;
        }
    }

    // 5. Stop → Idle
    emit_stream_done(app.as_ref(), &conv_id, &run_id, &assistant_id, "end_turn");
    app_state.set_session_state(&conv_id, AgentState::Stop);
    emit_status(app.as_ref(), &conv_id, AgentState::Stop);

    // 6. 持久化 final assistant 文本
    if !final_text.is_empty() {
        let msg = ChatMessage::assistant_text(
            format!("msg_{}", uuid::Uuid::new_v4()),
            final_text.clone(),
            chrono::Utc::now().timestamp_millis(),
        );
        session.push_assistant(session_store, msg).await?;
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

    app_state.set_session_state(&conv_id, AgentState::Idle);
    emit_status(app.as_ref(), &conv_id, AgentState::Idle);
    Ok(result)
}

/// 消费单个 stream，累积 text + tool_use。
///
/// 从 `AgentLoop::consume_stream` 迁移，逻辑不变。
///
/// Phase 3.2：加 `conversation_id` 参数，传给 host emit 方法（per-conv 路由）。
#[allow(clippy::too_many_arguments)]
async fn consume_stream(
    stream: &mut crate::providers::TokenStream,
    _app: &Option<AppHandle>,
    conversation_id: &str,
    run_id: &str,
    message_id: &str,
    host: &Arc<dyn AgentHost>,
    round: u32,
    all_records: &mut Vec<ToolCallRecord>,
) -> RoundResponse {
    let mut text = String::new();
    let mut tool_uses: Vec<ToolUseBlock> = Vec::new();
    let mut stop_reason: Option<String> = None;
    let mut current_tool: Option<usize> = None;

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
                    host.emit_stream_delta(conversation_id, run_id, message_id, &delta, None);
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
                    let started_at = chrono::Utc::now().timestamp();

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
                        sensitive: false,
                        artifacts: vec![],
                        structured_content: None,
                    };
                    host.emit_tool_record(conversation_id, run_id, message_id, &record);
                    all_records.push(record);
                }
                StreamChunk::ToolUseInputDelta(delta) => {
                    if let Some(idx) = current_tool {
                        debug!(
                            "SSE tool_use input delta: tool_idx={}, delta_len={}, raw_so_far={}",
                            idx,
                            delta.len(),
                            tool_uses[idx].input_raw.len() + delta.len()
                        );
                        tool_uses[idx].input_raw.push_str(&delta);
                    } else {
                        warn!(
                            "SSE tool_use input delta but no current_tool (delta_len={})",
                            delta.len()
                        );
                    }
                }
                StreamChunk::ToolUseEnd => {
                    info!(
                        "SSE tool_use end: parsing {} accumulated tool_use(s)",
                        tool_uses.len()
                    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn session_runner_new_initializes_fields() {
        let runner = SessionRunner::new(
            "conv_abc".into(),
            "run_1".into(),
            "msg_1".into(),
            vec![],
            42,
        );
        assert_eq!(runner.conversation_id, "conv_abc");
        assert_eq!(runner.run_id, "run_1");
        assert_eq!(runner.message_id, "msg_1");
        assert_eq!(runner.generation, 42);
        assert!(runner.history.is_empty());
    }

    #[tokio::test]
    async fn push_user_appends_to_history_and_persists() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::new(dir.path().join("sessions"));
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();
        let conv_id = conv.id.clone();

        let mut runner = SessionRunner::new(conv.id, "run_1".into(), "msg_1".into(), vec![], 1);
        runner.push_user(&store, "hello world").await.unwrap();

        // 内存 history 更新
        assert_eq!(runner.history.len(), 1);
        assert_eq!(runner.history[0].role, "user");
        assert_eq!(runner.history[0].content.as_deref(), Some("hello world"));

        // 磁盘持久化（新 store 模拟重启）
        let store2 = SessionStore::new(dir.path().join("sessions"));
        store2.load_index().await.unwrap();
        let messages = store2.load_messages(&conv_id).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content.as_deref(), Some("hello world"));
    }

    #[tokio::test]
    async fn push_assistant_appends_to_history() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::new(dir.path().join("sessions"));
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();

        let mut runner = SessionRunner::new(conv.id, "run_1".into(), "msg_1".into(), vec![], 1);
        let msg = ChatMessage::assistant_text("msg_2", "hi there", 1000);
        runner.push_assistant(&store, msg).await.unwrap();

        assert_eq!(runner.history.len(), 1);
        assert_eq!(runner.history[0].role, "assistant");
        assert_eq!(runner.history[0].content.as_deref(), Some("hi there"));
    }

    #[tokio::test]
    async fn push_tool_appends_to_history() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::new(dir.path().join("sessions"));
        store.load_index().await.unwrap();
        let conv = store.create_session().await.unwrap();

        let mut runner = SessionRunner::new(conv.id, "run_1".into(), "msg_1".into(), vec![], 1);
        let msg = ChatMessage {
            id: "msg_tool".into(),
            role: "tool".into(),
            content: Some("result body".into()),
            tool_calls: None,
            tool_call_id: Some("call_x".into()),
            tool_records: None,
            created_at: 1000,
        };
        runner.push_tool(&store, msg).await.unwrap();

        assert_eq!(runner.history.len(), 1);
        assert_eq!(runner.history[0].role, "tool");
        assert_eq!(runner.history[0].tool_call_id.as_deref(), Some("call_x"));
    }

    #[test]
    fn to_llm_messages_converts_format() {
        let history = vec![
            ChatMessage::user("msg_1", "hello", 1000),
            ChatMessage::assistant_text("msg_2", "hi", 2000),
        ];
        let runner = SessionRunner::new("conv_x".into(), "run_1".into(), "msg_2".into(), history, 1);

        let llm_msgs = runner.to_llm_messages();
        assert_eq!(llm_msgs.len(), 2);
        assert_eq!(llm_msgs[0].role, "user");
        assert_eq!(llm_msgs[0].content.as_deref(), Some("hello"));
        assert!(llm_msgs[0].tool_calls.is_none());
        assert!(llm_msgs[0].tool_call_id.is_none());

        assert_eq!(llm_msgs[1].role, "assistant");
        assert_eq!(llm_msgs[1].content.as_deref(), Some("hi"));
    }

    #[test]
    fn to_llm_messages_preserves_tool_calls() {
        let history = vec![ChatMessage {
            id: "msg_1".into(),
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![OpenAiToolCall {
                id: "call_x".into(),
                call_type: "function".into(),
                function: OpenAiFunction {
                    name: "read_file".into(),
                    arguments: "{\"path\":\"a.rs\"}".into(),
                },
            }]),
            tool_call_id: None,
            tool_records: None,
            created_at: 1000,
        }];
        let runner = SessionRunner::new("conv_x".into(), "run_1".into(), "msg_1".into(), history, 1);

        let llm_msgs = runner.to_llm_messages();
        assert_eq!(llm_msgs.len(), 1);
        let tc = llm_msgs[0].tool_calls.as_ref().unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].function.name, "read_file");
    }

    #[test]
    fn to_llm_messages_preserves_tool_result() {
        let history = vec![ChatMessage {
            id: "msg_tool".into(),
            role: "tool".into(),
            content: Some("file content".into()),
            tool_calls: None,
            tool_call_id: Some("call_x".into()),
            tool_records: None,
            created_at: 1000,
        }];
        let runner = SessionRunner::new("conv_x".into(), "run_1".into(), "msg_1".into(), history, 1);

        let llm_msgs = runner.to_llm_messages();
        assert_eq!(llm_msgs.len(), 1);
        assert_eq!(llm_msgs[0].role, "tool");
        assert_eq!(llm_msgs[0].tool_call_id.as_deref(), Some("call_x"));
        assert_eq!(llm_msgs[0].content.as_deref(), Some("file content"));
    }

    #[test]
    fn build_tool_registry_registers_ten_tools() {
        let registry = build_tool_registry();
        // 验证至少注册了 read_file（不验证全部，避免随注册数变化）
        assert!(registry.by_name("read_file").is_some());
        assert!(registry.by_name("write_file").is_some());
        assert!(registry.by_name("ask_user").is_some());
    }
}
