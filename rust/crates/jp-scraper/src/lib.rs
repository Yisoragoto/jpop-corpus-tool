//! 元数据解析流水线：查询构造 → provider → 候选打分 → 判定 → 落库。
//!
//! 移植自 Python 侧 `scraper/` 包，分层一一对应：
//!
//! | 模块 | 职责 | 能离线测试 |
//! |---|---|---|
//! | `status` | 状态与失败原因枚举 | ✅ |
//! | `models` | 各层之间传递的数据结构 | ✅ |
//! | `query` | 由准到宽的查询阶梯 | ✅ |
//! | `error` | 分类过的 provider 错误 | ✅ |
//! | `http` | 超时 / 重试 / 退避 / 限流 | ✅（注入传输层和时钟） |
//! | `providers` | iTunes / MusicBrainz / Deezer | ✅（注入响应） |
//! | `resolver` | 串起来：查询 → 候选 → 打分 → 判定 | ✅（假 provider） |
//! | `store` | 状态 / 历史 / 原始元数据落库 | ✅（内存库） |
//! | `artwork` | 封面下载 / 校验 / 落盘 | ✅（注入传输层） |
//! | `net` | 真实 HTTP 传输层（ureq）| ✗（要联网） |
//! | `matching` | 可解释的候选打分 | ✅ |
//! | `similarity` | 字符串相似度（difflib / Jaro-Winkler） | ✅ |
//!
//! 归一化在独立的 `jp-normalize` crate 里，和 `jp-import` 共用一份——
//! 这个键决定「是不是同一首歌」，两处各写一套迟早会分叉。

pub mod lyrics;
pub mod artwork;
pub mod error;
pub mod http;
pub mod matching;
pub mod models;
#[cfg(feature = "network")]
pub mod net;
pub mod providers;
pub mod query;
pub mod resolver;
pub mod similarity;
pub mod status;
pub mod store;

pub use artwork::{
    ImageKind, SaveOutcome, artist_image_path_for, artwork_urls, cover_path_for, image_dimensions,
    looks_square, safe_name, save_cover_any, save_image, save_image_any,
};
pub use error::ProviderError;
pub use http::{HttpClient, HttpConfig, HttpResponse, RateLimiter, Transport};
pub use matching::{MatchScorer, ScorerConfig};
pub use models::{
    MatchBreakdown, NormalizedTrack, Penalty, ResolvedTrack, ScrapeCandidate, SearchQuery,
    TrackFile,
};
#[cfg(feature = "network")]
pub use net::{UreqTransport, artist_providers, default_providers, default_resolver};
pub use providers::{ArtistInfo, MetadataProvider};
pub use query::{QueryBuilder, QueryBuilderConfig};
pub use resolver::{MetadataResolver, ResolverConfig};
pub use similarity::Similarity;
pub use status::{ErrorType, ScrapeStatus};
pub use store::{ScrapeAttempt, ScrapeRecord, ScrapeStore, ScrapeSummary};
