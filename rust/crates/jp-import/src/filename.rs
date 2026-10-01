//! 从文件名和目录结构里榨取 artist / title / 音轨号。
//!
//! 移植自 Python 侧的 `scraper/filename.py`，行为保持一致
//! （那边有 20 个测试钉着，这边照着建）。
//!
//! 为什么需要它：导入时最常见的失败不是「匹配错」，而是「压根没进来」。
//! 没有内嵌 tag 的文件光看 tag 是空的，但文件名几乎总是带着信息：
//!
//! ```text
//! 01 - YOASOBI - 夜に駆ける.flac
//! [01] 夜に駆ける.flac
//! 1-05 群青.flac              ← disc 1 track 5
//! 夜に駆ける - YOASOBI.m4a     ← 反过来写的
//! ```

use std::path::Path;

use unicode_normalization::UnicodeNormalization;

/// 一眼就知道不是歌手也不是曲名的目录名，用作歌手线索时要跳过。
const GENERIC_DIRS: &[&str] = &[
    "music", "musics", "audio", "songs", "song", "album", "albums", "disc", "disk", "cd", "cd1",
    "cd2", "tracks", "flac", "mp3", "downloads", "download", "media", "library", "itunes", "各種",
    "音楽", "音乐", "新建文件夹",
];

/// 文件名里常见的、和识别无关的噪声段。
const NOISE: &[&str] = &[
    "[flac]", "[mp3]", "[wav]", "[m4a]", "[aac]", "[alac]", "[hi-res]", "[hires]", "[web]", "[cd]",
    "[vinyl]", "[tak]", "[ape]",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilenameInfo {
    pub stem: String,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub artist: String,
    pub title: String,
    /// 怎么猜出来的。排查「这首歌为什么识别成这样」时有用。
    pub layout: &'static str,
}

/// Python 侧 `scraper.normalize._PUNCT_RE` 的字符表，逐字搬过来。
///
/// **不能用 `is_alphanumeric()` 代替。** 那样两个方向都会错：
/// 片假名长音符 `ー` 的 Unicode 类别是 Lm（修饰字母），`is_alphanumeric`
/// 判真会把它留下，而 Python 当标点去掉（「アルジャーノン」→「アルジャノン」）；
/// 反过来 `♪` `→` `☆` 这类不在表里、Python 会保留的符号，
/// `is_alphanumeric` 又会丢掉。
///
/// 拿真实曲库量过：294 个曲名 / 歌手 / 专辑 / 人名里有 42 个因此对不上。
const PUNCT: &str = concat!(
    "「」『』\"'’‘“”",
    "()[]{}（）［］〔〕【】〈〉《》",
    "-–—―ー_.,!?！？、。・:：;/／\\~〜=+*&#@｜|^%$…‥·",
);

/// feat 的各种写法。和 Python 的 `_FEAT_PATTERN` 对应。
///
/// Python 用的是零宽先行 `(?=\s|$)`，`regex` crate 不支持先行断言；
/// 这里改成把那个空白一起吃掉。后面反正要删掉所有空白，结果等价。
static FEAT_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)(?:\bfeat\.?(?:\s|$)|\bft\.?(?:\s|$)|\bfeaturing\b|\bwith\b|\bw/|ｆｅａｔ)",
    )
    .expect("feat 正则写死在代码里，编不过就是打字错了")
});

