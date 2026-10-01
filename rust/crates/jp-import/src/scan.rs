//! 扫描目录，读出每个音频文件「看起来是什么」。
//!
//! **只读取，不判断。** 「这首歌该不该导入」「和库里哪首重复」属于
//! `plan` 的职责——分开是为了让扫描能在没有数据库的情况下测试。
//!
//! 内嵌 tag 优先，读不到就退到文件名和目录结构。Python 版的
//! `_read_tags` 只读 tag，读不到就把整个文件丢掉；这里不丢。

use std::path::{Path, PathBuf};

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey};
use serde::Serialize;

use crate::filename::{folder_hints, parse_filename};

/// 认得的音频扩展名。和 `jp-audio` 能解码的格式保持一致。
pub const AUDIO_EXTS: &[&str] = &["flac", "mp3", "wav", "m4a", "ogg", "opus", "aac", "wma"];

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// 扫到的一个文件。原值和推断值都保留——
/// 需求「永远不要覆盖原始 metadata」从扫描这一步就开始。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedTrack {
    pub path: String,
    pub file_name: String,
    // ── 内嵌 tag 的原值 ──
    pub tag_title: String,
    pub tag_artist: String,
    pub tag_album: String,
    pub tag_album_artist: String,
    pub tag_year: String,
    pub tag_genre: String,
    // ── 从文件名/目录推断的 ──
    pub guess_title: String,
    pub guess_artist: String,
    pub folder_artist: String,
    pub folder_album: String,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub duration_sec: Option<f64>,
    pub size: Option<u64>,
    /// 旁边找到的 .lrc 文件
    pub lyrics_path: Option<String>,
    /// 读 tag 时出的问题。为空表示一切正常。
    pub warning: Option<String>,
}

impl ScannedTrack {
    /// 有效曲名：tag 优先，其次文件名。
    pub fn title(&self) -> &str {
        if !self.tag_title.trim().is_empty() {
            &self.tag_title
        } else {
            &self.guess_title
        }
    }

    /// 有效歌手：tag → albumartist → 文件名 → 目录。
    pub fn artist(&self) -> &str {
        for candidate in [
            &self.tag_artist,
            &self.tag_album_artist,
            &self.guess_artist,
            &self.folder_artist,
        ] {
            if !candidate.trim().is_empty() {
                return candidate;
            }
        }
        ""
    }

    pub fn album(&self) -> &str {
        if !self.tag_album.trim().is_empty() {
            &self.tag_album
        } else {
            &self.folder_album
        }
    }

    /// 够不够导入。曲名是底线——没有曲名的条目进了库也没法用。
    pub fn has_identity(&self) -> bool {
        !self.title().trim().is_empty()
    }
}

/// 读一个文件。**任何读取失败都归到 `warning`，不抛出去**——
/// 一个坏文件不该中断整轮扫描。
pub fn scan_file(path: &Path) -> ScannedTrack {
    let mut track = ScannedTrack {
        path: platform_path(path),
        file_name: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string(),
        size: std::fs::metadata(path).ok().map(|m| m.len()),
        lyrics_path: find_lyrics(path).map(|p| platform_path(&p)),
        ..Default::default()
    };

    match Probe::open(path).and_then(|p| p.options(ParseOptions::new()).read()) {
        Ok(tagged) => {
            track.duration_sec = Some(tagged.properties().duration().as_secs_f64())
                .filter(|d| *d > 0.0);
            if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
                track.tag_title = tag.title().unwrap_or_default().to_string();
                track.tag_artist = tag.artist().unwrap_or_default().to_string();
                track.tag_album = tag.album().unwrap_or_default().to_string();
                track.tag_genre = tag.genre().unwrap_or_default().to_string();
                track.tag_album_artist = tag
                    .get_string(ItemKey::AlbumArtist)
                    .unwrap_or_default()
                    .to_string();
                // 年份取 date 的年那一段。有的文件写的是完整日期。
                track.tag_year = tag
                    .get_string(ItemKey::RecordingDate)
                    .or_else(|| tag.get_string(ItemKey::Year))
                    .map(|d| d.chars().take(4).collect::<String>())
                    .filter(|y| y.chars().all(|c| c.is_ascii_digit()) && y.len() == 4)
                    .unwrap_or_default();
                track.track_number = tag.track();
                track.disc_number = tag.disk();
            }
        }
        Err(err) => {
            // 读不出 tag 不等于文件没用——文件名往往就够了
            track.warning = Some(format!("读不出元数据：{err}"));
        }
    }

    let (folder_artist, folder_album) = folder_hints(path);
    track.folder_artist = folder_artist;
    track.folder_album = folder_album;

    // 歌手线索优先信 tag，其次信目录。这决定了「A - B」到底是
    // Artist - Title 还是 Title - Artist。
    let hint = if !track.tag_artist.is_empty() {
        track.tag_artist.clone()
    } else if !track.tag_album_artist.is_empty() {
        track.tag_album_artist.clone()
    } else {
        track.folder_artist.clone()
    };
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let parsed = parse_filename(stem, &hint);
    track.guess_title = parsed.title;
    track.guess_artist = parsed.artist;
    track.track_number = track.track_number.or(parsed.track_number);
    track.disc_number = track.disc_number.or(parsed.disc_number);

    track
}

