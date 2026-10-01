//! Deezer。免 key。主要用来拿**歌手照片**——iTunes 不提供艺人图。
//!
//! 一个必须处理的坑：Deezer 对没有照片的歌手也会返回 `picture_xl`，
//! 只是 URL 里的图片 hash 是空的（`.../images/artist//1000x1000-...`），
//! 下下来是一张灰色人形占位图。初星学園 的两条记录拿到的就是同一个
//! 32KB 占位图。
//!
//! 另一个：大量日本歌手在 Deezer 上按罗马字收录
//! （きのこ帝国 → Kinoko Teikoku、ずっと真夜中でいいのに。→ ZUTOMAYO），
//! 所以不能要求返回的名字和查询字面相同。别名来自 MusicBrainz。

use jp_normalize::{matching_key, normalize_artist};

use crate::error::ProviderError;
use crate::http::HttpClient;
use crate::matching::artist_similarity;
use crate::models::{ScrapeCandidate, SearchQuery};

use super::{ArtistInfo, MetadataProvider, as_f64, as_i64, as_str};

const ARTIST_URL: &str = "https://api.deezer.com/search/artist";
const TRACK_URL: &str = "https://api.deezer.com/search";

/// 空 hash 的占位图 URL。
pub fn is_placeholder_picture(url: &str) -> bool {
    url.contains("/images/artist//")
}

pub struct DeezerProvider {
    http: HttpClient,
}

impl DeezerProvider {
    pub fn new(http: HttpClient) -> Self {
        Self {
            http: http.with_provider_name("deezer"),
        }
    }
}

impl MetadataProvider for DeezerProvider {
    fn name(&self) -> &'static str {
        "deezer"
    }

    /// 曲目元数据不如 iTunes/MB 干净，先验压低；艺人照片才是它的主职。
    fn confidence(&self) -> f64 {
        0.85
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
            TRACK_URL,
            &[("q", term), ("limit", limit.clamp(1, 100).to_string())],
        );
        let payload = self.http.get_json(&url)?;
        let object = payload.as_object().ok_or_else(|| {
            ProviderError::invalid_response("响应不是 JSON 对象").with_provider("deezer")
        })?;
        let items = object.get("data").ok_or_else(|| {
            ProviderError::invalid_response("响应里没有 data 字段").with_provider("deezer")
        })?;
        let items = items.as_array().ok_or_else(|| {
            ProviderError::invalid_response("data 不是列表").with_provider("deezer")
        })?;

        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let Some(item) = item.as_object() else {
                continue;
            };
            let album = item.get("album");
            let artist = item.get("artist");
            let cover = |key: &str| album.and_then(|a| a.get(key)).map(|v| as_str(Some(v)));
            let artwork = cover("cover_xl")
                .filter(|s| !s.is_empty())
                .or_else(|| cover("cover_big"))
                .unwrap_or_default();
            let thumb = cover("cover_small")
                .filter(|s| !s.is_empty())
                .or_else(|| cover("cover_medium"))
                .unwrap_or_default();
            out.push(ScrapeCandidate {
                provider: "deezer".into(),
                provider_id: as_i64(item.get("id"))
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                title: as_str(item.get("title")),
                artist: as_str(artist.and_then(|a| a.get("name"))),
                album: as_str(album.and_then(|a| a.get("title"))),
                // Deezer 的 duration 单位就是秒
                duration_sec: as_f64(item.get("duration")),
                artwork_url: artwork,
                thumb_url: thumb,
                provider_confidence: self.confidence(),
                ..Default::default()
            });
        }
        Ok(out)
    }

    /// 拿歌手照片。
    ///
    /// 优先取能和名字/别名对上的那条；对不上就退而取**原名查询**的第一条
    /// 非占位图——Deezer 自己的搜索已经做过一轮匹配，查不到的名字它返回空列表。
    fn get_artist(
        &self,
        name: &str,
        aliases: &[String],
    ) -> Result<Option<ArtistInfo>, ProviderError> {
        let mut queries: Vec<String> = std::iter::once(name.to_string())
            .chain(aliases.iter().cloned())
            .filter(|q| !q.trim().is_empty())
            .collect();
        if queries.is_empty() {
            return Ok(None);
        }
        // Python 用 dict.fromkeys 去重且保序
        let mut seen = std::collections::BTreeSet::new();
        queries.retain(|q| seen.insert(q.clone()));

        let accept: std::collections::BTreeSet<String> =
            queries.iter().map(|q| matching_key(q)).collect();
        let mut fallback: Option<ArtistInfo> = None;

        for (index, query_name) in queries.iter().enumerate() {
            let url = self.http.build_url(
                ARTIST_URL,
                &[("q", query_name.clone()), ("limit", "5".into())],
            );
            let payload = self.http.get_json(&url)?;
            let object = payload.as_object().ok_or_else(|| {
                ProviderError::invalid_response("响应不是 JSON 对象").with_provider("deezer")
            })?;
            for item in object
                .get("data")
                .and_then(|v| v.as_array())
                .unwrap_or(&Vec::new())
            {
                let Some(item) = item.as_object() else {
                    continue;
                };
                let image_url = {
                    let xl = as_str(item.get("picture_xl"));
                    if xl.is_empty() {
                        as_str(item.get("picture_big"))
                    } else {
                        xl
                    }
                };
                // 灰色人形占位图，下下来也没用
                if is_placeholder_picture(&image_url) {
                    continue;
                }
                let got = as_str(item.get("name"));
                let record = ArtistInfo {
                    provider: "deezer".into(),
                    provider_id: as_i64(item.get("id"))
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                    name: got.clone(),
                    image_url,
                    fans: as_i64(item.get("nb_fan")).unwrap_or(0),
                    ..Default::default()
                };
                if accept.contains(&matching_key(&got)) || artist_close(query_name, &got) {
                    return Ok(Some(record));
                }
                if fallback.is_none() && index == 0 {
                    fallback = Some(record);
                }
            }
        }
        Ok(fallback)
    }
}

