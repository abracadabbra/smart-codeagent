#![allow(dead_code)]

pub mod agent;
pub mod config;
pub mod ipc;
pub mod providers;

use std::sync::Arc;

use tauri::Manager;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::agent::loop_::AgentLoop;
use crate::ipc::commands::send_message;

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
            // 构造共享的 Agent Loop，注入到 managed state
            let agent: Arc<AgentLoop> = Arc::new(AgentLoop::new());
            app.manage(agent);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![send_message])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}