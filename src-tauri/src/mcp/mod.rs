//! MCP（Model Context Protocol）集成模块。
//!
//! Phase 3.1：stdio transport，参照 Kivio `src-tauri/src/mcp/` 的瘦版。
//! Phase 5.6：新增 HTTP/SSE transport（`http_client.rs`）。
//!
//! 模块组织：
//! - `types`：协议数据结构 + 转换函数（无 IO、无并发，可独立单测）
//! - `client`：`StdioMcpClient` — 单 server 持久会话 + reader_task + 单飞门闩
//! - `http_client`：`HttpMcpClient` — HTTP/SSE 长连接 + POST/事件路由
//! - `manager`：`McpManager` — 多 server 协调 + 状态事件 emit

pub mod client;
pub mod http_client;
pub mod manager;
pub mod types;

pub use client::{McpEventSink, McpSession, StdioMcpClient};
pub use http_client::HttpMcpClient;
pub use manager::{McpManager, TauriEventSink};
pub use types::{
    McpServerState, McpServerStatePayload, McpTool, McpToolCallResult, looks_sensitive_tool,
    parse_mcp_name, parse_tool_result, tool_definition_from_mcp,
};
