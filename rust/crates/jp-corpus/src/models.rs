//! 数据模型。
//!
//! 全部 `Serialize`，Tauri command 可以直接返回，不用再包一层 DTO。
//! 字段名用 camelCase 输出，前端 TypeScript 那边就是原生形状。

use serde::{Deserialize, Serialize};

// ────────────────────────────── 曲库 ──────────────────────────────

/// 一首曲目。对应 `songs` 表。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub title: String,
    /// 原始歌手串。合作曲是「A/B」这种形态——结构化的版本在 `credits` 里。
    pub artist: String,
    pub album: String,
    pub album_id: Option<i64>,
    pub year: String,
    pub genre: String,
    pub audio_path: String,
    pub cover_path: String,
    pub duration_sec: Option<f64>,
    /// 歌词行数。0 表示还没有歌词，UI 要据此降级而不是当成错误。
    pub line_count: i64,
}

/// 一张专辑。对应 `albums` 表。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub id: i64,
    pub title: String,
    pub album_artist: String,
    pub year: String,
    pub artwork_path: String,
    pub track_count: i64,
    pub total_duration_sec: f64,
}

/// 一个人（歌手 / 作词 / 作曲 / 编曲 统一实体）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub normalized_name: String,
}

/// 某人在某个角色下的作品数。Library 的 Composers / Lyricists 列表。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonSummary {
    pub id: i64,
    pub name: String,
    pub role: String,
    pub track_count: i64,
    /// 歌手照片（`artists.image_path`）。只有刮削过的演唱者才有；
    /// 作词、作曲这类人从来没去查过，是空串。
    #[serde(default)]
    pub image_path: String,
}

/// 一条曲目信用。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credit {
    pub person_id: i64,
    pub name: String,
    pub role: String,
    pub position: i64,
    /// lrc / library / manual / provider——重跑回填时不覆盖 manual
    pub source: String,
}

/// Collaboration Graph 的一条边。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collaborator {
    pub person_id: i64,
    pub name: String,
    pub shared_tracks: i64,
    pub roles: Vec<String>,
    /// 同 `PersonSummary::image_path`。点合作者会切到他，头像得跟过去。
    #[serde(default)]
    pub image_path: String,
}

/// 一行歌词里的一个词。Research Mode 里每个都是可点击的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineToken {
    pub surface: String,
    pub lemma: String,
    /// UPOS。空串表示分词时没定出词性。
    pub pos: String,
}

/// 一行歌词。`tokens` 为空表示这行还没分词，UI 要降级成纯文本而不是报错。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    pub utterance_id: i64,
    pub line_idx: i64,
    /// 秒。None 表示这份 LRC 没有时间轴——不能假设所有歌词都能跟随播放。
    pub time_sec: Option<f64>,
    pub text: String,
    pub tokens: Vec<LineToken>,
}

// ────────────────────────────── 检索 ──────────────────────────────

/// KWIC 检索的匹配字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchField {
    /// 表层形：搜「駆ける」只命中「駆ける」
    Surface,
    /// 词元：搜「駆ける」连「駆け」「駆けた」一起命中
    Lemma,
}

impl MatchField {
    pub(crate) fn column(self) -> &'static str {
        match self {
            MatchField::Surface => "surface",
            MatchField::Lemma => "lemma",
        }
    }
}

/// KWIC 查询条件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct KwicQuery {
    pub keywords: Vec<String>,
    pub field: MatchField,
    /// UPOS 过滤，空表示不限
    pub pos: Option<String>,
    /// 按人筛选。用 `people.id` 而不是歌手字符串——
    /// 旧实现是 `'/'||artist||'/' LIKE '%/'||?||'/%'`，走不了索引，
    /// 合作曲还得靠拼字符串。
    pub person_ids: Vec<i64>,
    /// 命中行的上下各取一行，拼进左右语境
    pub cross_line: bool,
    /// 同一首歌里重复出现的相同歌词行折叠成一条
    pub dedup: bool,
    /// 只留关键词里有假名或汉字的命中（Python `_JP_RE.search(match)`），先筛再截断到 `limit`
    pub jp_only: bool,
    pub limit: Option<i64>,
}

impl Default for KwicQuery {
    fn default() -> Self {
        Self {
            keywords: Vec::new(),
            field: MatchField::Surface,
            pos: None,
            person_ids: Vec::new(),
            cross_line: false,
            dedup: true,
            jp_only: false,
            limit: None,
        }
    }
}

/// 一条 KWIC 命中。左 / 关键词 / 右 三段，前端直接渲染。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KwicHit {
    pub song_id: String,
    pub artist: String,
    pub title: String,
    pub audio_path: String,
    pub utterance_id: i64,
    pub line_idx: i64,
    /// 秒。可能为 None——不是所有 LRC 都有时间轴，UI 要优雅降级。
    pub time_sec: Option<f64>,
    pub text: String,
    pub left: String,
    pub keyword: String,
    pub right: String,
    /// 折叠掉的重复次数，dedup 关闭时恒为 1
    pub repeat_count: i64,
    /// 这一行做过分词校正（`token_corrections` 里有它）。KWIC 里标 ✏。
    #[serde(default)]
    pub corrected: bool,
}

