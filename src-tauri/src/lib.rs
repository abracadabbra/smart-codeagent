#![allow(dead_code)]

pub mod agent;
pub mod config;
pub mod ipc;
pub mod mcp;
pub mod providers;
pub mod settings;

use std::sync::Arc;

use tauri::Manager;
use tokio::sync::Mutex;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::agent::host_impl::TauriHost;
use crate::agent::loop_::AgentLoop;
use crate::ipc::commands::{
    answer_ask_user, approve_tool, cancel_run, list_mcp_server_states, list_mcp_servers,
    send_message,
};
use crate::mcp::McpManager;
use crate::settings::Settings;

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
            // 共享 TauriHost：approval / ask_user / cancel 命令通过 try_state 取它，
            // loop 也用同一个实例（否则 oneshot sender 永远等不到 command 解析）。
            let host: Arc<TauriHost> = Arc::new(TauriHost::new(app.handle().clone()));
            app.manage(host);

            // 构造共享的 Agent Loop，注入到 managed state
            let agent: Arc<AgentLoop> = Arc::new(AgentLoop::new(crate::agent::types::AgentRunConfig::default()));
            app.manage(agent);

            // Phase 3.1: 冷加载 settings.json + 构造 McpManager（懒连接，首次 list_tools 时才握手）
            let mut settings = Settings::load_from_disk(&app.handle());
            settings.sanitize();
            app.manage(Arc::new(Mutex::new(settings)));

            let mcp_manager = Arc::new(McpManager::new(app.handle().clone()));
            app.manage(mcp_manager);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_message,
            approve_tool,
            answer_ask_user,
            cancel_run,
            list_mcp_servers,
            list_mcp_server_states
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
            }
        });
}