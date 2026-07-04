//! Anthropic Messages API SSE 流式 Provider。
//!
//! 协议：POST {base_url}/v1/messages，body.stream=true，Accept: text/event-stream。
//! 每个 SSE event: `event: <type>\ndata: <json>\n\n`。
//! 我们关心的 event: `content_block_delta` 的 data.delta.text 即为 token。

use crate::config::AnthropicConfig;
use crate::providers::{MessagesRequest, Provider, ProviderError, ProviderResult, TokenStream};
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
    delta: Option<SseDelta>,
    #[serde(default)]
    error: Option<SseError>,
}

#[derive(Debug, Deserialize)]
struct SseDelta {
    #[serde(default)]
    #[serde(rename = "type")]
    delta_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SseError {
    #[serde(default)]
    message: String,
}

#[async_trait::async_trait]
impl Provider for AnthropicClient {
    async fn stream_chat(&self, req: MessagesRequest) -> ProviderResult<TokenStream> {
        // 1. HTTP 请求
        let url = format!("{}/v1/messages", self.cfg.base_url);

        // SenseNova /v1/messages 与 Anthropic Messages API 协议兼容：
        // 鉴权用 Authorization: Bearer（与 OpenAI 兼容接口共用 Key）；
        // anthropic-version 头 SenseNova 也接受但非必需，省略以减少噪音。
        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.cfg.api_key))
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .json(&serde_json::json!({
                "model": req.model,
                "max_tokens": req.max_tokens,
                "messages": req.messages,
                "system": req.system,
                "stream": req.stream,
            }))
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

        // 3. try_stream: Item = Result<String, ProviderError>
        //    yield text 给消费者；想表达错误时用 `?` 直接 bail 即可。
        let stream = try_stream! {
            while let Some(chunk_res) = byte_stream.next().await {
                let chunk = chunk_res.map_err(|e| ProviderError::Http(e.to_string()))?;
                buf.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(idx) = buf.find("\n\n") {
                    let event_text: String = buf.drain(..idx + 2).collect();
                    match parse_event(&event_text) {
                        ParsedEvent::Delta(text) => yield text,
                        ParsedEvent::ApiError(msg) => {
                            Err(ProviderError::Api { status: 0, message: msg })?;
                            return;
                        }
                        ParsedEvent::Skip => {}
                    }
                }
            }
            // 收尾 flush
            if !buf.is_empty() {
                if let ParsedEvent::Delta(text) = parse_event(&buf) {
                    yield text;
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

enum ParsedEvent {
    /// 有增量文本，送给消费者
    Delta(String),
    /// API 报回来的错误，yield Err 终止 stream
    ApiError(String),
    /// ping/message_start/message_stop 等
    Skip,
}

/// 从一个 SSE event 文本块中抽出首个 delta.text。
fn parse_event(event_text: &str) -> ParsedEvent {
    let mut data_payload: Option<String> = None;
    for line in event_text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("data:") {
            let raw = rest.trim();
            if raw == "[DONE]" {
                return ParsedEvent::Skip;
            }
            data_payload = Some(raw.to_string());
        }
    }

    let Some(data) = data_payload else {
        return ParsedEvent::Skip;
    };

    match serde_json::from_str::<SseEvent>(&data) {
        Ok(ev) => {
            if let Some(err) = ev.error {
                return ParsedEvent::ApiError(err.message);
            }
            if ev.event_type == "content_block_delta" {
                if let Some(delta) = ev.delta {
                    // 优先取正文的 text_delta；thinking_delta 在 Rust 层吞掉，
                    // 不 forward 给前端（避免 1M 上下文模型把思考过程灌爆 UI）。
                    // Phase 1 后续要做工具系统时再单独开个 agent:thinking 事件。
                    if let Some(text) = delta.text {
                        if !text.is_empty() {
                            return ParsedEvent::Delta(text);
                        }
                    }
                    // delta_type == "thinking_delta" 或两者皆空 → 丢
                }
            }
            ParsedEvent::Skip
        }
        Err(e) => {
            tracing::debug!("skip non-JSON or unexpected SSE payload: {e}");
            ParsedEvent::Skip
        }
    }
}