/// 全文检索命中（走 FTS5）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricHit {
    pub song_id: String,
    pub artist: String,
    pub title: String,
    pub utterance_id: i64,
    pub time_sec: Option<f64>,
    pub text: String,
    /// FTS5 的 bm25 排序分。越小越相关（SQLite 的约定）。
    pub rank: f64,
}

// ────────────────────────────── 语料 ──────────────────────────────

/// Corpus Overview 卡片。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub tracks: i64,
    pub albums: i64,
    pub people: i64,
    pub performers: i64,
    pub composers: i64,
    pub lyricists: i64,
    pub arrangers: i64,
    pub lyric_lines: i64,
    pub tokens: i64,
    pub vocabulary: i64,
    pub total_duration_sec: f64,
}

/// Analytics 页的 Timeline 一格。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YearStats {
    pub year: String,
    pub track_count: i64,
    pub album_count: i64,
    pub vocabulary: i64,
}

/// 词频表的一行。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordFrequency {
    pub lemma: String,
    pub pos: String,
    pub freq: i64,
    pub song_count: i64,
    /// 该词元下出现过的表层形，按频次降序
    pub surfaces: Vec<String>,
}

/// Research Mode 的核心：点一个词能看到什么。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordInCorpus {
    pub lemma: String,
    pub occurrences: i64,
    pub song_count: i64,
    pub artist_count: i64,
    /// 首次出现的年份（按曲目年份），语料里没有年份时为 None
    pub first_year: Option<String>,
    pub last_year: Option<String>,
    pub pos_distribution: Vec<PosCount>,
    /// 跨歌曲例句，当前歌曲的排在最前
    pub examples: Vec<WordExample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PosCount {
    pub pos: String,
    pub count: i64,
}

/// 一条例句。点它就能跳到那首歌的那个时间点。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordExample {
    pub song_id: String,
    pub artist: String,
    pub title: String,
    pub utterance_id: i64,
    pub time_sec: Option<f64>,
    pub text: String,
}

// ────────────────────────────── 播放历史 ──────────────────────────────

/// 一次播放记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlayEvent {
    pub song_id: String,
    /// 实际听了多久，用来区分「真的听完了」和「点开就切走」
    pub listened_sec: f64,
    pub position_sec: f64,
    pub completed: bool,
    /// library / search / research——用于分析研究工作流
    pub source: String,
}

impl Default for PlayEvent {
    fn default() -> Self {
        Self {
            song_id: String::new(),
            listened_sec: 0.0,
            position_sec: 0.0,
            completed: false,
            source: String::new(),
        }
    }
}

/// 最近播放 / 播放排行的一行。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayedTrack {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover_path: String,
    pub play_count: i64,
    pub last_played: String,
    pub total_listened_sec: f64,
}

/// 按流派 / 年代分组的统计。Home 页的 Genres / Decades 板块。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FacetCount {
    /// 分组值。年代是「2010s」这种，流派是原始字符串。
    pub label: String,
    pub track_count: i64,
    pub total_duration_sec: f64,
}

// ────────────────────────────── 全局搜索 ──────────────────────────────

/// 全局搜索的一条结果。
///
/// 用带标签的枚举而不是「一个宽表 + kind 字段」，是因为前端要按类型
/// 分组显示、点击行为也各不相同；宽表会让每种类型都带一堆用不上的空字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum QuickHit {
    #[serde(rename_all = "camelCase")]
    Track {
        song_id: String,
        title: String,
        artist: String,
        album: String,
        duration_sec: Option<f64>,
    },
    #[serde(rename_all = "camelCase")]
    Album {
        album_id: i64,
        title: String,
        album_artist: String,
        track_count: i64,
    },
    #[serde(rename_all = "camelCase")]
    Person {
        person_id: i64,
        name: String,
        /// 这个人担任过的角色，按作品数排序
        roles: Vec<String>,
        track_count: i64,
    },
    #[serde(rename_all = "camelCase")]
    Lyric {
        song_id: String,
        title: String,
        artist: String,
        utterance_id: i64,
        time_sec: Option<f64>,
        text: String,
    },
    /// 语料里的一个词。点开进 Research 视图。
    #[serde(rename_all = "camelCase")]
    Word {
        lemma: String,
        pos: String,
        freq: i64,
        song_count: i64,
    },
}

/// 全局搜索结果，按类型分组。
///
/// 分组而不是混在一起排序：跨类型的相关性分数没有可比性
/// （曲名匹配的 0.9 和歌词 bm25 的 -8.3 不是一个量纲），
/// 硬排出来的顺序是假的。分组让用户自己按类型找。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickSearchResults {
    pub tracks: Vec<QuickHit>,
    pub albums: Vec<QuickHit>,
    pub people: Vec<QuickHit>,
    pub words: Vec<QuickHit>,
    pub lyrics: Vec<QuickHit>,
}

impl QuickSearchResults {
    pub fn total(&self) -> usize {
        self.tracks.len()
            + self.albums.len()
            + self.people.len()
            + self.words.len()
            + self.lyrics.len()
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}
