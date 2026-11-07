//! 音频旁边同名的外挂歌词：挑哪一份当原文，其余的当备选、译文。
//!
//! 纯函数：只看文件名列表，不碰磁盘。调用方自己列目录，再把结果映射回路径。
//!
//! 同一个主名下按这个顺序挑第一份：
//! `.ttml` > `.qrc` / `.yrc` / `.krc` / `.lys` > `.lrc` > `.srt` > `.txt`。
//! 越靠前的时间精度越高（TTML 和逐字格式带字级时间，LRC 多半只到行）。
//! 带语言后缀的（`song.zh-Hans.lrc`）是译文；日语后缀（`ja`、`jpn`）算原文，排在不带后缀的后面。

/// 认得的扩展名，按优先级排
pub const EXTENSIONS: [&str; 8] = ["ttml", "qrc", "yrc", "krc", "lys", "lrc", "srt", "txt"];

/// 认得的扩展名和它们的优先级（越小越优先）
const RANKS: &[(&str, u8)] = &[
    ("ttml", 0),
    ("qrc", 1),
    ("yrc", 1),
    ("krc", 1),
    ("lys", 1),
    ("lrc", 2),
    ("srt", 3),
    ("txt", 4),
];

/// 一个外挂歌词文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalFile {
    /// 文件名（含扩展名），原样
    pub name: String,
    /// 扩展名，小写。也是交给 [`crate::decode`] 的提示
    pub extension: String,
    /// 文件名里的语言后缀（`zh-Hans`），没有就是空串
    pub language: String,
    /// 是不是译文（带语言后缀、且不是日语）
    pub translation: bool,
}

/// 挑选结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalLyrics {
    /// 当原文用的那一份
    pub primary: Option<ExternalFile>,
    /// 其余的原文（格式优先级低的、带日语后缀的），按优先级排
    pub alternatives: Vec<ExternalFile>,
    /// 译文，按文件名排
    pub translations: Vec<ExternalFile>,
}

impl ExternalLyrics {
    /// 全部文件：原文、备选、译文
    pub fn all(&self) -> impl Iterator<Item = &ExternalFile> {
        self.primary.iter().chain(&self.alternatives).chain(&self.translations)
    }
}

/// 扩展名是不是外挂歌词认的那几种（不分大小写）
pub fn is_lyric_extension(extension: &str) -> bool {
    rank(&extension.to_ascii_lowercase()).is_some()
}

fn rank(extension: &str) -> Option<u8> {
    RANKS.iter().find(|(e, _)| *e == extension).map(|(_, r)| *r)
}

/// 语言标记：`zh`、`zh-Hans`、`pt-BR`、`ja-Latn` 这一类。
pub fn is_language_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let Some(primary) = parts.next() else { return false };
    (2..=3).contains(&primary.len())
        && primary.chars().all(|c| c.is_ascii_alphabetic())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// 从一个目录的文件名里挑出 `stem` 的外挂歌词。`stem` 是音频文件名去掉扩展名。
///
/// 主名要完全一致（区分大小写，和文件系统看到的一样）；扩展名不分大小写。
pub fn find_external<'a>(stem: &str, names: impl IntoIterator<Item = &'a str>) -> ExternalLyrics {
    let mut originals: Vec<(u8, bool, ExternalFile)> = Vec::new();
    let mut translations = Vec::new();
    for name in names {
        let Some((base, extension)) = name.rsplit_once('.') else { continue };
        let extension = extension.to_ascii_lowercase();
        let Some(rank) = rank(&extension) else { continue };
        let language = if base == stem {
            String::new()
        } else if let Some(tag) = base.strip_prefix(stem).and_then(|r| r.strip_prefix('.')) {
            if !is_language_tag(tag) {
                continue;
            }
            tag.to_string()
        } else {
            continue;
        };
        let primary_tag = language.split('-').next().unwrap_or("").to_ascii_lowercase();
        let translation = !language.is_empty() && primary_tag != "ja" && primary_tag != "jpn";
        let file = ExternalFile { name: name.to_string(), extension, language, translation };
        if translation {
            translations.push(file);
        } else {
            originals.push((rank, !file.language.is_empty(), file));
        }
    }
    originals.sort_by(|a, b| (a.0, a.1, &a.2.name).cmp(&(b.0, b.1, &b.2.name)));
    translations.sort_by(|a: &ExternalFile, b: &ExternalFile| a.name.cmp(&b.name));
    let mut originals = originals.into_iter().map(|(_, _, f)| f);
    ExternalLyrics { primary: originals.next(), alternatives: originals.collect(), translations }
}

/// 音频旁边当原文用的那一份外挂歌词（[`find_external`] 的 `primary`），找不到是 None。
///
/// 只探各扩展名的同名文件，不整个列目录：导入时每首歌都要找一次，
/// 一个文件夹几百首歌时每首都列一遍目录太慢。所以带语言后缀的文件这里看不到——它们只会是译文或备选。
pub fn primary_beside(audio: &std::path::Path) -> Option<std::path::PathBuf> {
    let stem = audio.file_stem()?.to_str()?;
    let present: Vec<String> = EXTENSIONS
        .iter()
        .map(|ext| audio.with_extension(ext))
        .filter(|p| p.is_file())
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_owned))
        .collect();
    let found = find_external(stem, present.iter().map(String::as_str)).primary?;
    Some(audio.with_file_name(found.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(found: &ExternalLyrics) -> (Option<&str>, Vec<&str>, Vec<&str>) {
        (
            found.primary.as_ref().map(|f| f.name.as_str()),
            found.alternatives.iter().map(|f| f.name.as_str()).collect(),
            found.translations.iter().map(|f| f.name.as_str()).collect(),
        )
    }

    #[test]
    fn the_most_precise_format_wins_and_the_rest_are_alternatives() {
        let listing = ["夜.flac", "夜.txt", "夜.srt", "夜.lrc", "夜.QRC", "夜.ttml", "夜.zh.lrc", "夜明け.lrc", "cover.jpg"];
        let found = find_external("夜", listing);
        assert_eq!(names(&found), (Some("夜.ttml"), vec!["夜.QRC", "夜.lrc", "夜.srt", "夜.txt"], vec!["夜.zh.lrc"]));
        assert_eq!(found.primary.unwrap().extension, "ttml");
        assert!(found.translations[0].translation);
        assert_eq!(found.translations[0].language, "zh");
    }

    #[test]
    fn each_tier_is_ranked_in_order() {
        for (listing, want) in [
            (vec!["a.txt", "a.srt"], "a.srt"),
            (vec!["a.txt", "a.srt", "a.lrc"], "a.lrc"),
            (vec!["a.lrc", "a.yrc"], "a.yrc"),
            (vec!["a.lrc", "a.lys"], "a.lys"),
            (vec!["a.krc", "a.ttml"], "a.ttml"),
        ] {
            assert_eq!(find_external("a", listing).primary.unwrap().name, want);
        }
    }

    #[test]
    fn a_japanese_suffix_is_an_original_after_the_plain_name() {
        let found = find_external("a", ["a.ja.lrc", "a.lrc", "a.zh-Hans.srt", "a.notes.txt"]);
        assert_eq!(names(&found), (Some("a.lrc"), vec!["a.ja.lrc"], vec!["a.zh-Hans.srt"]));
    }

    #[test]
    fn nothing_matching_gives_nothing() {
        assert_eq!(find_external("a", ["b.lrc", "a.mp3", "a"]), ExternalLyrics::default());
    }
}
