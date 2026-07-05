//! LLM Provider 抽象 + 实现。
//!
//! 当前实现走 Anthropic Messages API 兼容协议（`AnthropicClient`），默认指向
//! SenseNova (`https://token.sensenova.cn`) + `deepseek-v4-flash`。
//! 协议层完全兼容（Body + SSE 事件序列），仅鉴权头从 `x-api-key` 换成
//! `Authorization: Bearer`（见 `anthropic.rs`）。
//!
//! 真正的 OpenAI 兼容 / Gemini / 自定义 Provider 留到 Phase 2 走
//! `OpenAICompatible` 分支。

pub mod anthropic;

use crate::agent::Message;
use crate::agent::tools::ChatToolDefinition;
use crate::config::AnthropicConfig;
use futures::Stream;
use serde::Serialize;
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

/// Anthropic Messages API 请求体（Phase 2 加 `tools` 字段）。
#[derive(Debug, Clone, Serialize)]
pub struct MessagesRequest {
    pub model: String,
    pub max_tokens: u32,
    pub messages: Vec<Message>,
    pub system: Option<String>,
    pub stream: bool,
    /// Phase 2 新增：可用工具定义（Anthropic `tools` 字段）
    #[serde(default)]
    pub tools: Vec<ChatToolDefinition>,
}

/// 流式响应：每 item 是一个 `StreamChunk`（Phase 2 起改为区分 text / tool_use）。
///
/// Phase 1 的 `TokenStream = Pin<Box<Stream<Item = Result<String>>>>` 被
/// `StreamChunk::Text` 取代；tool_use 通过 `StreamChunk::ToolUseStart` /
/// `StreamChunk::ToolUseInputDelta` / `StreamChunk::ToolUseEnd` 三段式累积。
pub type TokenStream = Pin<Box<dyn Stream<Item = ProviderResult<StreamChunk>> + Send>>;

#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// 普通文本增量
    Text(String),
    /// tool_use 开始（携带 id + name）
    ToolUseStart { id: String, name: String },
    /// tool_use 参数增量（input_json_delta 累积）
    ToolUseInputDelta(String),
    /// tool_use 结束
    ToolUseEnd,
    /// 流结束信号（含 stop_reason）
    Done { stop_reason: Option<String> },
}

/// Provider 抽象（Phase 1 仅 Anthropic 实现，未来可扩展）。
///
/// `stream_chat` 是 async：Tauri command 本身就在 tokio runtime 里，
/// async 不会卡死；同步实现只能用 `block_on`，嵌套在 runtime 里会死锁。
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    async fn stream_chat(&self, req: MessagesRequest) -> ProviderResult<TokenStream>;
}

/// 构造默认 provider：Phase 2 = Anthropic（SenseNova 兼容）。
pub fn default_provider(config: AnthropicConfig) -> Box<dyn Provider> {
    Box::new(anthropic::AnthropicClient::new(config))
}