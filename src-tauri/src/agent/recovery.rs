//! LLM 请求错误恢复策略。
//!
//! PRD §8 错误恢复策略实现：
//! - RateLimited / NetworkError → RetryBackoff（指数退避 + jitter，最多 5 次）
//! - ContextOverflow / ParseError → TrimAndRetry（缩减上下文窗口后重试）
//! - AuthFailed → NotifyUser（直接向上返回错误，由 runner emit 给前端）
//! - ToolFailed / ToolNotFound → ReportAndContinue（在 rounds.rs 已实现）

use crate::agent::Message;
use crate::agent::context::trim_messages;
use crate::providers::{ProviderError, ProviderResult};
use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

/// 可恢复错误类型。
#[derive(Debug, Clone, PartialEq)]
pub enum RecoverableError {
    /// 速率限制（429）。
    RateLimited,
    /// 上下文溢出（input length / context too long）。
    ContextOverflow,
    /// 网络错误（连接失败 / 5xx）。
    NetworkError,
    /// 流解析错误（SSE 非法 JSON）。
    ParseError,
    /// 鉴权失败（401 / 403）。
    AuthFailed,
    /// 不可恢复的其他错误。
    Fatal(String),
}

impl RecoverableError {
    /// 从 ProviderError 分类。
    pub fn from_provider_error(err: &ProviderError) -> Self {
        match err {
            ProviderError::Http(msg) => classify_http_message(msg),
            ProviderError::SseParse(_) => {
                // SSE 解析失败通常是不可恢复的协议异常，先按 ParseError 重试一次。
                RecoverableError::ParseError
            }
            ProviderError::Api { status, message } => classify_api_error(*status, message),
            ProviderError::Config(msg) => RecoverableError::Fatal(format!("config error: {msg}")),
        }
    }

    /// 是否需要退避重试。
    pub fn should_backoff(&self) -> bool {
        matches!(
            self,
            RecoverableError::RateLimited | RecoverableError::NetworkError
        )
    }

    /// 是否需要裁剪上下文后重试。
    pub fn should_trim(&self) -> bool {
        matches!(
            self,
            RecoverableError::ContextOverflow | RecoverableError::ParseError
        )
    }

    /// 是否直接通知用户。
    pub fn should_notify_user(&self) -> bool {
        matches!(self, RecoverableError::AuthFailed)
    }
}

fn classify_http_message(msg: &str) -> RecoverableError {
    let lower = msg.to_lowercase();
    if lower.contains("429") || lower.contains("rate limit") || lower.contains("too many requests")
    {
        return RecoverableError::RateLimited;
    }
    if lower.contains("401") || lower.contains("403") {
        return RecoverableError::AuthFailed;
    }
    if lower.contains("timeout")
        || lower.contains("connection refused")
        || lower.contains("dns error")
        || lower.contains("connect error")
        || lower.contains("5")
    {
        return RecoverableError::NetworkError;
    }
    RecoverableError::Fatal(msg.to_string())
}

fn classify_api_error(status: u16, message: &str) -> RecoverableError {
    match status {
        401 | 403 => RecoverableError::AuthFailed,
        429 => RecoverableError::RateLimited,
        500..=599 => RecoverableError::NetworkError,
        400 | 422 => {
            let lower = message.to_lowercase();
            if lower.contains("input length")
                || lower.contains("context")
                || lower.contains("too long")
                || lower.contains("max tokens")
                || lower.contains("token limit")
                || lower.contains("range of input length")
            {
                RecoverableError::ContextOverflow
            } else {
                RecoverableError::Fatal(format!("{status}: {message}"))
            }
        }
        _ => RecoverableError::Fatal(format!("{status}: {message}")),
    }
}

/// 重试策略配置。
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 5,
            base_delay_ms: 1000,
            max_delay_ms: 20000,
        }
    }
}

/// 计算第 `attempt` 次重试的退避延迟（指数退避 + jitter）。
///
/// attempt 从 0 开始：第 0 次延迟 base，第 1 次 2*base，... 封顶 max。
pub fn backoff_delay(policy: &RetryPolicy, attempt: u32) -> Duration {
    let exp = policy
        .base_delay_ms
        .saturating_mul(2_u64.saturating_pow(attempt));
    let capped = exp.min(policy.max_delay_ms);
    let jitter = jitter_millis();
    Duration::from_millis(capped.saturating_add(jitter))
}

/// 简单 jitter：用当前时间纳秒数取模 500ms。
fn jitter_millis() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (now.subsec_nanos() % 500) as u64
}

/// 带退避重试地执行异步操作。
///
/// 仅对 `should_backoff()` 为 true 的错误重试；其他错误立即返回。
/// 每次重试前 sleep 退避时间，并检查是否仍应继续（cancel 检查）。
pub async fn retry_with_backoff<F, Fut, T>(
    policy: &RetryPolicy,
    operation: F,
    is_cancelled: impl Fn() -> bool,
) -> ProviderResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ProviderResult<T>>,
{
    retry_with_backoff_hooked(policy, operation, is_cancelled, |_, _, _| {}).await
}

