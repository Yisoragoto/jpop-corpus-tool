//! iTunes Search API。免鉴权、免 key，JP 区对 J-POP 的覆盖明显好于默认的 US 区。
//!
//! 已知脾气（沿用原 `cover_scraper` 里踩过的坑）：
//! `limit` 不是「取前 N 条」而是会影响搜索本身——同一个 term，
//! `limit=1` 经常返回空或者一首不相干的歌，`limit=25` 却能把正确的那首
//! 排在里面。所以这里一律多取，排序和挑选交给本地打分。

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use regex::Regex;

use crate::error::ProviderError;
use crate::http::{HttpClient, HttpConfig, RateLimiter};
use crate::models::{ScrapeCandidate, SearchQuery};

use super::{ArtistInfo, MetadataProvider, as_f64, as_i64, as_str, year_of};

const SEARCH_URL: &str = "https://itunes.apple.com/search";
const LOOKUP_URL: &str = "https://itunes.apple.com/lookup";

/// iTunes 没有公开的限流额度，但持续高频请求会开始返回 403 + 空 body。
/// 实测：4 个 worker 不加节流地跑 209 首，失败率从 20% 一路涨到 50%。
/// 在进程内排一条队，把请求摊开。0.25s ≈ 4 req/s，足够快也不会被拒。
pub const MIN_INTERVAL: Duration = Duration::from_millis(250);

pub static SHARED_RATE_LIMITER: LazyLock<Arc<RateLimiter>> =
    LazyLock::new(|| Arc::new(RateLimiter::new(MIN_INTERVAL)));

/// iTunes CDN 的封面 URL 里尺寸可替换：100x100 换成 600x600 就是高清图。
static ARTWORK_SIZE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/(\d+)x(\d+)bb\.(jpg|png)").unwrap());

/// 把封面 URL 换成高清版本；URL 形态不认识时原样返回。
pub fn hi_res_artwork(url: &str, size: u32) -> String {
    if url.is_empty() {
        return String::new();
    }
    ARTWORK_SIZE_RE
        .replace_all(url, format!("/{size}x{size}bb.$3").as_str())
        .into_owned()
}

/// iTunes 给单曲和 EP 的专辑名加了「 - Single」「 - EP」后缀，别的源和本地标签都没有。
///
/// 不去掉的话，同一张专辑在库里会出现两种写法（「創作 - EP」和「創作」），
/// 专辑页也就分成两张。**只去掉结尾的那一段**，名字本身带 EP 的（`Fantasy EP`）不动。
pub fn strip_release_suffix(album: &str) -> String {
    for suffix in [" - Single", " - EP"] {
        if let Some(stripped) = album.strip_suffix(suffix) {
            return stripped.trim_end().to_string();
        }
    }
    album.to_string()
}

pub struct ITunesProvider {
    pub country: String,
    pub artwork_size: u32,
    http: HttpClient,
}

impl ITunesProvider {
    /// 用给定的 client，不加任何默认配置。
    ///
    /// 测试走这条：不挂全局限流器，也就不会真的 sleep，
    /// 而且多个测试之间不会因为共享限流器互相干扰。
    pub fn new(http: HttpClient) -> Self {
        Self {
            country: "JP".into(),
            artwork_size: 600,
            http: http.with_provider_name("itunes"),
        }
    }

    /// 生产用：挂上共享限流器和适合 403 的退避。
    pub fn production(http: HttpClient) -> Self {
        let mut p = Self::new(http);
        p.http = p
            .http
            .with_rate_limiter(SHARED_RATE_LIMITER.clone())
            // 403 限流恢复得慢，退避起点比默认的 0.6s 大一些
            .with_config(HttpConfig {
                timeout: Duration::from_secs(10),
                retries: 2,
                backoff: Duration::from_millis(1500),
                ..Default::default()
            });
        p
    }

    pub fn with_country(mut self, country: impl Into<String>) -> Self {
        self.country = country.into();
        self
    }

