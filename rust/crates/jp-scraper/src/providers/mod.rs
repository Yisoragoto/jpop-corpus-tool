//! Provider 统一接口。
//!
//! 核心 resolver 只认识这个 trait，不认识 iTunes / MusicBrainz / Deezer。
//! 加一个新数据源 = 新增一个文件 + 在 [`default_providers`] 里注册，
//! resolver 一行不用改。
//!
//! **失败要抛分类过的错误，不要吞掉返回空列表**——
//! 「没搜到」和「网断了」对上层是完全不同的两件事。

pub mod deezer;
pub mod apple;
pub mod itunes;
pub mod musicbrainz;

use crate::error::ProviderError;
use crate::models::{ScrapeCandidate, SearchQuery};

/// Artwork is a separate capability: providers without it return no URLs.
pub trait ArtworkProvider: Send + Sync {
    fn artwork_urls(&self, candidate: &ScrapeCandidate, size:u32) -> Vec<String>;
}

impl<T: MetadataProvider + ?Sized> ArtworkProvider for T {
    fn artwork_urls(&self, candidate:&ScrapeCandidate, size:u32)->Vec<String> {
        let url=self.get_artwork(candidate,size);
        if url.is_empty() {Vec::new()} else {vec![url]}
    }
}

/// Existing HTTP uses blocking transports; Tauri runs this on its blocking
/// worker, without holding the corpus lock. Implementations need not provide metadata.
#[cfg(feature="network")]
pub trait LyricsProvider: Send + Sync {
    fn search_lyrics(&self, query:&SearchQuery)->crate::timed_sources::SearchOutput;
}

#[cfg(feature="network")]
pub struct ConfiguredLyricsProvider(pub crate::timed_sources::SourceOptions);

#[cfg(feature="network")]
impl LyricsProvider for ConfiguredLyricsProvider {
    fn search_lyrics(&self, query:&SearchQuery)->crate::timed_sources::SearchOutput {
        crate::timed_sources::search(&query.title,&query.artist,query.duration_sec,&self.0)
    }
}

/// 一条艺人信息。曲目之外的另一条主线：人物页要用。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistInfo {
    pub provider: String,
    pub provider_id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Group / Person
    #[serde(default)]
    pub artist_type: String,
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub formed: String,
    /// 乐队解散 / 个人去世的日期（MusicBrainz 的 life-span.end），可能只有年份
    #[serde(default)]
    pub ended: String,
    /// life-span.ended：有的条目知道已经结束，但不知道是哪天
    #[serde(default)]
    pub has_ended: bool,
    /// 艺人照片。iTunes 不提供，Deezer 才有。
    #[serde(default)]
    pub image_url: String,
    #[serde(default)]
    pub fans: i64,
}

/// 一个元数据源。
///
/// 只有 [`search_tracks`](MetadataProvider::search_tracks) 是必须实现的——
/// 大多数源就是拿来搜曲目的。其余方法有默认实现。
pub trait MetadataProvider: Send + Sync {
    /// 稳定标识，会落库进 `scrape_state.provider`，不要随便改。
    fn name(&self) -> &'static str;

    /// 这个源的可信度先验，参与打分。结构化程度高的给高一点。
    fn confidence(&self) -> f64 {
        1.0
    }

    fn search_tracks(
        &self,
        query: &SearchQuery,
        limit: usize,
    ) -> Result<Vec<ScrapeCandidate>, ProviderError>;

    fn get_track(&self, _provider_id: &str) -> Result<Option<ScrapeCandidate>, ProviderError> {
        Ok(None)
    }

    fn get_artist(
        &self,
        _name: &str,
        _aliases: &[String],
    ) -> Result<Option<ArtistInfo>, ProviderError> {
        Ok(None)
    }

    /// 该候选的高清封面 URL。默认直接用候选自带的。
    fn get_artwork(&self, candidate: &ScrapeCandidate, _size: u32) -> String {
        candidate.artwork_url.clone()
    }
}

// ────────────────────────── 解析辅助 ──────────────────────────

pub(crate) fn as_str(value: Option<&serde_json::Value>) -> String {
    value.and_then(|v| v.as_str()).unwrap_or("").to_string()
}

pub(crate) fn as_f64(value: Option<&serde_json::Value>) -> Option<f64> {
    value.and_then(|v| v.as_f64())
}

pub(crate) fn as_i64(value: Option<&serde_json::Value>) -> Option<i64> {
    value.and_then(|v| match v {
        serde_json::Value::Number(n) => n.as_i64(),
        // MusicBrainz 的音轨号是字符串
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    })
}

/// 取前四位当年份。有的源给的是完整日期。
pub(crate) fn year_of(date: &str) -> String {
    if date.chars().count() >= 4 {
        date.chars().take(4).collect()
    } else {
        String::new()
    }
}

/// 日期截到「年」「年-月」或「年-月-日」：`2019-08-15T07:00:00Z` → `2019-08-15`，
/// `2019-08` 原样，认不出来的给空串。
pub(crate) fn date_of(date: &str) -> String {
    let d = date.trim();
    let bytes = d.as_bytes();
    let digits = |range: std::ops::Range<usize>| bytes.get(range.clone()).is_some_and(|b| b.iter().all(u8::is_ascii_digit));
    if !digits(0..4) {
        return String::new();
    }
    if bytes.get(4) == Some(&b'-') && digits(5..7) {
        if bytes.get(7) == Some(&b'-') && digits(8..10) {
            return d[..10].to_string();
        }
        return d[..7].to_string();
    }
    d[..4].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn as_i64_accepts_musicbrainz_string_numbers() {
        // MusicBrainz 的 track.number 是 "3" 这种字符串
        assert_eq!(as_i64(Some(&json!("3"))), Some(3));
        assert_eq!(as_i64(Some(&json!(" 7 "))), Some(7));
        assert_eq!(as_i64(Some(&json!(3))), Some(3));
        assert_eq!(as_i64(Some(&json!("A1"))), None);
        assert_eq!(as_i64(None), None);
    }

    #[test]
    fn date_of_keeps_as_much_of_the_date_as_there_is() {
        assert_eq!(date_of("2019-08-15T07:00:00Z"), "2019-08-15");
        assert_eq!(date_of("2019-08-15"), "2019-08-15");
        assert_eq!(date_of("2019-08"), "2019-08");
        assert_eq!(date_of("2019"), "2019");
        assert_eq!(date_of("2019-8"), "2019");
        assert_eq!(date_of("19"), "");
        assert_eq!(date_of(""), "");
    }

    #[test]
    fn year_of_takes_four_digits_or_nothing() {
        assert_eq!(year_of("2019-08-15"), "2019");
        assert_eq!(year_of("2019"), "2019");
        assert_eq!(year_of("19"), "");
        assert_eq!(year_of(""), "");
    }
}
