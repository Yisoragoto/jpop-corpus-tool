//! 同名同歌手的两个文件是不是**同一个版本**。
//!
//! 导入计划拿它决定：「疑似重复」（同一个录音换了个文件，交给人看、默认不导），
//! 还是「同名的另一个版本」（Live、Remix、翻唱、重制、TV size、伴奏……，照常作为一首新歌导入）。
//! **从不自动合并**：判成同一版本也只是不导、提示一句，不会把两个文件并到一首歌上。
//!
//! 依据，按硬到软：
//! 1. ISRC、MusicBrainz 录音编号：两边都有且不同 → 不同录音；
//! 2. 曲名和专辑里的版本标记（live / remix / カバー / remaster / tv size / instrumental …）不同 → 不同版本；
//! 3. 时长都知道且差超过 [`SAME_DURATION_SEC`] → 不同版本（剪辑版、加长版、带 MC 的现场）。
//!
//! 曲名本身带「(Live)」的，匹配键（`matching_key`）已经不同，根本走不到这里；
//! 这里主要管曲名一样、版本写在专辑上的（「LIVE TOUR 2019」「Remastered」「COVERS」）。

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;

/// 同一个录音换个压制 / 换个格式，时长差不过一两秒（编码器补的静音、帧对齐）
pub const SAME_DURATION_SEC: f64 = 3.0;

/// 拿来比的几项。
#[derive(Debug, Clone, Copy, Default)]
pub struct VersionFacts<'a> {
    pub title: &'a str,
    pub album: &'a str,
    pub duration_sec: Option<f64>,
    pub isrc: &'a str,
    pub recording_mbid: &'a str,
}

/// 版本标记：（显示名，ASCII 词（按词边界）, 日文片段（按子串））
const MARKERS: &[(&str, &[&str], &[&str])] = &[
    ("Live", &["live", "live ver", "live version", "unplugged"], &["ライブ", "ライヴ"]),
    ("Remix", &["remix", "rmx", "re-mix", "remixed"], &["リミックス"]),
    ("Acoustic", &["acoustic"], &["アコースティック"]),
    ("Instrumental", &["instrumental", "inst", "off vocal", "karaoke", "backing track"], &["インスト", "オフボーカル", "カラオケ"]),
    ("Cover", &["cover", "covers", "self cover"], &["カバー", "カヴァー"]),
    ("Remaster", &["remaster", "remastered", "remastering"], &["リマスター"]),
    ("TV size", &["tv size", "tv ver", "tv version", "tv edit", "anime size"], &["tvサイズ", "tvsize"]),
    ("Edit", &["radio edit", "single edit", "edit", "short ver", "short version"], &["ショートver", "ショートバージョン"]),
    ("Demo", &["demo"], &["デモ"]),
    ("Arrangement", &["orchestra", "orchestral", "symphonic", "piano ver", "piano version", "strings ver", "ballad ver"], &["オーケストラ", "ピアノver"]),
];

static ASCII: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    MARKERS
        .iter()
        .map(|(name, words, _)| {
            let alternatives = words.iter().map(|w| regex::escape(w).replace(' ', r"\s*")).collect::<Vec<_>>().join("|");
            (*name, Regex::new(&format!(r"(?:^|[^a-z0-9])(?:{alternatives})(?:$|[^a-z0-9])")).expect("版本标记正则"))
        })
        .collect()
});

fn fold(text: &str) -> String {
    text.nfkc().flat_map(char::to_lowercase).collect()
}

/// 曲名和专辑里出现的版本标记。
pub fn markers(title: &str, album: &str) -> BTreeSet<&'static str> {
    let text = fold(&format!("{title} / {album}"));
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let mut found = BTreeSet::new();
    for ((name, _, japanese), (_, ascii)) in MARKERS.iter().zip(ASCII.iter()) {
        if ascii.is_match(&text) || japanese.iter().any(|j| compact.contains(&fold(j))) {
            found.insert(*name);
        }
    }
    found
}

fn clean_code(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase()
}

