//! HTTP/SSE transport MCP client。
//!
//! MCP 2025-06-18 协议规定 HTTP transport 使用 SSE（Server-Sent Events）承载
//! server→client 消息，client→server 消息使用 HTTP POST 到 SSE 握手时返回的
//! endpoint URL。
//!
//! 数据流：
//! 1. GET <server.url> (Accept: text/event-stream)
//! 2. 服务端发送 `event: endpoint\ndata: <post-url>`
//! 3. 客户端把该 endpoint 作为后续 JSON-RPC POST 的目标
//! 4. 服务端通过 `event: message\ndata: <json-rpc>` 返回响应
//!
//! 本模块与 `client.rs::StdioMcpClient` 保持相同对外接口，方便 `manager.rs` 统一调度。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use reqwest::{Client, RequestBuilder, header};
use serde_json::Value;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use crate::settings::ChatMcpServer;

use super::client::{McpEventSink, initialize_params};
use super::types::{McpServerState, McpTool, McpToolCallResult, parse_tool_result};

/// 解析 SSE 流中的单条事件。
///
/// 返回 `(event_name, data)`，如果当前块不完整则返回 None。
fn parse_sse_event(chunk: &str) -> Option<(String, String)> {
    let mut event = String::from("message");
    let mut data = String::new();
    let mut found_data = false;

    for line in chunk.lines() {
        if line.starts_with("event:") {
            event = line["event:".len()..].trim().to_string();
        } else if line.starts_with("data:") {
            if found_data {
                data.push('\n');
            }
            data.push_str(line["data:".len()..].trim_start());
            found_data = true;
        } else if line.is_empty() {
            // 空行表示事件结束
            if found_data {
                return Some((event, data));
            }
            event = String::from("message");
            data.clear();
            found_data = false;
        }
    }

    // 没有遇到空行说明当前块不完整，等待后续字节。
    None
}

/// HTTP/SSE 会话：持有 endpoint URL、在途请求映射、SSE reader task。
pub struct HttpMcpSession {
    /// POST JSON-RPC 消息的目标地址（SSE 握手时由服务端下发）。
    endpoint: String,
    next_id: AtomicU64,
    pending: Arc<Mutex<std::collections::HashMap<u64, oneshot::Sender<Result<Value, String>>>>>,
    reader_task: JoinHandle<()>,
    /// 握手 `initialize` 返回的 `serverInfo`。
    pub server_info: Option<Value>,
    /// 握手 `initialize` 返回的 `capabilities`。
    pub capabilities: Option<Value>,
    timeout: Duration,
    http_client: Client,
    base_url: String,
    headers: Option<std::collections::HashMap<String, String>>,
}

impl Drop for HttpMcpSession {
    fn drop(&mut self) {
        self.reader_task.abort();
    }
}

impl HttpMcpSession {
    /// 发一次 JSON-RPC 请求并等待匹配 id 的响应。
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }

        let mut message = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        if !params.is_null() {
            message["params"] = params;
        }

        if let Err(err) = self.post_message(&message).await {
            self.pending.lock().await.remove(&id);
            return Err(err);
        }

        match timeout(self.timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                self.pending.lock().await.remove(&id);
                Err("MCP SSE connection closed".to_string())
            }
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err("MCP HTTP request timed out".to_string())
            }
        }
    }

    /// 发一次 JSON-RPC notification（无 id，不等响应）。
    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let mut message = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
        });
        if !params.is_null() {
            message["params"] = params;
        }
        self.post_message(&message).await
    }

    /// 向 endpoint POST 一条 JSON-RPC 消息。
    async fn post_message(&self, message: &Value) -> Result<(), String> {
        let body = serde_json::to_string(message).map_err(|err| err.to_string())?;
        let mut builder = self
            .http_client
            .post(&self.endpoint)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body);
        builder = apply_headers(builder, self.headers.as_ref());

        timeout(self.timeout, builder.send())
            .await
            .map_err(|_| "MCP HTTP POST timed out".to_string())?
            .map_err(|err| format!("MCP HTTP POST failed: {err}"))?
            .error_for_status()
            .map_err(|err| format!("MCP HTTP POST error status: {err}"))?;
        Ok(())
    }

}

/// 单个 HTTP/SSE MCP server client：持久 session + 单飞门闩 + 死连接重连。
pub struct HttpMcpClient {
    server: ChatMcpServer,
    timeout: Duration,
    sink: Arc<dyn McpEventSink>,
    session: Mutex<Option<Arc<Mutex<HttpMcpSession>>>>,
    handshake_lock: Mutex<()>,
    http_client: Client,
}

impl HttpMcpClient {
    pub fn new(server: ChatMcpServer, sink: Arc<dyn McpEventSink>, timeout_ms: u64) -> Self {
        Self {
            server,
            timeout: Duration::from_millis(timeout_ms.max(1_000)),
            sink,
            session: Mutex::new(None),
            handshake_lock: Mutex::new(()),
            http_client: Client::new(),
        }
    }

