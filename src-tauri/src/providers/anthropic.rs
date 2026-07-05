//! Anthropic Messages API SSE 流式 Provider。
//!
//! 协议：POST {base_url}/v1/messages，body.stream=true，Accept: text/event-stream。
//! 每个 SSE event: `event: <type>\ndata: <json>\n\n`。
//!
//! Phase 1 只关心 `content_block_delta.delta.text`。
//! Phase 2 扩展：
//! - `content_block_start` 类型支持 `tool_use`（id + name）
//! - `content_block_delta` 类型支持 `input_json_delta`（累积成 JSON 字符串）
//! - `content_block_stop` 触发 tool_use 结束
//! - `message_delta.delta.stop_reason` 透传为 `StreamChunk::Done`
//!
//! SenseNova `/v1/messages` 与 Anthropic Messages API 完全兼容（已实测）。
//! 鉴权用 `Authorization: Bearer`；`anthropic-version` 头 SenseNova 不强制，省略。

use crate::config::AnthropicConfig;
use crate::providers::{MessagesRequest, Provider, ProviderError, ProviderResult, StreamChunk, TokenStream};
use async_stream::try_stream;
use futures::stream::StreamExt;
use reqwest::Client;
use serde::Deserialize;

/// Anthropic 客户端。
pub struct AnthropicClient {
    cfg: AnthropicConfig,
    http: Client,
}

impl AnthropicClient {
    pub fn new(cfg: AnthropicConfig) -> Self {
        let http = Client::builder()
            .build()
            .expect("reqwest client should build");
        Self { cfg, http }
    }

    pub fn config(&self) -> &AnthropicConfig {
        &self.cfg
    }
}

#[derive(Debug, Deserialize)]
struct SseEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    index: Option<u32>,
    #[serde(default)]
    delta: Option<SseDelta>,
    #[serde(default)]
    content_block: Option<SseContentBlock>,
    #[serde(default)]
    error: Option<SseError>,
    #[serde(default)]
    message: Option<SseMessage>,
}

#[derive(Debug, Deserialize)]
struct SseDelta {
    #[serde(default, rename = "type")]
    delta_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SseContentBlock {
    #[serde(default, rename = "type")]
    block_type: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SseError {
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SseMessage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[async_trait::async_trait]
impl Provider for AnthropicClient {
    async fn stream_chat(&self, req: MessagesRequest) -> ProviderResult<TokenStream> {
        // 1. HTTP 请求
        let url = format!("{}/v1/messages", self.cfg.base_url);

        // 构造 body：messages 用 Anthropic multi-block 格式（Phase 2 末段接 tool 累积）
        // Phase 2 中间版本：tools 字段直接传 ChatToolDefinition 数组（已 camelCase）
        let body = serde_json::json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "messages": req.messages,
            "system": req.system,
            "stream": req.stream,
            "tools": req.tools,
        });

        // SenseNova /v1/messages 与 Anthropic Messages API 协议兼容：
        // 鉴权用 Authorization: Bearer（与 OpenAI 兼容接口共用 Key）；
        // anthropic-version 头 SenseNova 也接受但非必需，省略以减少噪音。
        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.cfg.api_key))
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        // 2. 字节流
        let mut byte_stream = response.bytes_stream();
        let mut buf = String::new();