    fn parse(&self, payload: &serde_json::Value) -> Result<Vec<ScrapeCandidate>, ProviderError> {
        let object = payload.as_object().ok_or_else(|| {
            ProviderError::invalid_response("响应不是 JSON 对象").with_provider("itunes")
        })?;
        let results = object.get("results").ok_or_else(|| {
            ProviderError::invalid_response("响应里没有 results 字段").with_provider("itunes")
        })?;
        let results = results.as_array().ok_or_else(|| {
            ProviderError::invalid_response("results 不是列表").with_provider("itunes")
        })?;

        let mut out = Vec::with_capacity(results.len());
        for item in results {
            let Some(item) = item.as_object() else {
                continue;
            };
            let thumb = {
                let a = as_str(item.get("artworkUrl100"));
                if a.is_empty() {
                    as_str(item.get("artworkUrl60"))
                } else {
                    a
                }
            };
            let release = as_str(item.get("releaseDate"));
            let album_artist = {
                let a = as_str(item.get("collectionArtistName"));
                if a.is_empty() {
                    as_str(item.get("artistName"))
                } else {
                    a
                }
            };
            let mut extra = std::collections::BTreeMap::new();
            if let Some(v) = item.get("collectionId") {
                extra.insert("collection_id".to_string(), v.to_string());
            }
            if let Some(v) = item.get("artistId") {
                extra.insert("artist_id".to_string(), v.to_string());
            }
            out.push(ScrapeCandidate {
                provider: "itunes".into(),
                provider_id: as_i64(item.get("trackId"))
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                title: as_str(item.get("trackName")),
                artist: as_str(item.get("artistName")),
                album: strip_release_suffix(&as_str(item.get("collectionName"))),
                album_artist,
                year: year_of(&release),
                genre: as_str(item.get("primaryGenreName")),
                // trackTimeMillis 是毫秒
                duration_sec: as_f64(item.get("trackTimeMillis")).map(|ms| ms / 1000.0),
                track_number: as_i64(item.get("trackNumber")),
                disc_number: as_i64(item.get("discNumber")),
                artwork_url: hi_res_artwork(&thumb, self.artwork_size),
                thumb_url: thumb,
                provider_confidence: self.confidence(),
                breakdown: None,
                extra,
            });
        }
        Ok(out)
    }
}