    /// 获取或建立连接（单飞门闩）。
    pub async fn connect(&self) -> Result<Arc<Mutex<HttpMcpSession>>, String> {
        if let Some(arc) = self.live_session().await {
            return Ok(arc);
        }

        let _guard = self.handshake_lock.lock().await;
        if let Some(arc) = self.live_session().await {
            return Ok(arc);
        }

        self.sink
            .emit_server_state(&self.server, &McpServerState::Connecting);
        match self.handshake().await {
            Ok(session) => {
                let arc = Arc::new(Mutex::new(session));
                {
                    let mut guard = self.session.lock().await;
                    *guard = Some(arc.clone());
                }
                self.sink
                    .emit_server_state(&self.server, &McpServerState::Connected);
                Ok(arc)
            }
            Err(err) => {
                self.sink.emit_server_state(
                    &self.server,
                    &McpServerState::Error {
                        message: err.clone(),
                    },
                );
                Err(err)
            }
        }
    }

    /// 检查现有 session 是否存活。HTTP/SSE 没有子进程，直接判断 session 是否存在。
    async fn live_session(&self) -> Option<Arc<Mutex<HttpMcpSession>>> {
        let guard = self.session.lock().await;
        guard.as_ref().cloned()
    }

    /// 建立 SSE 连接并完成 initialize 握手。
    async fn handshake(&self) -> Result<HttpMcpSession, String> {
        let url = self
            .server
            .url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
            .ok_or_else(|| "MCP HTTP server URL is empty".to_string())?;

        let base_url = url.trim_end_matches('/').to_string();
        let pending: Arc<Mutex<std::collections::HashMap<u64, oneshot::Sender<Result<Value, String>>>>> =
            Arc::new(Mutex::new(std::collections::HashMap::new()));

        // 1. 建立 SSE 流
        let mut builder = self
            .http_client
            .get(url)
            .header(header::ACCEPT, "text/event-stream");
        builder = apply_headers(builder, self.server.headers.as_ref());

        let response = timeout(self.timeout, builder.send())
            .await
            .map_err(|_| "MCP SSE connect timed out".to_string())?
            .map_err(|err| format!("MCP SSE connect failed: {err}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| format!("status {status}"));
            return Err(format!("MCP SSE connect returned {status}: {body}"));
        }

        // 2. 读取第一个 endpoint 事件
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let endpoint = loop {
            match tokio::time::timeout(self.timeout, stream.next()).await {
                Ok(Some(Ok(bytes))) => {
                    buffer.push_str(&String::from_utf8_lossy(&bytes));
                    if let Some((event, data)) = parse_sse_event(&buffer) {
                        buffer.clear();
                        if event == "endpoint" {
                            break data;
                        }
                    }
                }
                Ok(Some(Err(err))) => {
                    return Err(format!("MCP SSE stream error: {err}"));
                }
                Ok(None) => {
                    return Err("MCP SSE stream closed before endpoint event".to_string());
                }
                Err(_) => {
                    return Err("MCP SSE endpoint event timed out".to_string());
                }
            }
        };

        let endpoint = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
            endpoint
        } else {
            // 相对路径，拼到 base_url
            format!("{}/{}", base_url.trim_end_matches('/'), endpoint.trim_start_matches('/'))
        };

        info!(
            "MCP HTTP server {} SSE endpoint resolved to post URL: {}",
            self.server.id, endpoint
        );

        // 3. 启动 SSE reader task
        let pending_for_reader = pending.clone();
        let reader_task = tokio::spawn(async move {
            let mut buffer = String::new();
            while let Ok(Some(result)) = tokio::time::timeout(Duration::from_secs(30), stream.next()).await {
                match result {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        while let Some((event, data)) = parse_sse_event(&buffer) {
                            // 消费掉已解析的部分（简单截断到倒数第二个换行）
                            if let Some(pos) = buffer.find("\n\n") {
                                buffer = buffer[pos + 2..].to_string();
                            } else {
                                buffer.clear();
                            }

                            if event == "endpoint" {
                                continue;
                            }
                            if event != "message" {
                                debug!("MCP SSE unknown event: {}", event);
                                continue;
                            }

                            let value: Value = match serde_json::from_str(&data) {
                                Ok(v) => v,
                                Err(err) => {
                                    warn!("MCP SSE invalid JSON: {err}");
                                    continue;
                                }
                            };

                            let Some(id) = value.get("id").and_then(|id| id.as_u64()) else {
                                continue;
                            };
                            let sender = {
                                let mut pending = pending_for_reader.lock().await;
                                pending.remove(&id)
                            };
                            if let Some(sender) = sender {
                                let outcome = if let Some(error) = value.get("error") {
                                    Err(format!("MCP error: {}", compact_json(error, 500)))
                                } else {
                                    Ok(value.get("result").cloned().unwrap_or(Value::Null))
                                };
                                let _ = sender.send(outcome);
                            }
                        }
                    }
                    Err(err) => {
                        warn!("MCP SSE stream error: {err}");
                        break;
                    }
                }
            }
            info!("MCP SSE reader task ended");
        });