/// 带退避重试地执行异步操作，每次重试触发回调。
///
/// `on_retry` 参数：`(recoverable_error, attempt, next_delay)`。
/// attempt 从 1 开始（即第一次失败后的重试）。
pub async fn retry_with_backoff_hooked<F, Fut, T>(
    policy: &RetryPolicy,
    mut operation: F,
    is_cancelled: impl Fn() -> bool,
    mut on_retry: impl FnMut(&RecoverableError, u32, Duration),
) -> ProviderResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ProviderResult<T>>,
{
    let mut last_err = None;

    for attempt in 0..=policy.max_retries {
        match operation().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                let recoverable = RecoverableError::from_provider_error(&e);
                if recoverable.should_notify_user() || !recoverable.should_backoff() {
                    return Err(e);
                }

                if attempt == policy.max_retries {
                    last_err = Some(e);
                    break;
                }

                if is_cancelled() {
                    return Err(ProviderError::Http(
                        "run cancelled during retry".to_string(),
                    ));
                }

                let delay = backoff_delay(policy, attempt);
                on_retry(&recoverable, attempt + 1, delay);
                tracing::warn!(
                    "LLM request failed (attempt {}/{}): {:?}, retrying in {:?}",
                    attempt + 1,
                    policy.max_retries,
                    recoverable,
                    delay
                );
                sleep(delay).await;

                if is_cancelled() {
                    return Err(ProviderError::Http(
                        "run cancelled during retry".to_string(),
                    ));
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| ProviderError::Http("max retries exceeded".to_string())))
}

/// 上下文裁剪重试上下文。
///
/// 维护一个逐步缩小的上下文窗口，遇到 ContextOverflow / ParseError 时丢弃最旧 1/3。
#[derive(Debug, Clone)]
pub struct TrimAndRetryContext {
    pub original_window_tokens: u32,
    pub current_window_tokens: u32,
    pub trim_count: u32,
    pub max_trims: u32,
}

impl TrimAndRetryContext {
    pub fn new(original_window_tokens: u32) -> Self {
        Self {
            original_window_tokens,
            current_window_tokens: original_window_tokens,
            trim_count: 0,
            max_trims: 3,
        }
    }

    /// 尝试裁剪一次。返回 true 表示还可以继续裁剪。
    pub fn trim(&mut self) -> bool {
        if self.trim_count >= self.max_trims {
            return false;
        }
        // 每次丢弃当前窗口的 1/3（向下取整），最少保留 1/3 原始窗口。
        let min_window = self.original_window_tokens / 3;
        let next = self.current_window_tokens - self.current_window_tokens / 3;
        self.current_window_tokens = next.max(min_window);
        self.trim_count += 1;
        tracing::warn!(
            "context overflow: trim {}/{}, window reduced to {}",
            self.trim_count,
            self.max_trims,
            self.current_window_tokens
        );
        true
    }

    /// 根据当前窗口裁剪消息。
    pub fn trim_messages(&self, messages: &[Message]) -> Vec<Message> {
        trim_messages(messages, self.current_window_tokens)
    }
}

/// 将 `ProviderError` 转为用户可见的中文提示。
pub fn error_message_for_user(err: &ProviderError) -> String {
    match err {
        ProviderError::Http(msg) => format!("网络错误: {msg}"),
        ProviderError::SseParse(msg) => format!("流解析错误: {msg}"),
        ProviderError::Api { status, message } => match status {
            401 | 403 => "API Key 无效或已过期，请检查设置面板中的 Provider 配置。".to_string(),
            429 => "请求过于频繁，请稍后再试。".to_string(),
            500..=599 => "LLM 服务商暂时不可用，请稍后再试。".to_string(),
            _ => format!("API 错误 ({status}): {message}"),
        },
        ProviderError::Config(msg) => format!("配置错误: {msg}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ProviderError;

    #[test]
    fn classify_rate_limited() {
        let err = ProviderError::Api {
            status: 429,
            message: "rate limit exceeded".into(),
        };
        assert_eq!(
            RecoverableError::from_provider_error(&err),
            RecoverableError::RateLimited
        );
    }

    #[test]
    fn classify_auth_failed() {
        let err = ProviderError::Api {
            status: 401,
            message: "invalid api key".into(),
        };
        assert_eq!(
            RecoverableError::from_provider_error(&err),
            RecoverableError::AuthFailed
        );
    }

    #[test]
    fn classify_context_overflow() {
        let err = ProviderError::Api {
            status: 400,
            message: "Range of input length should be [1, 1000000]".into(),
        };
        assert_eq!(
            RecoverableError::from_provider_error(&err),
            RecoverableError::ContextOverflow
        );
    }

    #[test]
    fn classify_network_error_timeout() {
        let err = ProviderError::Http("connection refused".into());
        assert_eq!(
            RecoverableError::from_provider_error(&err),
            RecoverableError::NetworkError
        );
    }

    #[test]
    fn trim_context_reduces_window() {
        let mut ctx = TrimAndRetryContext::new(90_000);
        assert!(ctx.trim());
        assert_eq!(ctx.current_window_tokens, 60_000);
        assert!(ctx.trim());
        assert_eq!(ctx.current_window_tokens, 40_000);
        assert!(ctx.trim());
        assert_eq!(ctx.current_window_tokens, 30_000); // 最小保留 1/3 原始窗口
        assert!(!ctx.trim());
    }

    #[test]
    fn backoff_delay_grows_and_caps() {
        let policy = RetryPolicy {
            max_retries: 5,
            base_delay_ms: 1000,
            max_delay_ms: 5000,
        };
        assert!(backoff_delay(&policy, 0).as_millis() >= 1000);
        assert!(backoff_delay(&policy, 1).as_millis() >= 2000);
        assert!(backoff_delay(&policy, 10).as_millis() <= 5500); // capped + jitter
    }
}
