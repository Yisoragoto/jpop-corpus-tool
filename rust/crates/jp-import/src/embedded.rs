//! 音频文件里内嵌的歌词和封面。
//!
//! 很多人的曲库是音乐 App 或 Mp3tag / foobar2000 整理过的：歌词和封面写进了文件本身，
//! 旁边没有 .lrc，也没有单独的图片（[#5](https://github.com/Yisoragoto/jpop-corpus-tool/issues/5)）。
//! 以前导入只认外挂 .lrc，这些歌进库就是一行歌词、一张图都没有，只能再去网上搜。
//!
//! 各格式把歌词放在哪儿（lofty 统一成两个键）：
//!
//! | 格式 | 字段 | lofty 的键 |
//! |---|---|---|
//! | FLAC / Ogg（Vorbis comment） | `LYRICS` / `UNSYNCEDLYRICS` | `Lyrics` / `UnsyncLyrics` |
//! | MP3（ID3v2） | `USLT` | `UnsyncLyrics` |
//! | M4A（MP4 ilst） | `©lyr` | `Lyrics` |
//!
//! **键名不等于有没有时间轴**：ID3v2 的 USLT 名义上是「不同步歌词」，
//! 但网易云、Mp3tag 往里写的常常就是整份带时间戳的 LRC。所以两个键都读，
//! 有没有时间轴按内容判断（见 [`EmbeddedLyrics::synced`]）。
//!
//! **字段里装的也不一定是 LRC**：别的播放器会往里写 QRC、YRC、ESLyric、TTML
//! （[#61](https://github.com/Yisoragoto/jpop-corpus-tool/issues/61)）。没有扩展名可看，
//! 所以交给 [`jp_lyric_format::decode`] 按内容判断格式。
//!
//! ID3v2 的 `SYLT`（二进制的同步歌词）lofty 不转成文本，这里不读——实际很少见。

use std::path::Path;

use lofty::config::ParseOptions;
use lofty::file::{TaggedFile, TaggedFileExt};
use lofty::picture::{MimeType, PictureType};
use lofty::probe::Probe;
use lofty::tag::ItemKey;

/// 文件里内嵌的一份歌词。
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddedLyrics {
    /// 原文，原样保留（只统一了换行）。登记来源、存 `source_raw` 用的是它
    pub text: String,
    /// 按内容判出的格式（`raw_format` 那个词：lrc / enhanced_lrc / qrc / ttml …）
    pub format: &'static str,
    /// 入库用的 LRC 文本：LRC 一家就是 `text`，逐字格式转换得到
    pub lrc: String,
    /// 解析出来的行里至少有一行带时间戳
    pub synced: bool,
}

/// 从已经读开的文件里取歌词。主 tag 优先，其次其余的 tag（MP3 可能同时有 ID3v2 和 APE）。
///
/// **解析出 0 行的不算**：只有 `[ti:…]` 这类标签、或者只有作词作曲署名的，
/// 当作没有，让外挂 .lrc 和在线搜还有机会。
pub fn lyrics_from(tagged: &TaggedFile) -> Option<EmbeddedLyrics> {
    let primary = tagged.primary_tag();
    let rest = tagged.tags().iter().filter(|t| primary.is_none_or(|p| !std::ptr::eq(*t, p)));
    for tag in primary.into_iter().chain(rest) {
        for key in [ItemKey::Lyrics, ItemKey::UnsyncLyrics] {
            let Some(text) = tag.get_string(key) else { continue };
            if let Some(found) = judge(text) {
                return Some(found);
            }
        }
    }
    None
}

fn judge(text: &str) -> Option<EmbeddedLyrics> {
    // ID3v2 和 MP4 里换行常常是单个 \r（老 iTunes 的写法），`str::lines` 不认它
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    if text.trim().is_empty() {
        return None;
    }
    let decoded = decode_text(&text);
    if decoded.parsed.lines.is_empty() {
        return None;
    }
    let synced = decoded.parsed.lines.iter().any(|l| l.time_sec.is_some());
    Some(EmbeddedLyrics { format: decoded.format.name(), lrc: decoded.lrc, text, synced })
}

/// 内嵌歌词的文本 → 解析结果。没有扩展名可看，按内容判断格式。
///
/// 看着像逐字格式、却解析失败的（坏掉的 TTML、时间写错的 QRC），退回按 LRC 读：
/// 和以前一样至少把字留下来，不让一首歌因为格式判断而整个没了歌词。
pub fn decode_text(text: &str) -> jp_lyric_format::Decoded {
    jp_lyric_format::decode_str(text, None).unwrap_or_else(|_| {
        jp_lyric_format::decode_as(text, jp_lyric_format::lrc_flavor(text)).expect("LRC parsing never fails")
    })
}