/// 比较用的键：NFKC → 折大小写 → 去 feat → 去标点 → 去所有空白。
///
/// 和 Python 侧 `matching_key` 逐步对齐。**NFKC 这一步不能省**：
/// 日语文件名里全角字母极常见（ＹＯＡＳＯＢＩ），不做兼容分解的话
/// 它和半角的 YOASOBI 是两个完全不同的字符串。
///
/// 去掉全部空白而不是折叠成一个，是因为日文曲名里空格的有无非常随意，
/// 「夜に駆ける」和「夜に 駆ける」必须等价。
///
/// 迁移期两边共用一个 `corpus.db`，这个键决定「是不是同一首歌 / 同一个人」。
/// 两边算得不一样，就会 Python 认为重复、Rust 认为是新的，库会被写脏。
pub fn matching_key(text: &str) -> String {
    // Python 用 casefold，Rust 只有 to_lowercase。差别仅在 ß→ss 这类
    // 特殊折叠上，日语曲库里不出现。
    let plain: String = text.nfkc().flat_map(char::to_lowercase).collect();
    let plain = FEAT_RE.replace_all(&plain, " ");
    let key: String = plain
        .chars()
        .filter(|c| !PUNCT.contains(*c) && !c.is_whitespace())
        .collect();
    if key.is_empty() {
        // 整首歌名就是标点的（きのこ帝国有一首叫「&」），剥完是空串，
        // 空串跟谁都能匹配上。这时退回只做全角/大小写归一。
        plain.chars().filter(|c| !c.is_whitespace()).collect()
    } else {
        key
    }
}

pub fn is_generic_directory(name: &str) -> bool {
    let key = matching_key(name);
    if key.is_empty() {
        return true;
    }
    if GENERIC_DIRS.iter().any(|g| matching_key(g) == key) {
        return true;
    }
    // 纯数字目录（年份、碟号）也不算歌手线索
    let trimmed = name.trim();
    !trimmed.is_empty() && trimmed.len() <= 4 && trimmed.chars().all(|c| c.is_ascii_digit())
}

