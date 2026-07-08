//! E2E 调试测试：直接调用 `run_agent_loop`，绕过 Tauri IPC + 前端。
//!
//! 目的：定位"发消息无响应"的 Bug 是在后端层还是 IPC/前端层。
//!
//! 运行方式：
//! ```sh
//! cd src-tauri
//! cargo test --test runner_e2e_debug -- --nocapture --ignored
//! ```

use std::sync::Arc;

use smart_codeagent_lib::agent::host::{AgentHost, AgentHostFuture};
use smart_codeagent_lib::agent::runner::{SessionRunner, run_agent_loop};
use smart_codeagent_lib::agent::tools::{
    AskUserPromptPayload, AskUserResponseResult, ToolCallRecord,
};
use smart_codeagent_lib::agent::types::AgentRunConfig;
use smart_codeagent_lib::session::store::SessionStore;
use smart_codeagent_lib::settings::Settings;
use smart_codeagent_lib::state::AppState;

use tempfile::TempDir;

/// Mock AgentHost：不依赖 Tauri AppHandle，只记录 emit 调用。
struct MockHost {
    app_state: Arc<AppState>,
    deltas: std::sync::Mutex<Vec<String>>,
    dones: std::sync::Mutex<Vec<String>>,
}

impl MockHost {
    fn new(app_state: Arc<AppState>) -> Self {
        Self {
            app_state,
            deltas: std::sync::Mutex::new(Vec::new()),
            dones: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl AgentHost for MockHost {
    fn emit_stream_delta(
        &self,
        _conversation_id: &str,
        _run_id: &str,
        _message_id: &str,
        delta: &str,
        _reasoning_delta: Option<&str>,
    ) {
        self.deltas.lock().unwrap().push(delta.to_string());
        eprintln!("[MOCK] emit_stream_delta: len={}", delta.len());
    }

    fn emit_stream_done(
        &self,
        _conversation_id: &str,
        _run_id: &str,
        _message_id: &str,
        reason: &str,
    ) {
        self.dones.lock().unwrap().push(reason.to_string());
        eprintln!("[MOCK] emit_stream_done: reason={}", reason);
    }

    fn emit_tool_record(
        &self,
        _conversation_id: &str,
        _run_id: &str,
        _message_id: &str,
        record: &ToolCallRecord,
    ) {
        eprintln!(
            "[MOCK] emit_tool_record: name={}, status={:?}",
            record.name, record.status
        );
    }

    fn request_tool_approval<'a>(
        &'a self,
        _ctx: &'a smart_codeagent_lib::agent::tools::ToolContext,
        _record: &'a ToolCallRecord,
    ) -> AgentHostFuture<'a, bool> {
        Box::pin(async { true })
    }

    fn request_ask_user<'a>(
        &'a self,
        _ctx: &'a smart_codeagent_lib::agent::tools::ToolContext,
        _payload: &'a AskUserPromptPayload,
    ) -> AgentHostFuture<'a, AskUserResponseResult> {
        Box::pin(async { AskUserResponseResult::default() })
    }

    fn persist_partial_assistant(
        &self,
        _conversation_id: &str,
        _run_id: &str,
        _message_id: &str,
        _records: &[ToolCallRecord],
        _api_messages: &[serde_json::Value],
    ) {
    }

    fn is_generation_active(&self, conversation_id: &str, generation: u64) -> bool {
        self.app_state
            .is_generation_active(conversation_id, generation)
    }
}

#[tokio::test]
#[ignore = "需要真实 LLM API + .env，手动运行：cargo test --test runner_e2e_debug -- --nocapture --ignored"]
async fn run_agent_loop_e2e_debug() {
    // 加载 .env
    let _ = dotenvy::dotenv();

    eprintln!("=== E2E DEBUG TEST START ===");
    eprintln!(
        "[ENV] LLM_API_KEY set: {}",
        std::env::var("LLM_API_KEY").is_ok()
    );
    eprintln!(
        "[ENV] LLM_BASE_URL: {}",
        std::env::var("LLM_BASE_URL").unwrap_or_default()
    );
    eprintln!(
        "[ENV] LLM_MODEL: {}",
        std::env::var("LLM_MODEL").unwrap_or_default()
    );

    // 1. 准备 SessionStore（临时目录）
    let tmp = TempDir::new().expect("tempdir");
    let store = Arc::new(SessionStore::new(tmp.path().join("sessions")));
    store.load_index().await.expect("load_index");
    let conv = store.create_session().await.expect("create_session");
    let conv_id = conv.id.clone();
    eprintln!("[SETUP] created session: {}", conv_id);

    // 2. 准备 AppState
    let app_state = Arc::new(AppState::new());

    // 3. 准备 SessionRunner
    let history = store.load_messages(&conv_id).await.expect("load_messages");
    eprintln!("[SETUP] history len: {}", history.len());
    let generation = app_state.new_run_generation(&conv_id);
    let mut session = SessionRunner::new(
        conv_id.clone(),
        "run_test_1".into(),
        "msg_test_1".into(),
        history,
        generation,
    );
    session
        .push_user(&store, "say hello in one word")
        .await
        .expect("push_user");
    eprintln!("[SETUP] pushed user message");

    // 4. 准备 host + config + settings
    let host: Arc<dyn AgentHost> = Arc::new(MockHost::new(app_state.clone()));
    let config = AgentRunConfig::default();
    let settings = Settings::default();

    eprintln!("[RUN] calling run_agent_loop...");
    let result = run_agent_loop(
        config,
        host,
        None, // app = None (no Tauri AppHandle)
        &mut session,
        &store,
        &app_state,
        None, // no MCP
        &settings,
    )
    .await;

    match &result {
        Ok(r) => {
            eprintln!(
                "[RESULT] OK: rounds={}, final_text_len={}, tool_records={}",
                r.rounds,
                r.final_text.len(),
                r.tool_records.len()
            );
            eprintln!("[RESULT] final_text: {:?}", r.final_text);
        }
        Err(e) => {
            eprintln!("[RESULT] ERR: {}", e);
        }
    }

    // 5. 检查持久化
    let messages = store
        .load_messages(&conv_id)
        .await
        .expect("load_messages after run");
    eprintln!("[PERSIST] messages.jsonl count: {}", messages.len());
    for (i, m) in messages.iter().enumerate() {
        eprintln!(
            "[PERSIST] msg[{}]: role={}, content_len={}",
            i,
            m.role,
            m.content.as_ref().map(|s| s.len()).unwrap_or(0)
        );
    }

    eprintln!("=== E2E DEBUG TEST END ===");

    assert!(result.is_ok(), "run_agent_loop should succeed");
}
