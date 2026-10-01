//! 刮削状态与失败原因。
//!
//! 单独成文件是因为 `store`（落库）、`resolver`（判定）、UI（展示）三边都要
//! 引用它，放在任何一边都会让另外两边产生反向依赖。
//!
//! 两个枚举都以字符串形态落库，取值和 Python 侧逐字相同——
//! 迁移期两边读写同一张 `scrape_state` 表。

use serde::{Deserialize, Serialize};

/// 一个本地文件在刮削流水线里的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrapeStatus {
    /// 已入库待刮
    #[default]
    Pending,
    /// 正在刮。进程崩溃后残留此状态即为中断。
    Running,
    /// 置信度够高，已自动采纳
    Success,
    /// 有候选但不够确定，等用户确认
    LowConfidence,
    /// 无候选 / 网络失败 / provider 不可用
    Failed,
    /// 用户显式跳过，重跑时不再自动处理
    Skipped,
}

impl ScrapeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Success => "success",
            Self::LowConfidence => "low_confidence",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    /// 落库的字符串反解。认不出的值一律当 `Pending`——
    /// 库里出现意外取值时宁可重刮一遍，也不要 panic 掉整个查询。
    pub fn from_str_lossy(text: &str) -> Self {
        match text {
            "running" => Self::Running,
            "success" => Self::Success,
            "low_confidence" => Self::LowConfidence,
            "failed" => Self::Failed,
            "skipped" => Self::Skipped,
            _ => Self::Pending,
        }
    }

    /// 是否是「这一轮不用再动它」的状态。
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Success | Self::Skipped)
    }

    pub fn needs_review(self) -> bool {
        matches!(self, Self::LowConfidence)
    }

    /// Retry Failed 按钮该捞起哪些行。
    ///
    /// `LowConfidence` 不算可重试：它缺的是用户决策，不是再跑一次网络请求。
    pub fn is_retryable(self) -> bool {
        matches!(self, Self::Failed | Self::Running | Self::Pending)
    }

    pub const ALL: &'static [ScrapeStatus] = &[
        Self::Pending,
        Self::Running,
        Self::Success,
        Self::LowConfidence,
        Self::Failed,
        Self::Skipped,
    ];
}

/// 失败原因分类。
///
/// 比起把异常信息直接存下来，分类字段能让 UI 做「按原因分组重试」：
/// `RateLimit` 应该等一会儿再跑，`NoResults` 再跑一百次也是同样结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    #[default]
    None,
    /// 请求超时
    Timeout,
    /// 被限流（429 / provider 明示）
    RateLimit,
    /// DNS、连接被拒、断网
    Network,
    /// provider 5xx 或维护中
    Unavailable,
    /// 返回了但不是预期结构 / JSON 解析失败
    InvalidResponse,
    /// provider 明确说没有这条记录（404）
    NotFound,
    /// 请求成功但搜索结果为空
    NoResults,
    /// 有结果但没有一个过得了阈值
    NoMatch,
    /// 元数据对上了，附带资源（封面）下载失败
    DownloadFailed,
    /// 兜底，不应该经常出现
    Unknown,
}

impl ErrorType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Timeout => "timeout",
            Self::RateLimit => "rate_limit",
            Self::Network => "network",
            Self::Unavailable => "unavailable",
            Self::InvalidResponse => "invalid_response",
            Self::NotFound => "not_found",
            Self::NoResults => "no_results",
            Self::NoMatch => "no_match",
            Self::DownloadFailed => "download_failed",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_str_lossy(text: &str) -> Self {
        match text {
            "timeout" => Self::Timeout,
            "rate_limit" => Self::RateLimit,
            "network" => Self::Network,
            "unavailable" => Self::Unavailable,
            "invalid_response" => Self::InvalidResponse,
            "not_found" => Self::NotFound,
            "no_results" => Self::NoResults,
            "no_match" => Self::NoMatch,
            "download_failed" => Self::DownloadFailed,
            "unknown" => Self::Unknown,
            _ => Self::None,
        }
    }

    /// 值得自动重试的失败原因。
    ///
    /// `NoResults` / `NoMatch` 不在其中——同样的查询词重跑不会有新结果，
    /// 要变的是查询词或 provider。
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Timeout
                | Self::RateLimit
                | Self::Network
                | Self::Unavailable
                | Self::DownloadFailed
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_strings_match_python() {
        // 这些字符串直接落库，改一个字就和 Python 写的行对不上
        let pairs = [
            (ScrapeStatus::Pending, "pending"),
            (ScrapeStatus::Running, "running"),
            (ScrapeStatus::Success, "success"),
            (ScrapeStatus::LowConfidence, "low_confidence"),
            (ScrapeStatus::Failed, "failed"),
            (ScrapeStatus::Skipped, "skipped"),
        ];
        for (status, text) in pairs {
            assert_eq!(status.as_str(), text);
            assert_eq!(ScrapeStatus::from_str_lossy(text), status);
        }
    }

    #[test]
    fn error_strings_match_python() {
        for e in [
            ErrorType::None,
            ErrorType::Timeout,
            ErrorType::RateLimit,
            ErrorType::Network,
            ErrorType::Unavailable,
            ErrorType::InvalidResponse,
            ErrorType::NotFound,
            ErrorType::NoResults,
            ErrorType::NoMatch,
            ErrorType::DownloadFailed,
            ErrorType::Unknown,
        ] {
            assert_eq!(ErrorType::from_str_lossy(e.as_str()), e, "{e:?}");
        }
        assert_eq!(ErrorType::None.as_str(), "");
    }

    #[test]
    fn low_confidence_is_not_retryable() {
        // 它缺的是用户决策，重跑网络请求没有意义
        assert!(!ScrapeStatus::LowConfidence.is_retryable());
        assert!(ScrapeStatus::Failed.is_retryable());
        // 崩溃残留的 running 要能被捞回来
        assert!(ScrapeStatus::Running.is_retryable());
    }

    #[test]
    fn no_results_is_not_retryable() {
        assert!(!ErrorType::NoResults.is_retryable());
        assert!(!ErrorType::NoMatch.is_retryable());
        assert!(ErrorType::RateLimit.is_retryable());
    }

    #[test]
    fn unknown_status_text_falls_back_to_pending() {
        // 库里出现意外取值时重刮一遍，而不是 panic
        assert_eq!(ScrapeStatus::from_str_lossy("ok"), ScrapeStatus::Pending);
        assert_eq!(ScrapeStatus::from_str_lossy(""), ScrapeStatus::Pending);
    }
}
