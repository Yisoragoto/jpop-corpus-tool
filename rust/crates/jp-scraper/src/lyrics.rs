//! 在线找歌词。
//!
//! 0.1.x 的歌词是 `scripts/01_fetch_lrc.py` 用 `syncedlyrics` 下回来的，
//! 存成 `raw/lyrics_lrc/{song_id}.lrc`；新版导入只认本地已有的 .lrc，
//! 于是从别处导进来的歌（旁边没有 .lrc）就一行歌词都没有。这个模块把那一步补回来。
//!
//! **源用网易云**。实测（2026-10-01）lrclib 对这批日文曲目几乎没有收录：
//! 「ヨルシカ 夏、バス停、君を待つ」「ずっと真夜中でいいのに。 勘ぐれい」「Vaundy 再会」
//! 搜出来都是 0 条；网易云一搜就有，而且给的是带逐行时间轴的 LRC
//! （`[00:15.00]いつかあの空が僕を忘れたとして`），连「作词 : 山口　一郎」这样的
//! 信用行都带着——正好是 `jp-import` 的 LRC 解析认得的那一套。
//!
//! **挑不准就不要**：曲名不像、时长差太多，宁可报「没找到」也不要塞一份别的歌的歌词进去。

use serde::Deserialize;

use crate::error::ProviderError;
use crate::http::HttpClient;
use crate::matching::artist_similarity;
use crate::similarity::difflib_ratio;

/// 搜到的一条候选。
#[derive(Debug, Clone, PartialEq)]
pub struct LyricsHit {
    pub id: String,
    pub title: String,
    pub artist: String,
    /// 网易云给的是毫秒，这里换成秒；拿不到是 None
    pub duration_sec: Option<f64>,
}

/// 曲名至少像到这个程度才考虑。0.78 是「ネイティブダンサー」对
/// 「ネイティブダンサー(rei harakami へっぽこre-arrange)」这类改编版的分界
const TITLE_MIN: f64 = 0.78;

/// 歌手至少要像到这个程度。
///
/// **曲名一模一样也不放宽**：同名曲太多了（「再会」Vaundy / 八代亜紀 / KIRINJI 都有），
/// 塞错一首的歌词比没有歌词糟得多。代价是网易云把歌手写成罗马字时会漏掉，
/// 那种情况仍然可以自己放一个 .lrc 在歌旁边。
const ARTIST_MIN: f64 = 0.6;

/// 时长差超过这么多秒就认为不是同一个版本（live、remix、不同剪辑）
const DURATION_TOLERANCE_SEC: f64 = 8.0;

/// 从候选里挑一条。挑不出来返回 None——**宁可没有歌词，也不要别的歌的歌词**。
///
/// 纯函数，单独测：排序和阈值是这一块唯一会出错的地方。
pub fn pick_best<'a>(
    hits: &'a [LyricsHit],
    want_title: &str,
    want_artist: &str,
    want_duration_sec: Option<f64>,
) -> Option<&'a LyricsHit> {
    let want_title_norm = jp_normalize::normalize_title(want_title);
    let want_artist_norm = jp_normalize::normalize_artist(want_artist);

    let mut best: Option<(f64, &LyricsHit)> = None;
    for hit in hits {
        let title_norm = jp_normalize::normalize_title(&hit.title);
        let artist_norm = jp_normalize::normalize_artist(&hit.artist);
        let title_score = if title_norm.normalized == want_title_norm.normalized {
            1.0
        } else {
            difflib_ratio(&want_title_norm.normalized, &title_norm.normalized)
        };
        if title_score < TITLE_MIN {
            continue;
        }
        let artist_score = artist_similarity(&want_artist_norm, &artist_norm);
        if artist_score < ARTIST_MIN {
            continue;
        }
        // 时长对不上的多半是 live / remix / 另一版剪辑
        let duration_gap = match (want_duration_sec, hit.duration_sec) {
            (Some(want), Some(got)) => {
                let gap = (want - got).abs();
                if gap > DURATION_TOLERANCE_SEC {
                    continue;
                }
                gap
            }
            _ => DURATION_TOLERANCE_SEC, // 不知道时长就不加分也不扣分
        };
        // 曲名最重要，其次歌手，时长用来在同分里挑最接近的那一版
        let score = title_score * 2.0 + artist_score - duration_gap / 100.0;
        if best.as_ref().is_none_or(|(b, _)| score > *b) {
            best = Some((score, hit));
        }
    }
    best.map(|(_, hit)| hit)
}