fn artist_close(want: &str, got: &str) -> bool {
    artist_similarity(&normalize_artist(want), &normalize_artist(got)) >= 0.82
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::testing::ScriptedTransport;
    use serde_json::json;

    fn provider(bodies: &[&str]) -> DeezerProvider {
        DeezerProvider::new(HttpClient::new(Box::new(ScriptedTransport::new(
            bodies.iter().map(|b| ScriptedTransport::ok(b)).collect(),
        ))))
    }

    #[test]
    fn a_placeholder_picture_is_recognised() {
        assert!(is_placeholder_picture(
            "https://e-cdns-images.dzcdn.net/images/artist//1000x1000-000000-80-0-0.jpg"
        ));
        assert!(!is_placeholder_picture(
            "https://e-cdns-images.dzcdn.net/images/artist/abc123/1000x1000-000000-80-0-0.jpg"
        ));
    }

    #[test]
    fn an_artist_with_only_a_placeholder_is_skipped() {
        // 初星学園 的两条记录拿到的就是同一张 32KB 灰色人形图
        let body = json!({"data":[
            {"id":1,"name":"初星学園","picture_xl":"https://x/images/artist//1000x1000-a.jpg"}
        ]})
        .to_string();
        assert!(
            provider(&[&body])
                .get_artist("初星学園", &[])
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_romanised_name_still_matches_through_aliases() {
        // ずっと真夜中でいいのに。在 Deezer 上叫 ZUTOMAYO
        let body = json!({"data":[
            {"id":7,"name":"ZUTOMAYO","picture_xl":"https://x/images/artist/h7/1000x1000.jpg",
             "nb_fan":12345}
        ]})
        .to_string();
        let got = provider(&[&body])
            .get_artist("ずっと真夜中でいいのに。", &["ZUTOMAYO".to_string()])
            .unwrap()
            .unwrap();
        assert_eq!(got.name, "ZUTOMAYO");
        assert_eq!(got.fans, 12345);
        assert!(!got.image_url.is_empty());
    }

    #[test]
    fn an_unrelated_first_result_becomes_a_fallback_not_a_match() {
        // Deezer 的搜索已经做过一轮匹配，但名字对不上时只当备选
        let body = json!({"data":[
            {"id":9,"name":"Someone Else","picture_xl":"https://x/images/artist/z9/1000.jpg"}
        ]})
        .to_string();
        let got = provider(&[&body]).get_artist("ヨルシカ", &[]).unwrap();
        assert!(got.is_some(), "第一条查询的结果可以当备选");
        assert_eq!(got.unwrap().name, "Someone Else");
    }

    #[test]
    fn tracks_are_parsed_with_seconds_not_millis() {
        // Deezer 的 duration 单位是秒，不是毫秒——弄错的话时长分永远是 0
        let body = json!({"data":[{
            "id":123,"title":"夜行","duration":233,
            "artist":{"name":"ヨルシカ"},
            "album":{"title":"盗作","cover_xl":"https://x/xl.jpg","cover_small":"https://x/s.jpg"}
        }]})
        .to_string();
        let c = &provider(&[&body])
            .search_tracks(
                &SearchQuery {
                    title: "夜行".into(),
                    ..Default::default()
                },
                25,
            )
            .unwrap()[0];
        assert_eq!(c.duration_sec, Some(233.0));
        assert_eq!(c.artist, "ヨルシカ");
        assert_eq!(c.album, "盗作");
        assert_eq!(c.artwork_url, "https://x/xl.jpg");
        assert_eq!(c.provider_confidence, 0.85);
    }

    #[test]
    fn a_missing_data_field_is_an_error() {
        let err = provider(&[r#"{"total":0}"#])
            .search_tracks(
                &SearchQuery {
                    title: "x".into(),
                    ..Default::default()
                },
                25,
            )
            .unwrap_err();
        assert_eq!(err.error_type, crate::ErrorType::InvalidResponse);
    }

    #[test]
    fn no_name_means_no_request() {
        assert!(provider(&[]).get_artist("  ", &[]).unwrap().is_none());
    }
}
