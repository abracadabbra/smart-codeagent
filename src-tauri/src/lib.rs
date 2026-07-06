#![allow(dead_code)]

pub mod agent;
pub mod config;
pub mod ipc;
pub mod mcp;
pub mod providers;
pub mod session;
pub mod settings;
pub mod state;

use std::sync::Arc;

use tauri::Manager;
use tokio::sync::Mutex;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::agent::host_impl::TauriHost;
use crate::ipc::commands::{
    answer_ask_user, approve_tool, cancel_run, list_mcp_server_states, list_mcp_servers,
    send_message,
};
use crate::mcp::McpManager;
use crate::session::commands::{
    create_session, delete_session, get_session, get_session_messages, list_sessions,
    search_sessions, update_session,
};
use crate::session::store::SessionStore;
use crate::settings::Settings;
use crate::state::AppState;

/// 初始化全局 tracing subscriber。
pub fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer().with_target(false).compact())
        .init();
}

/// Tauri 入口。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // Phase 3.2: AppState（多 session 并行核心）—— 全局单例
            let app_state = Arc::new(AppState::new());
            app.manage(app_state.clone());

            // 共享 TauriHost：approval / ask_user / cancel 命令通过 try_state 取它，
            // run_agent_loop 也用同一个实例（否则 oneshot sender 永远等不到 command 解析）。
            // Phase 3.2：TauriHost 持有 AppState 的 Arc 引用，用于 per-conv pending 路由。
            let host: Arc<TauriHost> = Arc::new(TauriHost::new(
                app.handle().clone(),
                app_state.clone(),
            ));
            app.manage(host);

            // Phase 3.2: SessionStore（内存缓存 + 写穿磁盘）—— 全局单例
            // 启动时 load_index 把所有会话 meta 灌入内存缓存（messages 懒加载）
            let session_store = Arc::new(SessionStore::from_app(&app.handle())?);
            {
                let store_clone = session_store.clone();
                tauri::async_runtime::block_on(async move {
                    if let Err(e) = store_clone.load_index().await {
                        tracing::warn!("SessionStore load_index 失败（不影响启动）: {e}");
                    }
                });
            }
            app.manage(session_store.clone());

            // Phase 3.1: 冷加载 settings.json + 构造 McpManager（懒连接，首次 list_tools 时才握手）
            let mut settings = Settings::load_from_disk(&app.handle());
            settings.sanitize();
            app.manage(Arc::new(Mutex::new(settings)));

            let mcp_manager = Arc::new(McpManager::new(app.handle().clone()));
            app.manage(mcp_manager);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Phase 1-2 agent 命令
            send_message,
            approve_tool,
            answer_ask_user,
            cancel_run,
            // Phase 3.1 MCP 命令
            list_mcp_servers,
            list_mcp_server_states,
            // Phase 3.2 会话管理命令
            create_session,
            list_sessions,
            get_session,
            get_session_messages,
            update_session,
            delete_session,
            search_sessions
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // 退出时排干所有 MCP 子进程，避免孤儿。kill_on_drop(true) 是兜底，
            // 这里显式 disconnect 更干净（abort reader_task + start_kill）。
            if let tauri::RunEvent::ExitRequested { .. } = event {
                if let Some(mcp_manager) = app_handle.try_state::<Arc<McpManager>>() {
                    let mgr = mcp_manager.inner().clone();
                    tauri::async_runtime::block_on(async move {
                        mgr.disconnect_all().await;
                    });
                }
                // Phase 3.2: flush SessionStore（写穿模式下通常 no-op，但保险）
                if let Some(session_store) = app_handle.try_state::<Arc<SessionStore>>() {
                    let store = session_store.inner().clone();
                    tauri::async_runtime::block_on(async move {
                        if let Err(e) = store.flush_all().await {
                            tracing::warn!("SessionStore flush_all 失败: {e}");
                        }
                    });
                }
            }
        });
}