        // 4. 发送 initialize 握手
        let mut session = HttpMcpSession {
            endpoint,
            next_id: AtomicU64::new(1),
            pending,
            reader_task,
            server_info: None,
            capabilities: None,
            timeout: self.timeout,
            http_client: self.http_client.clone(),
            base_url,
            headers: self.server.headers.clone(),
        };

        let init = session.request("initialize", initialize_params()).await?;
        session
            .notify("notifications/initialized", Value::Null)
            .await?;
        session.server_info = init.get("serverInfo").cloned();
        session.capabilities = init.get("capabilities").cloned();
        Ok(session)
    }

    /// 拉取工具列表（死连接透明重连一次）。
    pub async fn list_tools(&self) -> Result<Vec<McpTool>, String> {
        match self.list_tools_inner().await {
            Ok(tools) => Ok(tools),
            Err(err) if is_connection_closed_error(&err) => {
                self.invalidate_session().await;
                self.list_tools_inner().await
            }
            Err(err) => Err(err),
        }
    }

    async fn list_tools_inner(&self) -> Result<Vec<McpTool>, String> {
        let arc = self.connect().await?;
        let mut s = arc.lock().await;
        let value = s.request("tools/list", Value::Null).await?;
        parse_tools(&value)
    }

    /// 调用工具（死连接透明重连一次）。
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<McpToolCallResult, String> {
        match self.call_tool_inner(name, arguments.clone()).await {
            Ok(result) => Ok(result),
            Err(err) if is_connection_closed_error(&err) => {
                self.invalidate_session().await;
                self.call_tool_inner(name, arguments).await
            }
            Err(err) => Err(err),
        }
    }

    async fn call_tool_inner(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<McpToolCallResult, String> {
        let arc = self.connect().await?;
        let mut s = arc.lock().await;
        let value = s
            .request(
                "tools/call",
                serde_json::json!({
                    "name": name,
                    "arguments": arguments,
                }),
            )
            .await?;
        Ok(parse_tool_result(value))
    }

    /// 主动断开。
    pub async fn disconnect(&self) {
        let taken = {
            let mut guard = self.session.lock().await;
            guard.take()
        };
        drop(taken);
        self.sink.emit_disconnected(&self.server.id);
    }

    async fn invalidate_session(&self) {
        let taken = {
            let mut guard = self.session.lock().await;
            guard.take()
        };
        drop(taken);
    }

    pub fn server(&self) -> &ChatMcpServer {
        &self.server
    }
}

fn apply_headers(
    builder: RequestBuilder,
    headers: Option<&std::collections::HashMap<String, String>>,
) -> RequestBuilder {
    let mut builder = builder;
    if let Some(headers) = headers {
        for (key, value) in headers {
            if !key.trim().is_empty() {
                builder = builder.header(key.trim(), value);
            }
        }
    }
    builder
}

fn is_connection_closed_error(err: &str) -> bool {
    err.contains("MCP SSE connection closed")
        || err.contains("MCP SSE stream error")
        || err.contains("MCP SSE stream closed")
}

fn parse_tools(value: &Value) -> Result<Vec<McpTool>, String> {
    let tools = value
        .get("tools")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    serde_json::from_value(tools).map_err(|err| format!("MCP tools/list parse failed: {err}"))
}

fn compact_json(value: &Value, max_chars: usize) -> String {
    let raw = serde_json::to_string(value).unwrap_or_default();
    raw.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sse_event_simple_message() {
        let chunk = "event: message\ndata: {\"id\":1}\n\n";
        let (event, data) = parse_sse_event(chunk).unwrap();
        assert_eq!(event, "message");
        assert_eq!(data, r#"{"id":1}"#);
    }

    #[test]
    fn parse_sse_event_endpoint() {
        let chunk = "event: endpoint\ndata: /messages?session=abc\n\n";
        let (event, data) = parse_sse_event(chunk).unwrap();
        assert_eq!(event, "endpoint");
        assert_eq!(data, "/messages?session=abc");
    }

    #[test]
    fn parse_sse_event_multiline_data() {
        let chunk = "event: message\ndata: line1\ndata: line2\n\n";
        let (event, data) = parse_sse_event(chunk).unwrap();
        assert_eq!(event, "message");
        assert_eq!(data, "line1\nline2");
    }

    #[test]
    fn parse_sse_event_incomplete_returns_none() {
        let chunk = "event: message\ndata: {\"id\":1}\n";
        assert!(parse_sse_event(chunk).is_none());
    }
}
