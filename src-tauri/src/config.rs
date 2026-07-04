//! Anthropic 配置：Phase 1 全部从环境变量读取，不做 UI / 文件持久化。

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
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .unwrap_or_else(|_| panic!("ANTHROPIC_API_KEY env var is required; copy .env.example to .env and fill it in"));
        let base_url = std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
        let model = std::env::var("ANTHROPIC_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-5".to_string());

        Self { api_key, base_url, model }
    }
}