        // 3. try_stream: Item = Result<StreamChunk, ProviderError>
        //    yield 给消费者；想表达错误时用 `?` 直接 bail 即可。
        let stream = try_stream! {
            while let Some(chunk_res) = byte_stream.next().await {
                let chunk = chunk_res.map_err(|e| ProviderError::Http(e.to_string()))?;
                buf.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(idx) = buf.find("\n\n") {
                    let event_text: String = buf.drain(..idx + 2).collect();
                    for c in parse_event(&event_text) {
                        yield c;
                    }
                }
            }
            // 收尾 flush
            if !buf.is_empty() {
                for c in parse_event(&buf) {
                    yield c;
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

/// 从一个 SSE event 文本块中产出 0..N 个 StreamChunk。
fn parse_event(event_text: &str) -> Vec<StreamChunk> {
    let mut data_payload: Option<String> = None;
    for line in event_text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("data:") {
            let raw = rest.trim();
            if raw == "[DONE]" {
                return vec![StreamChunk::Done { stop_reason: None }];
            }
            data_payload = Some(raw.to_string());
        }
    }

    let Some(data) = data_payload else {
        return vec![];
    };

    let ev: SseEvent = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("skip non-JSON or unexpected SSE payload: {e}");
            return vec![];
        }
    };

    if let Some(err) = ev.error {
        tracing::warn!("API error in SSE: {}", err.message);
        return vec![StreamChunk::Done {
            stop_reason: Some(format!("api_error: {}", err.message)),
        }];
    }

    let mut out = Vec::new();

    match ev.event_type.as_str() {
        "content_block_start" => {
            if let Some(block) = ev.content_block {
                if block.block_type.as_deref() == Some("tool_use") {
                    if let (Some(id), Some(name)) = (block.id, block.name) {
                        out.push(StreamChunk::ToolUseStart { id, name });
                    }
                }
            }
        }
        "content_block_delta" => {
            if let Some(delta) = ev.delta {
                // 优先取正文的 text_delta；thinking_delta 在 Rust 层吞掉
                if let Some(text) = delta.text {
                    if !text.is_empty() {
                        out.push(StreamChunk::Text(text));
                    }
                }
                // tool_use 参数增量
                if let Some(json_delta) = delta.partial_json {
                    if !json_delta.is_empty() {
                        out.push(StreamChunk::ToolUseInputDelta(json_delta));
                    }
                }
                // delta.thinking 字段直接丢
            }
        }
        "content_block_stop" => {
            // 注意：tool_use_stop 与 text_stop 都用同一种事件；只有当本轮最后
            // 累积过 tool_use 时才产生 End。Loop 侧根据上下文判定（更准确的做法是
            // 在 content_block_start 时记下 type，stop 时按 type 分发）。Phase 2
            // 简化：loop 维护 `current_block_is_tool_use` 标志位。
            //
            // 这里仅作为信号，loop 自行决定。
            out.push(StreamChunk::ToolUseEnd);
        }
        "message_delta" => {
            if let Some(delta) = ev.delta {
                if let Some(reason) = delta.stop_reason {
                    out.push(StreamChunk::Done {
                        stop_reason: Some(reason),
                    });
                }
            }
        }
        _ => {
            // ping / message_start / message_stop 等忽略
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_delta() {
        let event = "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n";
        let chunks = parse_event(event);
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            StreamChunk::Text(t) => assert_eq!(t, "hello"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_use_start() {
        let event = "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_01\",\"name\":\"read_file\"}}\n\n";
        let chunks = parse_event(event);
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            StreamChunk::ToolUseStart { id, name } => {
                assert_eq!(id, "toolu_01");
                assert_eq!(name, "read_file");
            }
            other => panic!("expected ToolUseStart, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_use_input_delta() {
        let event = "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\"}}\n\n";
        let chunks = parse_event(event);
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            StreamChunk::ToolUseInputDelta(s) => assert_eq!(s, "{\"path\":"),
            other => panic!("expected ToolUseInputDelta, got {other:?}"),
        }
    }

    #[test]
    fn parses_message_delta_with_stop_reason() {
        let event = "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n";
        let chunks = parse_event(event);
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            StreamChunk::Done { stop_reason } => {
                assert_eq!(stop_reason.as_deref(), Some("end_turn"));
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn swallows_thinking_delta() {
        let event = "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"deep thought\"}}\n\n";
        let chunks = parse_event(event);
        assert!(chunks.is_empty(), "thinking_delta should be dropped");
    }

    #[test]
    fn parses_done_marker() {
        let chunks = parse_event("data: [DONE]\n\n");
        assert_eq!(chunks.len(), 1);
        assert!(matches!(chunks[0], StreamChunk::Done { .. }));
    }
}