/// 读一个文件里内嵌的歌词。读不开、没有都是 None。
///
/// 导入时歌词跟着扫描一起读了（见 `scan::scan_file`）；这个入口给「补齐缺失歌词」用：
/// 库里已经有、但当初导入时还不认内嵌歌词的歌。
pub fn read_lyrics(audio: &Path) -> Option<EmbeddedLyrics> {
    let tagged = Probe::open(audio)
        .and_then(|p| p.options(ParseOptions::new().read_cover_art(false)).read())
        .ok()?;
    lyrics_from(&tagged)
}

/// 文件里内嵌的一张封面。
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddedCover {
    pub data: Vec<u8>,
    /// `image/jpeg` / `image/png`，文件里没写就是 None
    pub mime: Option<String>,
}

/// 读一个文件里内嵌的封面。
///
/// 有好几张时（封面、封底、歌手照……）取标成「正面封面」的那张，没有标的就取第一张——
/// 很多工具写进去的唯一一张图类型是 `Other`，M4A 的 `covr` 则根本不记类型
/// （iTunes 的规矩是第一张就是封面）。空图不算。
pub fn read_cover(audio: &Path) -> Option<EmbeddedCover> {
    let tagged = Probe::open(audio)
        .and_then(|p| p.options(ParseOptions::new()).read())
        .ok()?;
    let pictures: Vec<_> = tagged
        .tags()
        .iter()
        .flat_map(|t| t.pictures())
        .filter(|p| !p.data().is_empty())
        .collect();
    let picture = pictures
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())?;
    Some(EmbeddedCover {
        data: picture.data().to_vec(),
        mime: picture.mime_type().map(MimeType::as_str).map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_decide_synced_not_the_key_name() {
        let synced = judge("[00:01.00]一行目\n[00:05.50]二行目").unwrap();
        assert!(synced.synced);
        let plain = judge("一行目\n二行目").unwrap();
        assert!(!plain.synced);
    }

    #[test]
    fn word_timed_formats_in_the_lyrics_tag_are_recognised() {
        // 老 iTunes 式的单个 \r 换行，和 #61 里内嵌 QRC 一样带着 [ti:] 头和 [kana:] 注音行
        let found = judge("[ti:夜]\r[kana:1よ]\r[0,900]夜(0,500)が(500,400)\r[1000,500]朝(1000,500)").unwrap();
        assert_eq!(found.format, "qrc");
        assert!(found.synced);
        let parsed = crate::lrc::parse(&found.lrc);
        assert_eq!(parsed.lines.len(), 2, "{}", found.lrc);
        assert_eq!(parsed.lines[0].text, "夜が");
        assert_eq!(parsed.lines[0].words.len(), 2);
        // 原文原样留着（只统一了换行），登记来源时存的是它
        assert!(found.text.starts_with("[ti:夜]\n[kana:1よ]\n[0,900]"));

        let found = judge("[0,900](0,500,0)夜(500,400,0)が").unwrap();
        assert_eq!(found.format, "yrc");
        let found = judge("[00:01.00]夜[00:01.50]が[00:02.00]").unwrap();
        assert_eq!(found.format, "enhanced_lrc");
        assert_eq!(found.lrc, found.text);
    }

    #[test]
    fn a_broken_word_timed_tag_falls_back_to_lrc_instead_of_disappearing() {
        let found = judge("[0,900]夜(0,abc)\n[00:01.00]朝").unwrap();
        assert_eq!(found.format, "lrc");
        assert!(found.lrc.contains("朝"));
    }

    #[test]
    fn a_lone_carriage_return_still_splits_lines() {
        let found = judge("[00:01.00]一行目\r[00:05.50]二行目").unwrap();
        assert_eq!(crate::lrc::parse(&found.text).lines.len(), 2);
    }

    #[test]
    fn tags_or_blank_text_count_as_no_lyrics() {
        assert_eq!(judge(""), None);
        assert_eq!(judge("  \n "), None);
        assert_eq!(judge("[ti:曲名]\n[ar:歌手]"), None);
    }

    #[test]
    fn a_missing_file_has_nothing_embedded() {
        assert_eq!(read_lyrics(Path::new("完全不存在.flac")), None);
        assert_eq!(read_cover(Path::new("完全不存在.flac")), None);
    }
}