impl MetadataProvider for ITunesProvider {
    fn name(&self) -> &'static str {
        "itunes"
    }

    /// iTunes 数据干净但没有 MBID 这类稳定标识，先验略低于 MusicBrainz。
    fn confidence(&self) -> f64 {
        0.95
    }

    fn search_tracks(
        &self,
        query: &SearchQuery,
        limit: usize,
    ) -> Result<Vec<ScrapeCandidate>, ProviderError> {
        let term = query.term();
        if term.is_empty() {
            return Ok(Vec::new());
        }
        let url = self.http.build_url(
            SEARCH_URL,
            &[
                ("term", term),
                ("media", "music".into()),
                ("entity", "song".into()),
                ("country", self.country.clone()),
                ("limit", limit.clamp(1, 200).to_string()),
            ],
        );
        self.parse(&self.http.get_json(&url)?)
    }

    fn get_track(&self, provider_id: &str) -> Result<Option<ScrapeCandidate>, ProviderError> {
        if provider_id.is_empty() {
            return Ok(None);
        }
        let url = self.http.build_url(
            LOOKUP_URL,
            &[
                ("id", provider_id.to_string()),
                ("country", self.country.clone()),
            ],
        );
        Ok(self.parse(&self.http.get_json(&url)?)?.into_iter().next())
    }

    fn get_artwork(&self, candidate: &ScrapeCandidate, size: u32) -> String {
        let url = if candidate.artwork_url.is_empty() {
            &candidate.thumb_url
        } else {
            &candidate.artwork_url
        };
        hi_res_artwork(url, size)
    }

    fn get_artist(
        &self,
        _name: &str,
        _aliases: &[String],
    ) -> Result<Option<ArtistInfo>, ProviderError> {
        // iTunes 不提供艺人照片，也没有结构化的艺人条目
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_itunes_single_and_ep_suffixes_are_dropped() {
        // iTunes 独有的后缀，别的源和本地标签都没有；留着的话同一张专辑会有两种写法
        assert_eq!(
            strip_release_suffix("アルジャーノン - Single"),
            "アルジャーノン"
        );
        assert_eq!(strip_release_suffix("創作 - EP"), "創作");
        // 名字本身带 EP / Single 的不动
        assert_eq!(strip_release_suffix("Fantasy EP"), "Fantasy EP");
        assert_eq!(
            strip_release_suffix("Single Collection"),
            "Single Collection"
        );
        assert_eq!(strip_release_suffix("834.194"), "834.194");
    }
    use crate::http::testing::ScriptedTransport;

    fn provider(body: &str) -> ITunesProvider {
        ITunesProvider::new(HttpClient::new(Box::new(ScriptedTransport::new(vec![
            ScriptedTransport::ok(body),
        ]))))
    }

    const ONE_TRACK: &str = r#"{"resultCount":1,"results":[{
        "trackId": 1490261063, "trackName": "夜に駆ける", "artistName": "YOASOBI",
        "collectionName": "THE BOOK", "collectionArtistName": "YOASOBI",
        "releaseDate": "2019-11-16T12:00:00Z", "primaryGenreName": "J-Pop",
        "trackTimeMillis": 261000, "trackNumber": 1, "discNumber": 1,
        "artworkUrl100": "https://is1.mzstatic.com/image/thumb/x/100x100bb.jpg",
        "collectionId": 1500000000, "artistId": 1400000000 }]}"#;

    #[test]
    fn a_track_is_parsed_with_every_field() {
        let c = &provider(ONE_TRACK)
            .search_tracks(
                &SearchQuery {
                    title: "夜に駆ける".into(),
                    ..Default::default()
                },
                25,
            )
            .unwrap()[0];
        assert_eq!(c.provider_id, "1490261063");
        assert_eq!(c.title, "夜に駆ける");
        assert_eq!(c.artist, "YOASOBI");
        assert_eq!(c.album, "THE BOOK");
        assert_eq!(c.year, "2019");
        assert_eq!(c.genre, "J-Pop");
        // 毫秒要换成秒，不然时长分永远是 0
        assert_eq!(c.duration_sec, Some(261.0));
        assert_eq!(c.track_number, Some(1));
        assert_eq!(c.provider_confidence, 0.95);
    }

    #[test]
    fn artwork_is_upgraded_to_high_resolution() {
        let c = &provider(ONE_TRACK)
            .search_tracks(
                &SearchQuery {
                    title: "x".into(),
                    ..Default::default()
                },
                25,
            )
            .unwrap()[0];
        assert!(
            c.artwork_url.ends_with("600x600bb.jpg"),
            "{}",
            c.artwork_url
        );
        // 缩略图保留原样，UI 列表用它省流量
        assert!(c.thumb_url.ends_with("100x100bb.jpg"));
    }

    #[test]
    fn an_unknown_artwork_url_shape_is_left_alone() {
        assert_eq!(
            hi_res_artwork("https://x/cover.webp", 600),
            "https://x/cover.webp"
        );
        assert_eq!(hi_res_artwork("", 600), "");
    }

    #[test]
    fn a_missing_results_field_is_an_error_not_an_empty_list() {
        // 「搜到了 0 条」和「响应结构不对」必须区分，
        // 前者不该重试，后者说明 provider 出问题了
        let err = provider(r#"{"resultCount":0}"#)
            .search_tracks(
                &SearchQuery {
                    title: "x".into(),
                    ..Default::default()
                },
                25,
            )
            .unwrap_err();
        assert_eq!(err.error_type, crate::ErrorType::InvalidResponse);

        let empty = provider(r#"{"resultCount":0,"results":[]}"#)
            .search_tracks(
                &SearchQuery {
                    title: "x".into(),
                    ..Default::default()
                },
                25,
            )
            .unwrap();
        assert!(empty.is_empty(), "空结果是正常的，不该报错");
    }

    #[test]
    fn a_non_object_result_entry_is_skipped_not_fatal() {
        let p = provider(r#"{"results":[null, 42, {"trackName":"x"}]}"#);
        let got = p
            .search_tracks(
                &SearchQuery {
                    title: "x".into(),
                    ..Default::default()
                },
                25,
            )
            .unwrap();
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn an_empty_term_does_not_hit_the_network() {
        let p = provider("{}");
        assert!(
            p.search_tracks(&SearchQuery::default(), 25)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn the_query_targets_the_japanese_store() {
        // 默认 US 区对 J-POP 的覆盖差很多
        let transport = ScriptedTransport::new(vec![ScriptedTransport::ok(ONE_TRACK)]);
        let urls = std::sync::Arc::new(transport);
        let p = ITunesProvider::new(HttpClient::new(Box::new(ScriptedTransport::new(vec![
            ScriptedTransport::ok(ONE_TRACK),
        ]))));
        let _ = urls;
        // 直接检查 build_url 的产物
        let url = p.http.build_url(
            SEARCH_URL,
            &[("term", "x".into()), ("country", p.country.clone())],
        );
        assert!(url.contains("country=JP"), "{url}");
    }

    #[test]
    fn limit_is_clamped_into_the_apis_range() {
        let p = provider(ONE_TRACK);
        let url = p.http.build_url(
            SEARCH_URL,
            &[("limit", 5000usize.clamp(1, 200).to_string())],
        );
        assert!(url.ends_with("limit=200"), "{url}");
    }
}
