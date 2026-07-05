//! LLM Provider 抽象 + 实现。
//!
//! 当前实现走 OpenAI Chat Completions 兼容协议（`AnthropicClient`），默认指向
//! SenseNova (`https://token.sensenova.cn`) + `deepseek-v4-flash`。
//!
//! 为什么不用 Anthropic `/v1/messages`：
//! - SenseNova 的 `/v1/messages` 只对 Claude 系列模型翻译 tool_use content block；
//! - 对 DeepSeek / Qwen 等模型，tool_use 被退化成纯文本输出；
//! - `/v1/chat/completions` 是 OpenAI 标准，所有模型原生支持 tool calling。
//!
//! 文件名保留 `anthropic.rs` 是历史原因（Phase 1 用 Anthropic 协议）。

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

/// OpenAI chat completions 请求体。
///
/// `system` 字段由 provider 转成 messages 数组里的第一条 system role 消息
/// （OpenAI 不用顶层 `system` 字段）。
#[derive(Debug, Clone, Serialize)]
pub struct MessagesRequest {
    pub model: String,
    pub max_tokens: u32,
    pub messages: Vec<Message>,
    pub system: Option<String>,
    pub stream: bool,
    /// 可用工具定义（内部格式，provider 转成 OpenAI function calling 格式）
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