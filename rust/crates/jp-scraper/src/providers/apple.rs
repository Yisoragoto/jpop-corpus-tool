//! Official Apple Music Catalog API only. It does not expose lyric text/timing.
//! https://developer.apple.com/documentation/applemusicapi
use super::{MetadataProvider, as_i64, as_str, date_of, year_of};
use crate::{HttpClient, ProviderError, ScrapeCandidate, SearchQuery};

pub struct AppleMusicProvider {
    http: HttpClient,
    storefront: String,
    token: String,
}

impl AppleMusicProvider {
    pub fn new(http: HttpClient, storefront: &str, token: &str) -> Result<Self, ProviderError> {
        let region = storefront.trim().to_ascii_lowercase();
        if region.len() != 2 || !region.bytes().all(|b| b.is_ascii_lowercase()) {
            return Err(ProviderError::invalid_response("Apple 地区应为两位字母"));
        }
        let token = token.trim().strip_prefix("Bearer ").unwrap_or(token.trim());
        if token.is_empty()
            || token.len() > 16384
            || !token.bytes().all(|b| (33..=126).contains(&b))
        {
            return Err(ProviderError::unavailable(
                "Apple 元数据需要有效的 Developer Token",
            ));
        }
        Ok(Self {
            http: http.with_provider_name("apple"),
            storefront: region,
            token: token.into(),
        })
    }

    fn request(&self, url: &str) -> Result<serde_json::Value, ProviderError> {
        let bytes = self
            .http
            .get_bytes_with(
                url,
                &[("Authorization".into(), format!("Bearer {}", self.token))],
            )
            .map_err(|e| {
                ProviderError::new(
                    e.error_type,
                    "Apple 目录请求失败，请检查网络、令牌有效期、地区或限额",
                )
                .with_provider("apple")
            })?;
        serde_json::from_slice(&bytes)
            .map_err(|_| ProviderError::invalid_response("Apple 目录响应不是有效 JSON"))
    }

    fn candidate(&self, item: &serde_json::Value) -> Option<ScrapeCandidate> {
        let a = &item["attributes"];
        let id = as_str(item.get("id"));
        if id.is_empty() || as_str(a.get("name")).is_empty() {
            return None;
        }
        let artwork = as_str(a["artwork"].get("url"))
            .replace("{w}", "600")
            .replace("{h}", "600");
        let mut c = ScrapeCandidate {
            provider: "apple".into(),
            provider_id: id,
            title: as_str(a.get("name")),
            artist: as_str(a.get("artistName")),
            album: super::itunes::strip_release_suffix(&as_str(a.get("albumName"))),
            year: year_of(&as_str(a.get("releaseDate"))),
            release_date: date_of(&as_str(a.get("releaseDate"))),
            duration_sec: a["durationInMillis"]
                .as_f64()
                .filter(|v| v.is_finite() && *v > 0.0)
                .map(|v| v / 1000.0),
            track_number: as_i64(a.get("trackNumber")),
            disc_number: as_i64(a.get("discNumber")),
            genre: a["genreNames"]
                .as_array()
                .map(|v| {
                    v.iter()
                        .filter_map(|g| g.as_str())
                        .collect::<Vec<_>>()
                        .join(" / ")
                })
                .unwrap_or_default(),
            artwork_url: artwork.clone(),
            thumb_url: artwork,
            provider_confidence: 1.0,
            ..Default::default()
        };
        for key in ["isrc", "url"] {
            if let Some(v) = a[key].as_str() {
                c.extra.insert(key.into(), v.into());
            }
        }
        if let Some(value) = a["hasLyrics"].as_bool() {
            c.extra.insert("hasLyrics".into(), value.to_string());
        }
        Some(c)
    }
}

