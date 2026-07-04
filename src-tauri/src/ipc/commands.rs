//! Tauri Command handler。

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::agent::loop_::AgentLoop;

/// Phase 1 唯一 command：用户发消息，启动一轮 Agent Loop。
/// 立即返回（不阻塞 IPC），实际执行由 tokio::spawn 后台进行。
#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    agent: State<'_, Arc<AgentLoop>>,
    text: String,
    assistant_id: String,
) -> Result<(), String> {
    tracing::info!(
        "send_message invoked: text={:?}, assistantId={:?}",
        text,
        assistant_id
    );

    let agent: Arc<AgentLoop> = (*agent).clone();
    // 第一次进入时把 AppHandle 灌进 Loop；后续 emit 才有窗口。
    agent.attach_app(app).await;
    agent.spawn_run(text, assistant_id);

    Ok(())
}