fn strip_noise(text: &str) -> String {
    let mut out = text.to_string();
    for token in NOISE {
        // 大小写不敏感地去掉
        while let Some(pos) = out.to_lowercase().find(token) {
            out.replace_range(pos..pos + token.len(), " ");
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
        .trim_matches(|c: char| " -–—_.".contains(c))
        .to_string()
}

/// 剥掉开头的碟号/音轨号，返回 (剩余文本, track, disc)。
fn strip_track_prefix(stem: &str) -> (String, Option<u32>, Option<u32>) {
    let body = stem.trim();

    // 「1-05」「CD2 03」这类带碟号的
    if let Some((disc, track, rest)) = parse_disc_track(body) {
        // 「2019 - Album」会被误判成 disc=20 track=19，挡掉
        if disc <= 20 && track <= 199 {
            return (trim_separators(rest), Some(track), Some(disc));
        }
    }

    // 「[01]」「(01)」
    if let Some((n, rest)) = parse_bracketed(body) {
        return (trim_separators(rest), Some(n), None);
    }

    // 「01 」「01. 」「01-」
    if let Some((n, rest)) = parse_leading_number(body) {
        return (trim_separators(rest), Some(n), None);
    }

    (trim_separators(body), None, None)
}

fn trim_separators(text: &str) -> String {
    text.trim_matches(|c: char| " -–—_.[".contains(c)).to_string()
}

fn parse_disc_track(text: &str) -> Option<(u32, u32, &str)> {
    let lower = text.to_lowercase();
    let rest = ["cd", "disc", "disk"]
        .iter()
        .find_map(|p| lower.strip_prefix(p).map(|_| &text[p.len()..]))
        .unwrap_or(text)
        .trim_start();

    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || digits.len() > 2 {
        return None;
    }
    let after = &rest[digits.len()..];
    let sep = after.chars().next()?;
    if !"-_.".contains(sep) {
        return None;
    }
    let tail = &after[sep.len_utf8()..];
    let track_digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
    if track_digits.is_empty() || track_digits.len() > 3 {
        return None;
    }
    // 后面必须是分隔符或结束，否则「1-2-3」这种会被误吃
    let remainder = &tail[track_digits.len()..];
    if !remainder.is_empty() && !remainder.starts_with([' ', '.', '-', '_']) {
        return None;
    }
    Some((digits.parse().ok()?, track_digits.parse().ok()?, remainder))
}

fn parse_bracketed(text: &str) -> Option<(u32, &str)> {
    let (open, close) = match text.chars().next()? {
        '[' => ('[', ']'),
        '(' => ('(', ')'),
        '（' => ('（', '）'),
        '［' => ('［', '］'),
        _ => return None,
    };
    let _ = open;
    let end = text.find(close)?;
    let inner = text[open.len_utf8()..end].trim();
    if inner.is_empty() || inner.len() > 3 || !inner.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((inner.parse().ok()?, &text[end + close.len_utf8()..]))
}

fn parse_leading_number(text: &str) -> Option<(u32, &str)> {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    // 四位数是年份，三位以上的音轨号基本不存在
    if digits.is_empty() || digits.len() == 4 || digits.len() > 3 {
        return None;
    }
    let rest = &text[digits.len()..];
    // 后面必须跟分隔符，否则「2019」「Chu」这种会被误吃
    if !rest.starts_with([' ', '.', '-', '_', '－']) {
        return None;
    }
    let value: u32 = digits.parse().ok()?;
    if value > 199 {
        return None;
    }
    Some((value, rest))
}

/// 按分隔符切字段。要求两侧有空格或用全角连字符，
/// 否则「Chu-Ru-Ru」「K-POP」会被拦腰砍断。
fn split_fields(body: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let is_dash = matches!(c, '-' | '–' | '—' | '－');
        let spaced = is_dash
            && i > 0
            && chars[i - 1] == ' '
            && i + 1 < chars.len()
            && chars[i + 1] == ' ';
        if spaced {
            fields.push(current.trim().to_string());
            current.clear();
            i += 2; // 跳过 "- "
        } else {
            current.push(c);
            i += 1;
        }
    }
    fields.push(current.trim().to_string());
    fields
        .into_iter()
        .map(|f| strip_noise(&f))
        .filter(|f| !f.is_empty())
        .collect()
}

/// 解析不含扩展名的文件名。
///
/// `artist_hint` 通常来自父目录名或内嵌 tag。有它才能区分
/// 「Artist - Title」和「Title - Artist」——光看字符串是分不出来的。
pub fn parse_filename(stem: &str, artist_hint: &str) -> FilenameInfo {
    let raw = stem.trim();
    if raw.is_empty() {
        return FilenameInfo::default();
    }

    let (body, track, disc) = strip_track_prefix(raw);
    let fields = split_fields(&body);
    let base = FilenameInfo {
        stem: raw.to_string(),
        track_number: track,
        disc_number: disc,
        ..Default::default()
    };

    if fields.is_empty() {
        return FilenameInfo {
            title: strip_noise(&body),
            layout: "empty",
            ..base
        };
    }

    if fields.len() == 1 {
        let only = &fields[0];
        // 「01.flac」这种：剩下的唯一字段是纯数字，那是音轨号不是曲名。
        // 不挡住的话会拿字符串 "01" 去搜，必然匹配到一堆无关的东西。
        if !only.is_empty() && only.chars().all(|c| c.is_ascii_digit()) && only.len() <= 3 {
            return FilenameInfo {
                track_number: track.or_else(|| only.parse().ok()),
                layout: "track-number-only",
                ..base
            };
        }
        return FilenameInfo {
            title: only.clone(),
            layout: "title-only",
            ..base
        };
    }

    let hint = matching_key(artist_hint);
    let first = fields.first().cloned().unwrap_or_default();
    let last = fields.last().cloned().unwrap_or_default();

    if fields.len() == 2 {
        // 有歌手线索时以它为准；否则按最常见的 Artist - Title 处理
        if !hint.is_empty() && matching_key(&last) == hint && matching_key(&first) != hint {
            return FilenameInfo {
                artist: last,
                title: first,
                layout: "title-artist",
                ..base
            };
        }
        return FilenameInfo {
            artist: first,
            title: last,
            layout: "artist-title",
            ..base
        };
    }

    // 三段及以上：最常见的是 Artist - Album - Title，曲名在最后
    if !hint.is_empty() && matching_key(&last) == hint {
        return FilenameInfo {
            artist: last,
            title: first,
            layout: "title-first-artist-last",
            ..base
        };
    }
    FilenameInfo {
        artist: first,
        title: last,
        layout: "artist-first-title-last",
        ..base
    }
}

/// 从目录结构猜歌手和专辑。
///
/// 本项目自己的布局是 `raw/audio/{歌手}/{id}.flac`，父目录就是歌手。
/// 更常见的第三方布局是 `{歌手}/{专辑}/{曲目}`，此时父目录是专辑、
/// 祖父目录才是歌手。两种都覆盖。
pub fn folder_hints(path: &Path) -> (String, String) {
    let parent = path.parent();
    let parent_name = parent
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .filter(|n| !is_generic_directory(n))
        .unwrap_or("")
        .to_string();
    let grand_name = parent
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .filter(|n| !is_generic_directory(n))
        .unwrap_or("")
        .to_string();

    if !grand_name.is_empty() && !parent_name.is_empty() {
        (grand_name, parent_name) // 歌手/专辑/曲目
    } else {
        (parent_name, String::new()) // 歌手/曲目
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_track_number_with_dash() {
        let info = parse_filename("01 - YOASOBI - 夜に駆ける", "");
        assert_eq!(info.track_number, Some(1));
        assert_eq!(info.artist, "YOASOBI");
        assert_eq!(info.title, "夜に駆ける");
    }

    #[test]
    fn leading_number_with_dot() {
        let info = parse_filename("03. 群青", "");
        assert_eq!(info.track_number, Some(3));
        assert_eq!(info.title, "群青");
    }

    #[test]
    fn bracketed_number() {
        let info = parse_filename("[01] 夜に駆ける", "");
        assert_eq!(info.track_number, Some(1));
        assert_eq!(info.title, "夜に駆ける");
    }

    #[test]
    fn disc_and_track() {
        let info = parse_filename("1-05 群青", "");
        assert_eq!(info.disc_number, Some(1));
        assert_eq!(info.track_number, Some(5));
        assert_eq!(info.title, "群青");
    }

    #[test]
    fn a_year_is_not_a_track_number() {
        // 「2019 - Album」不能被读成 disc 20 / track 19
        let info = parse_filename("2019 - Album Name", "");
        assert_eq!(info.disc_number, None);
        assert_eq!(info.track_number, None);
    }

    #[test]
    fn artist_dash_title() {
        let info = parse_filename("YOASOBI - 夜に駆ける", "");
        assert_eq!(info.artist, "YOASOBI");
        assert_eq!(info.title, "夜に駆ける");
        assert_eq!(info.layout, "artist-title");
    }

    #[test]
    fn title_dash_artist_needs_a_hint() {
        // 光看字符串分不出正反，必须靠歌手线索
        let info = parse_filename("夜に駆ける - YOASOBI", "YOASOBI");
        assert_eq!(info.artist, "YOASOBI");
        assert_eq!(info.title, "夜に駆ける");
        assert_eq!(info.layout, "title-artist");
    }

    #[test]
    fn hint_matching_is_normalized() {
        // 线索用全角写的也要能对上
        let info = parse_filename("夜に駆ける - ＹＯＡＳＯＢＩ", "YOASOBI");
        assert_eq!(info.title, "夜に駆ける");
    }

    #[test]
    fn artist_album_title() {
        let info = parse_filename("YOASOBI - THE BOOK - 群青", "");
        assert_eq!(info.artist, "YOASOBI");
        assert_eq!(info.title, "群青");
    }

    #[test]
    fn japanese_filename_survives_intact() {
        let info = parse_filename("ずっと真夜中でいいのに。 - 秒針を噛む", "");
        assert_eq!(info.artist, "ずっと真夜中でいいのに。");
        assert_eq!(info.title, "秒針を噛む");
    }

    #[test]
    fn hyphenated_name_is_not_split() {
        // 分隔符要求两侧有空格，否则 Chu-Ru-Ru 会被砍成三段
        assert_eq!(parse_filename("Chu-Ru-Ru", "").title, "Chu-Ru-Ru");
    }

    #[test]
    fn quality_noise_is_dropped() {
        assert_eq!(parse_filename("YOASOBI - 群青 [FLAC]", "").title, "群青");
    }

    #[test]
    fn a_purely_numeric_name_is_a_track_number_not_a_title() {
        // 本项目自己的布局就是 raw/audio/{歌手}/{id}.flac
        let info = parse_filename("001", "");
        assert_eq!(info.track_number, Some(1));
        assert!(info.title.is_empty(), "纯数字不该当成曲名");
    }

    #[test]
    fn empty_stem() {
        assert_eq!(parse_filename("", ""), FilenameInfo::default());
    }

    #[test]
    fn generic_directories() {
        for name in ["Music", "CD1", "2019", "新建文件夹", ""] {
            assert!(is_generic_directory(name), "{name}");
        }
        for name in ["YOASOBI", "ずっと真夜中でいいのに。", "Mrs. GREEN APPLE"] {
            assert!(!is_generic_directory(name), "{name}");
        }
    }

    #[test]
    fn folder_hints_for_artist_slash_track() {
        let (artist, album) = folder_hints(Path::new("X:/raw/audio/YOASOBI/001.flac"));
        assert_eq!(artist, "YOASOBI");
        assert_eq!(album, "");
    }

    #[test]
    fn folder_hints_for_artist_slash_album_slash_track() {
        let (artist, album) = folder_hints(Path::new("X:/music/YOASOBI/THE BOOK/03 群青.flac"));
        assert_eq!(artist, "YOASOBI");
        assert_eq!(album, "THE BOOK");
    }

    #[test]
    fn generic_directory_is_not_an_artist_hint() {
        let (artist, _) = folder_hints(Path::new("X:/Music/群青.flac"));
        assert_eq!(artist, "");
    }

    // 下面这些期望值全部是拿 Python 的 `scraper.normalize.matching_key`
    // 实跑出来的，不是照着规范推的。两边共用一个库，这个键必须逐字对齐。

    #[test]
    fn matching_key_folds_case_and_punctuation() {
        assert_eq!(matching_key("YOASOBI"), "yoasobi");
        assert_eq!(matching_key("ＹＯＡＳＯＢＩ"), "yoasobi");
        assert_eq!(matching_key("Mrs. GREEN APPLE"), "mrsgreenapple");
        assert_eq!(matching_key("山口　一郎"), "山口一郎");
        assert_eq!(matching_key("夜に 駆ける"), "夜に駆ける");
    }

    #[test]
    fn the_katakana_long_vowel_mark_is_punctuation_not_a_letter() {
        // `ー` 的 Unicode 类别是 Lm，`is_alphanumeric()` 会判真，
        // 但 Python 把它当标点去掉。294 个真实曲名里 42 个栽在这里。
        assert_eq!(matching_key("アルジャーノン"), "アルジャノン");
        assert_eq!(matching_key("さよならはエモーション"), "さよならはエモション");
    }

    #[test]
    fn a_title_made_only_of_punctuation_falls_back() {
        // 剥完是空串的话，空串跟谁都匹配得上——退回只做归一。
        // Python 对这两个返回的就是原样。
        assert_eq!(matching_key("!?"), "!?");
        assert_eq!(matching_key("&"), "&"); // きのこ帝国真有这么一首
    }

    #[test]
    fn feat_markers_are_stripped() {
        assert_eq!(matching_key("A feat. B"), "ab");
        assert_eq!(matching_key("A ft B"), "ab");
        // 后面不跟空白就不算标记，"feat" 是曲名的一部分
        assert_eq!(matching_key("A feat.B"), "afeatb");
    }
}