impl MetadataProvider for AppleMusicProvider {
    fn name(&self) -> &'static str {
        "apple"
    }
    fn search_tracks(
        &self,
        query: &SearchQuery,
        limit: usize,
    ) -> Result<Vec<ScrapeCandidate>, ProviderError> {
        if let Some(id) = query.provider_ids.get("apple") {
            return Ok(self.get_track(id)?.into_iter().collect());
        }
        if !query.isrc.is_empty() {
            let url = self.http.build_url(
                &format!(
                    "https://api.music.apple.com/v1/catalog/{}/songs",
                    self.storefront
                ),
                &[("filter[isrc]", query.isrc.clone())],
            );
            let data = self.request(&url)?;
            return Ok(data["data"]
                .as_array()
                .ok_or_else(|| ProviderError::invalid_response("Apple ISRC 响应缺少 data"))?
                .iter()
                .filter_map(|i| self.candidate(i))
                .collect());
        }
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let url = self.http.build_url(
            &format!(
                "https://api.music.apple.com/v1/catalog/{}/search",
                self.storefront
            ),
            &[
                ("term", query.term()),
                ("types", "songs".into()),
                ("limit", limit.clamp(1, 25).to_string()),
            ],
        );
        let data = self.request(&url)?;
        let items = data["results"]["songs"]["data"]
            .as_array()
            .ok_or_else(|| ProviderError::invalid_response("Apple 搜索缺少 songs.data"))?;
        Ok(items.iter().filter_map(|i| self.candidate(i)).collect())
    }
    fn get_track(&self, id: &str) -> Result<Option<ScrapeCandidate>, ProviderError> {
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ProviderError::invalid_response("Apple 曲目编号无效"));
        }
        let data = self.request(&format!(
            "https://api.music.apple.com/v1/catalog/{}/songs/{id}",
            self.storefront
        ))?;
        Ok(data["data"]
            .as_array()
            .and_then(|i| i.first())
            .and_then(|i| self.candidate(i)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_metadata_keeps_isrc_without_claiming_lyric_access() {
        let p = AppleMusicProvider::new(
            HttpClient::new(Box::new(crate::http::testing::ScriptedTransport::new(
                vec![],
            ))),
            "jp",
            "test",
        );
        // Transport is never called; parser fixture contains the official catalog shape.
        let p = p.unwrap();
        let c=p.candidate(&serde_json::json!({"id":"1","attributes":{"name":"試験","artistName":"歌手","durationInMillis":120000,"isrc":"JPTEST000001","hasLyrics":true,"artwork":{"url":"https://example.com/{w}x{h}.jpg"}}})).unwrap();
        assert_eq!(c.extra["isrc"], "JPTEST000001");
        assert_eq!(c.extra["hasLyrics"], "true");
        assert_eq!(c.duration_sec, Some(120.0));
        assert!(c.artwork_url.contains("600x600"));
    }

    #[test]
    fn official_catalog_search_and_isrc_filter_use_expected_response_shapes() {
        use crate::http::testing::ScriptedTransport;
        let fixture = r#"{"results":{"songs":{"data":[{"id":"12","attributes":{"name":"試験","artistName":"歌手"}}]}}}"#;
        let p = AppleMusicProvider::new(
            HttpClient::new(Box::new(ScriptedTransport::new(vec![
                ScriptedTransport::ok(fixture),
            ]))),
            "jp",
            "secret",
        )
        .unwrap();
        assert_eq!(
            p.search_tracks(
                &SearchQuery {
                    title: "試験".into(),
                    artist: "歌手".into(),
                    ..Default::default()
                },
                20
            )
            .unwrap()[0]
                .provider_id,
            "12"
        );
        let p = AppleMusicProvider::new(
            HttpClient::new(Box::new(ScriptedTransport::new(vec![
                ScriptedTransport::ok(
                    r#"{"data":[{"id":"12","attributes":{"name":"試験","isrc":"JPTEST000001"}}]}"#,
                ),
            ]))),
            "jp",
            "secret",
        )
        .unwrap();
        assert_eq!(
            p.search_tracks(
                &SearchQuery {
                    isrc: "JPTEST000001".into(),
                    ..Default::default()
                },
                20
            )
            .unwrap()[0]
                .extra["isrc"],
            "JPTEST000001"
        );
    }

    #[test]
    fn authentication_failure_is_explicit_and_redacts_transport_secrets() {
        use crate::http::testing::ScriptedTransport;
        let p = AppleMusicProvider::new(
            HttpClient::new(Box::new(ScriptedTransport::new(vec![Err(
                ProviderError::network("oops secret-token"),
            )])))
            .with_config(crate::HttpConfig {
                retries: 0,
                ..Default::default()
            }),
            "jp",
            "secret-token",
        )
        .unwrap();
        let err = p
            .search_tracks(
                &SearchQuery {
                    title: "試験".into(),
                    ..Default::default()
                },
                20,
            )
            .unwrap_err();
        assert!(err.message.contains("令牌"));
        assert!(!err.message.contains("secret-token"));
    }
}
