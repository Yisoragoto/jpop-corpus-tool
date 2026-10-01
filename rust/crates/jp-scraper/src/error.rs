//! Provider 错误。**必须是分类过的。**
//!
//! 上层要能区分「限流了，等会儿重试」和「这首歌本来就查不到，
//! 重试一万次也一样」。原来那套 `except Exception: return None`
//! 做不到这件事——一个 None 里同时装着超时、404、限流和 DNS 挂了。

use std::fmt;

use crate::status::ErrorType;

/// 一次 provider 调用的失败。
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderError {
    pub error_type: ErrorType,
    pub message: String,
    pub provider: String,
    /// provider 明示的等待秒数（HTTP `Retry-After`），没有就是 None
    pub retry_after: Option<f64>,
}

impl ProviderError {
    pub fn new(error_type: ErrorType, message: impl Into<String>) -> Self {
        Self {
            error_type,
            message: message.into(),
            provider: String::new(),
            retry_after: None,
        }
    }

    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = provider.into();
        self
    }

    pub fn with_retry_after(mut self, retry_after: Option<f64>) -> Self {
        self.retry_after = retry_after;
        self
    }

    pub fn retryable(&self) -> bool {
        self.error_type.is_retryable()
    }

    // ── 便捷构造 ──

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(ErrorType::Timeout, message)
    }
    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::new(ErrorType::RateLimit, message)
    }
    pub fn network(message: impl Into<String>) -> Self {
        Self::new(ErrorType::Network, message)
    }
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(ErrorType::Unavailable, message)
    }
    pub fn invalid_response(message: impl Into<String>) -> Self {
        Self::new(ErrorType::InvalidResponse, message)
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorType::NotFound, message)
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.provider.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "[{}] {}", self.provider, self.message)
        }
    }
}

impl std::error::Error for ProviderError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryability_follows_the_error_type() {
        assert!(ProviderError::rate_limited("429").retryable());
        assert!(ProviderError::timeout("").retryable());
        // 「这首歌不存在」重试多少次都一样
        assert!(!ProviderError::new(ErrorType::NoResults, "").retryable());
        assert!(!ProviderError::not_found("404").retryable());
    }

    #[test]
    fn display_names_the_provider() {
        let e = ProviderError::rate_limited("被限流").with_provider("itunes");
        assert_eq!(e.to_string(), "[itunes] 被限流");
    }
}