#[derive(Deserialize)]
struct SearchEnvelope {
    result: Option<SearchResult>,
}

#[derive(Deserialize)]
struct SearchResult {
    #[serde(default)]
    songs: Vec<SearchSong>,
}

#[derive(Deserialize)]
struct SearchSong {
    id: i64,
    name: String,
    #[serde(default)]
    duration: i64,
    #[serde(default)]
    artists: Vec<NamedEntity>,
}

#[derive(Deserialize)]
struct NamedEntity {
    name: String,
}

#[derive(Deserialize)]
struct LyricEnvelope {
    lrc: Option<LyricBody>,
}

#[derive(Deserialize)]
struct LyricBody {
    #[serde(default)]
    lyric: String,
}

/// 网易云的公开接口。和 0.1.x 用的 `syncedlyrics` 走的是同一条路。
pub struct NeteaseLyrics {
    client: HttpClient,
}

impl NeteaseLyrics {
    pub fn new(client: HttpClient) -> Self {
        // 不带 Referer 这两个接口会拒绝
        Self {
            client: client
                .with_provider_name("netease-lyrics")
                .with_user_agent(
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) jpop-corpus-tool/0.2",
                ),
        }
    }

    fn headers() -> Vec<(String, String)> {
        vec![("Referer".to_string(), "https://music.163.com/".to_string())]
    }

    /// 按「歌手 曲名」搜。
    pub fn search(&self, title: &str, artist: &str, limit: usize) -> Result<Vec<LyricsHit>, ProviderError> {
        let term = format!("{artist} {title}");
        let url = format!(
            "https://music.163.com/api/search/get?type=1&limit={limit}&offset=0&s={}",
            urlencode(&term)
        );
        let raw = self.client.get_bytes_with(&url, &Self::headers())?;
        let parsed: SearchEnvelope = serde_json::from_slice(&raw).map_err(|e| {
            ProviderError::new(crate::ErrorType::InvalidResponse, format!("搜索结果解析失败: {e}"))
        })?;
        Ok(parsed
            .result
            .map(|r| r.songs)
            .unwrap_or_default()
            .into_iter()
            .map(|song| LyricsHit {
                id: song.id.to_string(),
                title: song.name,
                artist: song
                    .artists
                    .iter()
                    .map(|a| a.name.clone())
                    .collect::<Vec<_>>()
                    .join("/"),
                duration_sec: (song.duration > 0).then(|| song.duration as f64 / 1000.0),
            })
            .collect())
    }

    /// 取这首歌的 LRC。没有歌词（纯音乐之类）时返回 None。
    pub fn lyric(&self, id: &str) -> Result<Option<String>, ProviderError> {
        let url = format!("https://music.163.com/api/song/lyric?id={id}&lv=1&kv=1&tv=-1");
        let raw = self.client.get_bytes_with(&url, &Self::headers())?;
        let parsed: LyricEnvelope = serde_json::from_slice(&raw).map_err(|e| {
            ProviderError::new(crate::ErrorType::InvalidResponse, format!("歌词解析失败: {e}"))
        })?;
        let text = parsed.lrc.map(|l| l.lyric).unwrap_or_default();
        Ok((!text.trim().is_empty()).then_some(text))
    }

    /// 搜 + 挑 + 取。挑不出来或者那条没有歌词，返回 None。
    pub fn best_lyrics(
        &self,
        title: &str,
        artist: &str,
        duration_sec: Option<f64>,
    ) -> Result<Option<(LyricsHit, String)>, ProviderError> {
        let hits = self.search(title, artist, 10)?;
        let Some(best) = pick_best(&hits, title, artist, duration_sec).cloned() else {
            return Ok(None);
        };
        Ok(self.lyric(&best.id)?.map(|text| (best, text)))
    }
}

