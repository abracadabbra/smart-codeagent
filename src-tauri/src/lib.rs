#![allow(dead_code)]

pub mod agent;
pub mod config;
pub mod ipc;
pub mod providers;
pub mod settings;

use std::sync::Arc;

use tauri::Manager;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::agent::host_impl::TauriHost;
use crate::agent::loop_::AgentLoop;
use crate::ipc::commands::{answer_ask_user, approve_tool, cancel_run, send_message};

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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            send_message,
            approve_tool,
            answer_ask_user,
            cancel_run
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}