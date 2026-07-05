//! Tauri Command handler。
//!
//! Phase 2: send_message 接受 run_id + generation（前端唯一生成，避免后端重复）；
//! 新增 approve_tool / answer_ask_user 两个 command。
//! Phase 3.1: 新增 list_mcp_servers / list_mcp_server_states。

use std::collections::HashMap;
use std::sync::Arc;

use serde::Deserialize;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;

use crate::agent::host_impl::TauriHost;
use crate::agent::loop_::AgentLoop;
use crate::agent::tools::AskUserResponseResult;
use crate::mcp::{McpManager, McpServerState};
use crate::settings::{ChatMcpServer, Settings};

/// Phase 1 兼容 + Phase 2：用户发消息，启动一轮 Agent Loop。
/// 立即返回（不阻塞 IPC），实际执行由 tokio::spawn 后台进行。
#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    agent: State<'_, Arc<AgentLoop>>,
    text: String,
    assistant_id: String,
    run_id: String,
    generation: u64,
) -> Result<(), String> {
    tracing::info!(
        "send_message invoked: text={:?}, assistantId={:?}, run_id={:?}",
        text,
        assistant_id,
        run_id
    );

    let agent: Arc<AgentLoop> = (*agent).clone();
    agent.attach_app(app).await;
    agent.spawn_run(text, assistant_id, run_id, generation);

    Ok(())
}

/// 用户对 approval_request 的响应。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveToolArgs {
    pub approval_id: String,
    pub allow: bool,
}

#[tauri::command]
pub async fn approve_tool(
    app: AppHandle,
    args: ApproveToolArgs,
) -> Result<(), String> {
    let host = app
        .try_state::<Arc<TauriHost>>()
        .ok_or_else(|| "TauriHost not managed".to_string())?;
    host.resolve_approval(&args.approval_id, args.allow);
    Ok(())
}

/// 用户对 ask_user prompt 的回答。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerAskUserArgs {
    pub ask_user_id: String,
    pub response: AskUserResponseResult,
}

#[tauri::command]
pub async fn answer_ask_user(
    app: AppHandle,
    args: AnswerAskUserArgs,
) -> Result<(), String> {
    let host = app
        .try_state::<Arc<TauriHost>>()
        .ok_or_else(|| "TauriHost not managed".to_string())?;
    host.resolve_ask_user(&args.ask_user_id, args.response);
    Ok(())
}

/// 用户取消当前 generation。
#[tauri::command]
pub async fn cancel_run(
    app: AppHandle,
    run_id: String,
) -> Result<(), String> {
    let host = app
        .try_state::<Arc<TauriHost>>()
        .ok_or_else(|| "TauriHost not managed".to_string())?;
    host.cancel_generation(&run_id);
    Ok(())
}

/// 列出 settings.json 中配置的所有 MCP server（前端 StatusBar / 设置面板用）。
#[tauri::command]
pub async fn list_mcp_servers(
    app: AppHandle,
) -> Result<Vec<ChatMcpServer>, String> {
    let settings = app
        .try_state::<Arc<Mutex<Settings>>>()
        .ok_or_else(|| "Settings not managed".to_string())?;
    let settings = settings.lock().await;
    Ok(settings.mcp.servers.clone())
}

/// 列出所有 MCP server 的当前连接状态快照（前端 StatusBar 用）。
/// 未在缓存中的 server（未初始化）由前端视作 Disconnected。
#[tauri::command]
pub async fn list_mcp_server_states(
    app: AppHandle,
) -> Result<HashMap<String, McpServerState>, String> {
    let mcp_manager = app
        .try_state::<Arc<McpManager>>()
        .ok_or_else(|| "McpManager not managed".to_string())?;
    Ok(mcp_manager.list_server_states().await)
}