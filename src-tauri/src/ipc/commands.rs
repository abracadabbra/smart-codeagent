//! Tauri Command handler。
//!
//! Phase 3.2 重构：
//! - `send_message`：加 `conversation_id`，改用 `run_agent_loop` + `SessionRunner`
//! - `cancel_run` / `approve_tool` / `answer_ask_user`：加 `conversation_id`
//! - 返回 `SendResult`（busy 时 `success: false`）
//! - 删除 `State<'_, Arc<AgentLoop>>` 依赖
//! - Phase 3.1: 保留 `list_mcp_servers` / `list_mcp_server_states`

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;

use crate::agent::host_impl::TauriHost;
use crate::agent::runner::{run_agent_loop, SessionRunner};
use crate::agent::tools::AskUserResponseResult;
use crate::agent::types::AgentRunConfig;
use crate::mcp::{McpManager, McpServerState};
use crate::session::store::SessionStore;
use crate::settings::{ChatMcpServer, Settings};
use crate::state::{AppState, ChatSendReservation};

/// `send_message` 返回值。
///
/// `success: false` 表示该会话已有 run 在跑（busy）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 用户发消息，启动一轮 Agent Loop。
///
/// Phase 3.2：加 `conversation_id`，后端生成 `generation`，
/// 改用 `run_agent_loop` + `SessionRunner`。立即返回（不阻塞 IPC），
/// 实际执行由 `tokio::spawn` 后台进行。
///
/// `message_id` 由前端生成（前端创建 pending assistant 消息时生成），
/// 后端用它作为 emit 事件的 msg_id，确保前端能正确路由 stream_delta 到对应消息。
#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    app_state: State<'_, Arc<AppState>>,
    session_store: State<'_, Arc<SessionStore>>,
    conversation_id: String,
    text: String,
    run_id: String,
    message_id: String,
) -> Result<SendResult, String> {
    tracing::info!(
        "send_message invoked: conv={}, text={:?}, run_id={:?}, msg_id={:?}",
        conversation_id,
        text,
        run_id,
        message_id
    );

    let app_state_arc = app_state.inner().clone();
    let session_store_arc = session_store.inner().clone();

    // 1. busy 守门（ChatSendReservation，drop 时自动释放）
    let reservation = match ChatSendReservation::try_acquire(app_state_arc.clone(), &conversation_id)
    {
        Some(r) => r,
        None => {
            return Ok(SendResult {
                success: false,
                error: Some("session busy".into()),
            })
        }
    };

    // 2. generation（message_id 由前端传入，用于 emit 事件路由）
    let generation = app_state_arc.new_run_generation(&conversation_id);

    // 3. 加载 history + push user 消息（失败时清理 generation + reservation）
    let history = match session_store_arc.load_messages(&conversation_id).await {
        Ok(h) => h,
        Err(e) => {
            app_state_arc.end_generation(&conversation_id, generation);
            return Err(e);
        }
    };

    let mut session = SessionRunner::new(
        conversation_id.clone(),
        run_id.clone(),
        message_id.clone(),
        history,
        generation,
    );
    if let Err(e) = session.push_user(&session_store_arc, &text).await {
        app_state_arc.end_generation(&conversation_id, generation);
        return Err(e);
    }

    // 3.5 自动标题：若 title 仍是默认值，用首条消息内容前 N 字符更新
    if let Ok(meta) = session_store_arc.get_meta(&conversation_id).await {
        if meta.title == "New Session" {
            let auto_title: String = text
                .trim()
                .chars()
                .take(40)
                .collect::<String>()
                .trim()
                .to_string();
            if !auto_title.is_empty() {
                if let Ok(updated) = session_store_arc
                    .update_meta(&conversation_id, Some(&auto_title), None)
                    .await
                {
                    crate::ipc::events::emit_session_updated(Some(&app), &updated);
                }
            }
        }
    }

    // 4. 获取 host + config + settings + mcp_manager
    let host: Arc<dyn crate::agent::host::AgentHost> = match app.try_state::<Arc<TauriHost>>() {
        Some(h) => h.inner().clone(),
        None => {
            app_state_arc.end_generation(&conversation_id, generation);
            return Err("TauriHost not managed".into());
        }
    };
    let settings = match app.try_state::<Arc<Mutex<Settings>>>() {
        Some(s) => s.lock().await.clone(),
        None => {
            app_state_arc.end_generation(&conversation_id, generation);
            return Err("Settings not managed".into());
        }
    };
    let mcp_manager = app
        .try_state::<Arc<McpManager>>()
        .map(|m| m.inner().clone());

    // 5. spawn run_agent_loop（后台执行，不阻塞 IPC）
    let app_clone = app.clone();
    let conv_id = conversation_id.clone();
    let run_id_clone = run_id.clone();

    tokio::spawn(async move {
        // reservation 持有到 run 结束（drop 时释放 busy 槽位）
        let _reservation = reservation;
        let config = AgentRunConfig::from_settings(&settings.provider);
        let result = run_agent_loop(
            config,
            host,
            Some(app_clone),
            &mut session,
            &session_store_arc,
            &app_state_arc,
            mcp_manager.as_ref(),
            &settings,
        )
        .await;

        // 自然结束：移除 generation（区别于 cancel 的清空全部）
        app_state_arc.end_generation(&conv_id, generation);

        match &result {
            Ok(r) => tracing::info!(
                "run {} completed: rounds={}, tool_calls={}",
                run_id_clone,
                r.rounds,
                r.tool_records.len()
            ),
            Err(e) => tracing::error!("run {} failed: {}", run_id_clone, e),
        }
        // _reservation drop → end_chat_reply（释放 busy）
    });

    Ok(SendResult {
        success: true,
        error: None,
    })
}

