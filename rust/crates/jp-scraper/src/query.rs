//! 把一个本地文件变成一串**由准到宽**的查询。
//!
//! 一次查不到不代表这首歌不存在，往往只是写法对不上：
//! 文件写「群青 - TV size」，provider 收录的是「群青」；
//! 文件写「キタニタツヤ/suis」，provider 那边只有「キタニタツヤ」。
//!
//! 所以这里生成多个查询，resolver 按顺序试，拿到足够好的结果就停——
//! 既不会因为第一个写法不对就放弃，也不会每首歌都发五六个请求。

use std::collections::BTreeSet;

use jp_normalize::{matching_key, normalize_artist, normalize_title};

use crate::models::{SearchQuery, TrackFile};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QueryBuilderConfig {
    /// 最多生成几条查询。查询是网络请求，不能无限展开。
    pub max_queries: usize,
    /// 合作曲拆开后，最多用前几位歌手各查一次
    pub max_artist_parts: usize,
}

impl Default for QueryBuilderConfig {
    fn default() -> Self {
        Self {
            max_queries: 5,
            max_artist_parts: 2,
        }
    }
}

/// `TrackFile` → `[SearchQuery]`，由准到宽排列。
#[derive(Debug, Clone, Default)]
pub struct QueryBuilder {
    pub config: QueryBuilderConfig,
}

impl QueryBuilder {
    pub fn new(config: QueryBuilderConfig) -> Self {
        Self { config }
    }

    pub fn build(&self, track: &TrackFile) -> Vec<SearchQuery> {
        let title = normalize_title(track.title());
        let artist = normalize_artist(track.artist());
        if title.original.trim().is_empty() && title.base_display.is_empty() {
            return Vec::new();
        }

        let full_title = title.original.trim().to_string();
        let base_title = title.base_display.trim().to_string();
        let full_artist = artist.original.trim().to_string();
        let base_artist = artist
            .parts
            .first()
            .map(|p| p.trim().to_string())
            .unwrap_or_default();
        // Python 里反复出现的 `base_title or full_title`
        let short = if base_title.is_empty() {
            full_title.clone()
        } else {
            base_title.clone()
        };

        // 由准到宽。每一档都带上 duration / track number，打分时用得着。
        let mut drafts: Vec<(&str, String, String, String)> = vec![
            (
                "full",
                full_title.clone(),
                full_artist,
                track.album().into(),
            ),
            (
                "no-version",
                base_title,
                track.artist().trim().to_string(),
                track.album().into(),
            ),
            ("primary-artist", short.clone(), base_artist, String::new()),
            ("title-only", short.clone(), String::new(), String::new()),
        ];

        // 合作曲：主唱查不到时，用第二位歌手再试一次
        for extra in artist
            .parts
            .iter()
            .skip(1)
            .take(self.config.max_artist_parts.saturating_sub(1))
        {
            drafts.push((
                "collab-artist",
                short.clone(),
                extra.trim().to_string(),
                String::new(),
            ));
        }
        // 曲名里 feat. 的那位有时才是 provider 记的主唱
        if let Some(featured) = artist.featured.first().or(title.featured.first()) {
            drafts.push((
                "featured-artist",
                short.clone(),
                featured.trim().to_string(),
                String::new(),
            ));
        }

        let mut out = Vec::new();
        let mut seen: BTreeSet<(String, String, String)> = BTreeSet::new();
        for (kind, q_title, q_artist, q_album) in drafts {
            if q_title.trim().is_empty() {
                continue;
            }
            let key = (
                matching_key(&q_title),
                matching_key(&q_artist),
                matching_key(&q_album),
            );
            if !seen.insert(key) {
                continue;
            }
            out.push(SearchQuery {
                title: q_title,
                artist: q_artist,
                album: q_album,
                duration_sec: track.duration_sec,
                track_number: track.track_number,
                kind: kind.to_string(),
            });
            if out.len() >= self.config.max_queries {
                break;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(title: &str, artist: &str, album: &str) -> TrackFile {
        TrackFile {
            path: "D:/a/1.flac".into(),
            embedded_title: title.into(),
            embedded_artist: artist.into(),
            embedded_album: album.into(),
            ..Default::default()
        }
    }

    #[test]
    fn queries_go_from_precise_to_broad() {
        let qs = QueryBuilder::default().build(&track("群青 - TV size", "YOASOBI", "THE BOOK"));
        let kinds: Vec<&str> = qs.iter().map(|q| q.kind.as_str()).collect();
        assert_eq!(kinds[0], "full");
        assert!(kinds.contains(&"no-version"));
        // 第一条要带上完整写法，最后几条只剩曲名
        assert_eq!(qs[0].title, "群青 - TV size");
        assert!(qs.iter().any(|q| q.title == "群青" && q.artist.is_empty()));
    }

    #[test]
    fn a_collaboration_gets_a_query_per_artist() {
        let qs = QueryBuilder::default().build(&track("夜行", "キタニタツヤ/suis", ""));
        let artists: Vec<&str> = qs.iter().map(|q| q.artist.as_str()).collect();
        assert!(artists.contains(&"キタニタツヤ"));
        assert!(
            artists.contains(&"suis"),
            "第二位歌手也该单独试一次：{artists:?}"
        );
    }

    #[test]
    fn a_featured_artist_gets_its_own_query() {
        // provider 常常把 feat. 的那位记成主唱
        let qs = QueryBuilder::default().build(&track("アイドル (feat. Ado)", "YOASOBI", ""));
        assert!(
            qs.iter().any(|q| q.artist == "Ado"),
            "{:?}",
            qs.iter().map(|q| (&q.kind, &q.artist)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn duplicate_queries_are_dropped() {
        // 曲名没有版本后缀时 full 和 no-version 是同一条
        let qs = QueryBuilder::default().build(&track("夜行", "ヨルシカ", ""));
        let keys: Vec<(String, String)> = qs
            .iter()
            .map(|q| (matching_key(&q.title), matching_key(&q.artist)))
            .collect();
        let unique: BTreeSet<_> = keys.iter().cloned().collect();
        assert_eq!(keys.len(), unique.len(), "有重复查询：{keys:?}");
    }

    #[test]
    fn the_number_of_queries_is_capped() {
        // 查询就是网络请求，不能无限展开
        let qs = QueryBuilder::new(QueryBuilderConfig {
            max_queries: 2,
            ..Default::default()
        })
        .build(&track("群青 - TV size", "A/B/C", "X"));
        assert_eq!(qs.len(), 2);
    }

    #[test]
    fn a_track_without_a_title_produces_nothing() {
        assert!(
            QueryBuilder::default()
                .build(&track("", "YOASOBI", ""))
                .is_empty()
        );
    }

    #[test]
    fn duration_and_track_number_ride_along() {
        // 打分时要用，每一档都得带着
        let mut t = track("夜行", "ヨルシカ", "");
        t.duration_sec = Some(233.0);
        t.track_number = Some(3);
        for q in QueryBuilder::default().build(&t) {
            assert_eq!(q.duration_sec, Some(233.0));
            assert_eq!(q.track_number, Some(3));
        }
    }
}