/// 两边是不是不同的版本。是的话返回一句给人看的理由，同一个版本返回 None。
pub fn difference(existing: &VersionFacts, scanned: &VersionFacts) -> Option<String> {
    let (a, b) = (clean_code(existing.isrc), clean_code(scanned.isrc));
    if !a.is_empty() && !b.is_empty() && a != b {
        return Some(format!("ISRC 不同（{a} / {b}）"));
    }
    let (a, b) = (existing.recording_mbid.trim(), scanned.recording_mbid.trim());
    if !a.is_empty() && !b.is_empty() && !a.eq_ignore_ascii_case(b) {
        return Some("MusicBrainz 录音编号不同".into());
    }
    let (a, b) = (markers(existing.title, existing.album), markers(scanned.title, scanned.album));
    if a != b {
        let show = |set: &BTreeSet<&str>| if set.is_empty() { "原版".to_string() } else { set.iter().copied().collect::<Vec<_>>().join(" / ") };
        return Some(format!("版本不同：库里的是{}，这个是{}", show(&a), show(&b)));
    }
    if let (Some(a), Some(b)) = (existing.duration_sec, scanned.duration_sec)
        && a > 0.0
        && b > 0.0
        && (a - b).abs() > SAME_DURATION_SEC
    {
        return Some(format!("时长差 {:.0} 秒", (a - b).abs()));
    }
    None
}

/// 判成同一个版本时，依据是什么（给「疑似重复」配一句话）。
pub fn sameness(existing: &VersionFacts, scanned: &VersionFacts) -> String {
    let (a, b) = (clean_code(existing.isrc), clean_code(scanned.isrc));
    if !a.is_empty() && a == b {
        return "同一个 ISRC".into();
    }
    match (existing.duration_sec, scanned.duration_sec) {
        (Some(a), Some(b)) if a > 0.0 && b > 0.0 => format!("曲名、歌手相同，时长相差 {:.1} 秒", (a - b).abs()),
        _ => "曲名、歌手相同（没有时长可比）".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts<'a>(title: &'a str, album: &'a str, duration: Option<f64>) -> VersionFacts<'a> {
        VersionFacts { title, album, duration_sec: duration, ..Default::default() }
    }

    #[test]
    fn version_markers_come_from_title_and_album() {
        assert!(markers("夜に駆ける", "THE BOOK").is_empty());
        assert_eq!(markers("夜に駆ける", "YOASOBI LIVE 2023").into_iter().collect::<Vec<_>>(), ["Live"]);
        assert_eq!(markers("Lemon (Remastered 2021)", "").into_iter().collect::<Vec<_>>(), ["Remaster"]);
        assert_eq!(markers("残酷な天使のテーゼ", "ＣＯＶＥＲＳ").into_iter().collect::<Vec<_>>(), ["Cover"]);
        assert_eq!(markers("紅蓮華 (TV size)", "").into_iter().collect::<Vec<_>>(), ["TV size"]);
        assert_eq!(markers("曲", "セルフカバー集").into_iter().collect::<Vec<_>>(), ["Cover"]);
        assert_eq!(markers("Shout (off vocal)", "").into_iter().collect::<Vec<_>>(), ["Instrumental"]);
        // 词中间的不算：Alive / Olive / Edition / Democracy
        assert!(markers("Alive", "Olive Deluxe Edition").is_empty());
        assert!(markers("Democracy", "").is_empty());
    }

    #[test]
    fn live_cover_remaster_and_edits_are_never_the_same_version() {
        let studio = facts("夜に駆ける", "THE BOOK", Some(261.0));
        assert!(difference(&studio, &facts("夜に駆ける", "THE BOOK", Some(260.2))).is_none(), "换个压制还是同一个");
        assert!(difference(&studio, &facts("夜に駆ける", "LIVE 2023", Some(261.0))).unwrap().contains("Live"));
        assert!(difference(&studio, &facts("夜に駆ける", "THE BOOK (Remastered)", Some(261.0))).unwrap().contains("Remaster"));
        assert!(difference(&studio, &facts("夜に駆ける", "THE BOOK", Some(240.0))).unwrap().contains("时长差 21 秒"));
        assert!(difference(&studio, &facts("夜に駆ける", "THE BOOK", None)).is_none(), "没时长时不靠时长区分");
    }

    #[test]
    fn external_ids_decide_first() {
        let a = VersionFacts { isrc: "JP-XX1-23-00001", ..facts("曲", "", Some(200.0)) };
        let same = VersionFacts { isrc: "jpxx12300001", ..facts("曲", "", Some(200.0)) };
        let other = VersionFacts { isrc: "JPXX12300002", ..facts("曲", "", Some(200.0)) };
        assert!(difference(&a, &same).is_none());
        assert_eq!(sameness(&a, &same), "同一个 ISRC");
        assert!(difference(&a, &other).unwrap().starts_with("ISRC 不同"));
        let m1 = VersionFacts { recording_mbid: "aaa", ..facts("曲", "", None) };
        let m2 = VersionFacts { recording_mbid: "bbb", ..facts("曲", "", None) };
        assert!(difference(&m1, &m2).is_some());
    }
}