/// 路径字符串，分隔符统一成当前平台的。
///
/// 调用方可能传 `root.join("raw/audio")` 这种混着写的路径，`Path::display`
/// 会原样输出 `D:\jp_corpus\raw/audio\...`。这个串要存进 `songs.audio_path`
/// 并和库里已有的 209 行比对，混着写会让 SQL 里的等值比较全部落空。
fn platform_path(path: &Path) -> String {
    path.components().collect::<PathBuf>().display().to_string()
}

/// 找同名的 .lrc。先看同目录，再看 `raw/lyrics_lrc/{stem}.lrc`
/// （本项目自己的布局）。
fn find_lyrics(audio: &Path) -> Option<PathBuf> {
    let stem = audio.file_stem()?;
    let sibling = audio.with_extension("lrc");
    if sibling.is_file() {
        return Some(sibling);
    }
    // 往上找项目根，再去 raw/lyrics_lrc 里看
    for ancestor in audio.ancestors().skip(1).take(5) {
        let candidate = ancestor.join("raw/lyrics_lrc").join(stem).with_extension("lrc");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 递归扫描目录。
///
/// `max_depth` 防止在符号链接成环时无限下去。
pub fn scan_dir(root: &Path, max_depth: usize) -> Vec<ScannedTrack> {
    let mut out = Vec::new();
    if root.is_file() {
        if is_audio_file(root) {
            out.push(scan_file(root));
        }
        return out;
    }
    walk(root, max_depth, &mut out);
    // 稳定顺序：同一个目录扫两次结果一致，UI 里也不会跳
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<ScannedTrack>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return; // 权限不足之类的目录跳过，不中断整轮
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            walk(&path, depth - 1, out);
        } else if is_audio_file(&path) {
            out.push(scan_file(&path));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_extensions_are_recognised() {
        for name in ["a.flac", "a.mp3", "a.M4A", "a.OGG"] {
            assert!(is_audio_file(Path::new(name)), "{name}");
        }
        for name in ["cover.jpg", "lyrics.lrc", "notes.txt", "noext"] {
            assert!(!is_audio_file(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn a_missing_file_produces_a_warning_not_a_panic() {
        let track = scan_file(Path::new("完全不存在.flac"));
        assert!(track.warning.is_some());
        // 但文件名信息仍然可用
        assert_eq!(track.file_name, "完全不存在.flac");
    }

    #[test]
    fn effective_values_fall_back_in_order() {
        let track = ScannedTrack {
            tag_artist: String::new(),
            tag_album_artist: "AlbumArtist".into(),
            guess_artist: "FromName".into(),
            folder_artist: "FromFolder".into(),
            ..Default::default()
        };
        assert_eq!(track.artist(), "AlbumArtist");

        let track = ScannedTrack {
            guess_artist: "FromName".into(),
            folder_artist: "FromFolder".into(),
            ..Default::default()
        };
        assert_eq!(track.artist(), "FromName");
    }

    #[test]
    fn a_track_without_any_title_is_flagged() {
        assert!(!ScannedTrack::default().has_identity());
        let track = ScannedTrack {
            guess_title: "群青".into(),
            ..Default::default()
        };
        assert!(track.has_identity());
    }

    #[test]
    fn stored_paths_use_one_separator() {
        // 调用方混着写也不该漏进库
        let track = scan_file(Path::new(r"D:\jp_corpus\raw/audio\歌手\001.flac"));
        if cfg!(windows) {
            assert_eq!(track.path, r"D:\jp_corpus\raw\audio\歌手\001.flac");
        }
        assert!(
            !(track.path.contains('/') && track.path.contains('\\')),
            "两种分隔符混在一个路径里：{}",
            track.path
        );
    }

    #[test]
    fn scanning_a_missing_directory_returns_empty() {
        assert!(scan_dir(Path::new("X:/nope/nope"), 3).is_empty());
    }

    #[test]
    fn depth_limit_is_respected() {
        // depth 0 表示不进入任何目录
        let mut out = Vec::new();
        walk(Path::new("."), 0, &mut out);
        assert!(out.is_empty());
    }
}
