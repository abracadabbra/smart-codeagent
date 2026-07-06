// Prevent additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let _ = dotenvy::dotenv();
    smart_codeagent_lib::init_tracing();
    smart_codeagent_lib::run();
}