/// 百分号编码。查询串里有日文和空格，不编码接口会返回空结果
fn urlencode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(id: &str, title: &str, artist: &str, duration: f64) -> LyricsHit {
        LyricsHit {
            id: id.into(),
            title: title.into(),
            artist: artist.into(),
            duration_sec: Some(duration),
        }
    }

    #[test]
    fn the_same_song_wins_over_a_remix_of_it() {
        // 真实搜索结果：同名的原曲和一个改编版，时长差了 100 秒
        let hits = vec![
            hit("823935", "ネイティブダンサー(rei harakami へっぽこre-arrange)", "サカナクション", 369.0),
            hit("22635273", "ネイティブダンサー", "サカナクション", 264.0),
        ];
        let best = pick_best(&hits, "ネイティブダンサー", "サカナクション", Some(264.2)).unwrap();
        assert_eq!(best.id, "22635273");
    }

    #[test]
    fn a_different_song_is_not_accepted() {
        // 搜索兜底回来的无关结果，宁可没有歌词也不要它
        let hits = vec![hit("254143", "失忆", "梁静茹", 249.0)];
        assert!(pick_best(&hits, "ネイティブダンサー", "サカナクション", Some(264.0)).is_none());
    }

    #[test]
    fn the_live_version_is_rejected_by_duration() {
        // 曲名歌手都一样，只有时长差着——多半是 live 或另一版剪辑
        let hits = vec![hit("1", "再会", "Vaundy", 400.0)];
        assert!(pick_best(&hits, "再会", "Vaundy", Some(250.0)).is_none());
        // 时长不知道时不拦（老库里有些歌没有 duration）
        assert!(pick_best(&hits, "再会", "Vaundy", None).is_some());
    }

    /// **这是故意的**：曲名一模一样、时长也对得上，但歌手名对不上，仍然不要。
    ///
    /// 同名曲太多（「再会」Vaundy / 八代亜紀 / KIRINJI 都有），塞错一首的歌词
    /// 比没有歌词糟得多。代价是网易云偶尔把歌手写成罗马字（ZUTOMAYO）时会漏，
    /// 那种情况自己放一个 .lrc 在歌旁边即可。
    #[test]
    fn a_romanised_artist_is_not_guessed_at() {
        let hits = vec![hit("2", "勘ぐれい", "ZUTOMAYO", 230.0)];
        assert!(pick_best(&hits, "勘ぐれい", "ずっと真夜中でいいのに。", Some(230.0)).is_none());
        // 写法一致（哪怕差一个句点）就认
        let hits = vec![hit("2", "勘ぐれい", "ずっと真夜中でいいのに", 230.0)];
        assert!(pick_best(&hits, "勘ぐれい", "ずっと真夜中でいいのに。", Some(230.0)).is_some());
    }

    #[test]
    fn a_similar_title_still_needs_the_right_artist() {
        // 同名曲很多，曲名只是「像」的时候要靠歌手把关
        let hits = vec![hit("3", "再会", "八代亜紀", 250.0)];
        assert!(pick_best(&hits, "再会", "Vaundy", Some(250.0)).is_none());
    }

    #[test]
    fn japanese_queries_are_percent_encoded() {
        // 不编码的话接口会把查询串当空的，返回一堆不相干的歌
        assert_eq!(urlencode("ネイティブ"), "%E3%83%8D%E3%82%A4%E3%83%86%E3%82%A3%E3%83%96");
        assert_eq!(urlencode("Vaundy 再会"), "Vaundy%20%E5%86%8D%E4%BC%9A");
    }
}
