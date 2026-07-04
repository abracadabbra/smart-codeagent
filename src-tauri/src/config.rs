//! LLM 配置：Anthropic Messages API 兼容协议，默认指向 SenseNova。
//!
//! Phase 1 全部从环境变量读取，不做 UI / 文件持久化。
//! SenseNova `/v1/messages` 与 Anthropic 协议完全兼容（共用 Bearer 鉴权，
//! 同样的 body 字段与 SSE 事件序列），因此 AnthropicClient 可以原样复用，
//! 只是 base_url 指向 SenseNova。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("env var `{0}` is not set")]
    MissingEnv(&'static str),

    #[error("env var `{0}` is malformed: {1}")]
    MalformedEnv(&'static str, String),
}

#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

impl AnthropicConfig {
    /// 从环境变量加载，缺失或格式错误立即 panic（PRD §AC9 决策：不让静默失败）。
    pub fn from_env() -> Self {
        let api_key = std::env::var("LLM_API_KEY")
            .unwrap_or_else(|_| panic!("LLM_API_KEY env var is required; copy .env.example to .env and fill it in"));
        let base_url = std::env::var("LLM_BASE_URL")
            .unwrap_or_else(|_| "https://token.sensenova.cn".to_string());
        let model = std::env::var("LLM_MODEL")
            .unwrap_or_else(|_| "deepseek-v4-flash".to_string());

        Self { api_key, base_url, model }
    }
}