/// 用户对 approval_request 的响应。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveToolArgs {
    pub conversation_id: String,
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
    host.resolve_approval(&args.conversation_id, &args.approval_id, args.allow);
    Ok(())
}

/// 用户对 ask_user prompt 的回答。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerAskUserArgs {
    pub conversation_id: String,
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
    host.resolve_ask_user(&args.conversation_id, &args.ask_user_id, args.response);
    Ok(())
}

/// 用户取消当前会话的 generation。
#[tauri::command]
pub async fn cancel_run(
    app_state: State<'_, Arc<AppState>>,
    conversation_id: String,
) -> Result<(), String> {
    app_state.cancel_chat_generation(&conversation_id);
    Ok(())
}

/// 查询某个会话当前的 AgentState（前端用于启动时/会话切换时同步真实状态）。
///
/// 安全网：防止前端因 HMR 或事件丢失导致状态卡在非 Idle。
#[tauri::command]
pub async fn get_session_state(
    app_state: State<'_, Arc<AppState>>,
    conversation_id: String,
) -> Result<String, String> {
    Ok(app_state.get_session_state(&conversation_id).as_str().to_string())
}

/// 列出所有非 Idle 状态的会话（前端启动时同步 + 诊断"卡住"的会话）。
///
/// 返回 `[(conv_id, state_str), ...]`。
#[tauri::command]
pub async fn list_active_sessions(
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<(String, String)>, String> {
    Ok(app_state
        .list_non_idle_sessions()
        .into_iter()
        .map(|(id, s)| (id, s.as_str().to_string()))
        .collect())
}

/// 强制重置某会话为 Idle（解除僵尸状态：前端卡 Running 但后端 run 已不存在）。
#[tauri::command]
pub async fn force_reset_session(
    app_state: State<'_, Arc<AppState>>,
    conversation_id: String,
) -> Result<(), String> {
    app_state.force_reset_session(&conversation_id);
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

/// 保存配置到 settings.json。
/// 前端修改配置后调用此命令，修改会立即持久化到磁盘。
#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    settings: Settings,
) -> Result<(), String> {
    let settings_state = app
        .try_state::<Arc<Mutex<Settings>>>()
        .ok_or_else(|| "Settings not managed".to_string())?;

    let mut settings_guard = settings_state.lock().await;
    *settings_guard = settings.clone();
    drop(settings_guard);

    settings.save_to_disk(&app)
}

/// 热重载配置：从磁盘重新加载 settings.json，然后 McpManager 重新连接所有 server。
/// 无需重启应用即可应用新配置。
#[tauri::command]
pub async fn reload_settings(
    app: AppHandle,
) -> Result<Vec<ChatMcpServer>, String> {
    let settings_state = app
        .try_state::<Arc<Mutex<Settings>>>()
        .ok_or_else(|| "Settings not managed".to_string())?;

    let mut settings = settings_state.lock().await;
    settings.reload_from_disk(&app);
    let servers = settings.mcp.servers.clone();

    let mcp_manager = app
        .try_state::<Arc<McpManager>>()
        .ok_or_else(|| "McpManager not managed".to_string())?;

    mcp_manager.reconnect_all(&settings).await;

    Ok(servers)
}

/// 测试单个 MCP server 是否能正常连接（不注册到池，测试完毕立即断开）。
/// 用于前端"测试连接"按钮。
#[tauri::command]
pub async fn test_mcp_server(
    app: AppHandle,
    server: ChatMcpServer,
) -> Result<(), String> {
    let mcp_manager = app
        .try_state::<Arc<McpManager>>()
        .ok_or_else(|| "McpManager not managed".to_string())?;

    mcp_manager.test_connection(&server).await
}

/// 获取完整 settings 配置（用于前端读取 theme 等全局设置）。
#[tauri::command]
pub async fn get_settings(
    app: AppHandle,
) -> Result<String, String> {
    let settings_state = app
        .try_state::<Arc<Mutex<Settings>>>()
        .ok_or_else(|| "Settings not managed".to_string())?;

    let settings = settings_state.lock().await.clone();
    serde_json::to_string(&settings)
        .map_err(|e| format!("序列化 settings 失败: {e}"))
}
