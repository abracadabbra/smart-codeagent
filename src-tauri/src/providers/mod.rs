//! LLM Provider 抽象 + 实现。
//!
//! Phase 1 只提供 Anthropic 一个 Provider；OpenAI / DeepSeek / Gemini
//! 留到 Phase 2 走 OpenAICompatible 分支。

pub mod anthropic;

use crate::agent::Message;
use crate::config::AnthropicConfig;
use futures::Stream;
use std::pin::Pin;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("HTTP error: {0}")]
    Http(String),

    #[error("SSE parse error: {0}")]
    SseParse(String),

    #[error("API error: {status} — {message}")]
    Api { status: u16, message: String },

    #[error("config error: {0}")]
    Config(String),
}

pub type ProviderResult<T> = Result<T, ProviderError>;

/// Anthropic Messages API 请求体（仅 Phase 1 需要的字段）。
pub struct MessagesRequest {
    pub model: String,
    pub max_tokens: u32,
    pub messages: Vec<Message>,
    pub system: Option<String>,
    pub stream: bool,
}

/// 流式响应：每 item 是一个增量文本片段。
pub type TokenStream = Pin<Box<dyn Stream<Item = ProviderResult<String>> + Send>>;

/// Provider 抽象（Phase 1 仅 Anthropic 实现，未来可扩展）。
///
/// `stream_chat` 是 async：Tauri command 本身就在 tokio runtime 里，
/// async 不会卡死；同步实现只能用 `block_on`，嵌套在 runtime 里会死锁。
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    async fn stream_chat(&self, req: MessagesRequest) -> ProviderResult<TokenStream>;
}

/// 构造默认 provider：Phase 1 = Anthropic。
pub fn default_provider(config: AnthropicConfig) -> Box<dyn Provider> {
    Box::new(anthropic::AnthropicClient::new(config))
}