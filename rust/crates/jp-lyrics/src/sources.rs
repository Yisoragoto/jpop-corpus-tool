//! 歌词来源的应用层：找本地来源、解析成统一时间轴、换来源、调偏移。
//!
//! 数据和优先级规则在 `jp_corpus::lyric_sources`，时间轴结构在 `jp_corpus::lyric_timeline`；
//! 这里把它们和解析器（`jp_import::lrc` / `jp_import::srt` / 逐字格式）、入库路径接起来。
//! Tauri 和 macOS（jp-ffi）都调这一层，两边的行为才是一样的。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use jp_corpus::lyric_sources::{self as store, SourceInput, SourceRow};
use jp_corpus::lyric_timeline::{self as timeline, LyricTimeline, RawLine};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::lyrics::{Attached, LyricsSource, SongRef};

/// 本地歌词文件认这几种扩展名。
const LOCAL_EXTENSIONS: [&str; 3] = ["lrc", "srt", "txt"];

/// 太大的不是歌词（多半是放错了的别的文本）。
const MAX_LOCAL_BYTES: u64 = 4 * 1024 * 1024;

/// 一份本地歌词文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFile {
    pub path: PathBuf,
    /// 文件名里的语言后缀（`song.zh-Hans.lrc` 的 `zh-Hans`），没有就是空串
    pub language: String,
    /// original / translation
    pub role: &'static str,
}

/// 语言标记：`zh`、`zh-Hans`、`pt-BR`、`ja-Latn` 这一类。
fn is_language_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let Some(primary) = parts.next() else { return false };
    (2..=3).contains(&primary.len())
        && primary.chars().all(|c| c.is_ascii_alphabetic())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// 音频旁边同名的歌词文件：`song.lrc`、`song.srt`、`song.txt`，以及带语言后缀的
/// `song.zh-Hans.lrc`。日语后缀（`ja`、`jpn`）算原文，其他语言算译文。
///
/// 只看音频所在的那一个目录。`raw/lyrics_lrc/{id}.lrc` 是本程序自己落盘的当前歌词，
/// 不算「旁边的文件」——否则用户改过的歌词会被当成外挂文件再登记一遍。
pub fn local_files(audio: &Path) -> Vec<LocalFile> {
    let (Some(dir), Some(stem)) = (audio.parent(), audio.file_stem().and_then(|s| s.to_str())) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()) else { continue };
        if !LOCAL_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let language = if name == stem {
            String::new()
        } else if let Some(tag) = name.strip_prefix(stem).and_then(|r| r.strip_prefix('.')) {
            if !is_language_tag(tag) {
                continue;
            }
            tag.to_string()
        } else {
            continue;
        };
        let primary = language.split('-').next().unwrap_or("").to_ascii_lowercase();
        let role = if language.is_empty() || primary == "ja" || primary == "jpn" { "original" } else { "translation" };
        out.push(LocalFile { path, language, role });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// 解析一份原文，得到时间轴用的行和夹带的译文。
pub fn parse_raw(raw: &str, format: &str) -> Result<(Vec<RawLine>, Vec<RawLine>)> {
    let from_lrc = |lines: Vec<jp_import::lrc::LyricLine>| -> Vec<RawLine> {
        lines
            .into_iter()
            .map(|l| RawLine {
                start_sec: l.time_sec,
                end_sec: None,
                words: l.words.iter().map(|w| (w.start, w.end, w.time_sec, w.end_sec)).collect(),
                text: l.text,
                utterance_id: None,
            })
            .collect()
    };
    Ok(match format {
        "srt" => {
            let parsed = jp_import::srt::parse(raw);
            let mut lines = from_lrc(parsed.lines);
            for (line, end) in lines.iter_mut().zip(parsed.ends) {
                line.end_sec = Some(end);
            }
            (lines, from_lrc(parsed.translations))
        }
        _ => {
            let lrc = canonical_lrc(raw, format)?;
            let parsed = jp_import::lrc::parse(&lrc);
            (from_lrc(parsed.lines), from_lrc(parsed.translations))
        }
    })
}

/// 转成入库用的 LRC 文本。LRC、增强 LRC、纯文本原样；SRT 和逐字格式转换。
pub fn canonical_lrc(raw: &str, format: &str) -> Result<String> {
    use jp_scraper::timed_lyrics as t;
    Ok(match format {
        "srt" => {
            let cues = jp_import::srt::parse_cues(raw);
            ensure!(!cues.is_empty(), "这份 SRT 里没有认得出的字幕");
            jp_import::srt::to_lrc(&cues)
        }
        "yrc" => t::parse_yrc(raw)?.to_enhanced_lrc(),
        "qrc" => t::parse_qrc(raw)?.to_enhanced_lrc(),
        "krc" => t::parse_krc(raw)?.to_enhanced_lrc(),
        "ttml" => t::parse_ttml(raw)?.to_enhanced_lrc(),
        _ => raw.to_string(),
    })
}

fn format_of(path: &Path, raw: &str) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("srt") => "srt",
        _ => match store::sniff_format(raw) {
            "srt" => "srt",
            other => other,
        },
    }
}

/// 一份原文的行数、带时间的行数、精度。
fn measure(raw: &str, format: &str) -> (i64, i64, &'static str) {
    let Ok((lines, _)) = parse_raw(raw, format) else { return (0, 0, "none") };
    let timed = lines.iter().filter(|l| l.start_sec.is_some()).count() as i64;
    let precision = if lines.iter().any(|l| !l.words.is_empty()) {
        "word"
    } else if timed > 0 {
        "line"
    } else {
        "none"
    };
    (lines.len() as i64, timed, precision)
}

fn audio_path(conn: &Connection, song_id: &str) -> Result<String> {
    conn.query_row("SELECT COALESCE(audio_path,'') FROM songs WHERE id=?1", [song_id], |r| r.get(0))
        .optional()?
        .with_context(|| format!("找不到曲目 {song_id}"))
}

/// 本轮扫描的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovered {
    /// 新登记或内容有变的来源数
    pub updated: usize,
    /// 文件不见了、标成缺失的来源数
    pub missing: usize,
}

/// 把音频旁边的同名文件和内嵌歌词登记成来源。只读文件，不动当前歌词。
///
/// 用户改过的行不会被覆盖（见 `lyric_sources::upsert`）。上次登记过、这次不在了的标成缺失，
/// 行留着：原文还能看，选中的那份也不会因为文件被挪走就消失。
pub fn discover_local(conn: &Connection, song_id: &str) -> Result<Discovered> {
    discover_local_at(conn, song_id, None)
}

/// 同 [`discover_local`]，音频路径由调用方给（库里记的是相对路径、或者库从别的机器拷来，
/// macOS 端先按音频根目录找到真实文件再传进来）。`None` 就用库里记的路径。
pub fn discover_local_at(conn: &Connection, song_id: &str, audio: Option<&Path>) -> Result<Discovered> {
    let stored = audio_path(conn, song_id)?;
    let mut report = Discovered::default();
    let audio = match audio {
        Some(path) => path.to_path_buf(),
        None if stored.is_empty() => return Ok(report),
        None => PathBuf::from(&stored),
    };
    let audio = audio.as_path();
    let before: Vec<SourceRow> = store::list(conn, song_id)?;
    let mut seen = Vec::new();
    for file in local_files(audio) {
        if std::fs::metadata(&file.path).map(|m| m.len()).unwrap_or(0) > MAX_LOCAL_BYTES {
            continue;
        }
        let Ok(raw) = crate::lyrics::read_text(&file.path) else { continue };
        if raw.trim().is_empty() {
            continue;
        }
        let format = format_of(&file.path, &raw);
        let (lines, timed, precision) = measure(&raw, format);
        let name = file.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let hash = store::raw_hash(&raw);
        let id = store::upsert(
            conn,
            &SourceInput {
                song_id,
                kind: "sibling",
                provider: &name,
                source_ref: &file.path.display().to_string(),
                role: file.role,
                language: &file.language,
                source_type: "sibling",
                raw_format: format,
                raw_text: Some(&raw),
                content_hash: &hash,
                line_count: lines,
                timed_line_count: timed,
                precision,
                user_modified: false,
            },
        )?;
        if !before.iter().any(|r| r.id == id && r.content_hash == hash && !r.missing) {
            report.updated += 1;
        }
        seen.push(id);
    }
    if audio.is_file()
        && let Some(embedded) = jp_import::embedded::read_lyrics(audio) {
            let format = store::sniff_format(&embedded.text);
            let (lines, timed, precision) = measure(&embedded.text, format);
            let name = audio.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let hash = store::raw_hash(&embedded.text);
            let id = store::upsert(
                conn,
                &SourceInput {
                    song_id,
                    kind: "embedded",
                    provider: &name,
                    source_ref: &audio.display().to_string(),
                    role: "original",
                    source_type: "embedded",
                    raw_format: format,
                    raw_text: Some(&embedded.text),
                    content_hash: &hash,
                    line_count: lines,
                    timed_line_count: timed,
                    precision,
                    ..Default::default()
                },
            )?;
            if !before.iter().any(|r| r.id == id && r.content_hash == hash && !r.missing) {
                report.updated += 1;
            }
            seen.push(id);
        }
    // 上次在、这次没找到的本地来源。只看这一轮确实去找过的类别；
    // 记的路径不是音频旁边（老布局的 raw/lyrics_lrc）的，按路径是否还在判断
    for row in before.iter().filter(|r| matches!(r.kind.as_str(), "sibling" | "embedded") && !r.missing) {
        if seen.contains(&row.id) {
            continue;
        }
        let gone = if row.kind == "embedded" {
            audio.is_file()
        } else {
            row.source_ref.is_empty() || !Path::new(&row.source_ref).is_file()
        };
        if gone {
            conn.execute("UPDATE lyric_sources SET missing=1 WHERE id=?1", [row.id])?;
            report.missing += 1;
        }
    }
    Ok(report)
}

// ── 时间轴 ────────────────────────────────────────────────────────

fn duration(conn: &Connection, song_id: &str) -> Result<Option<f64>> {
    Ok(conn
        .query_row("SELECT duration_sec FROM songs WHERE id=?1", [song_id], |r| r.get::<_, Option<f64>>(0))
        .optional()?
        .flatten())
}

/// 一个来源（不一定是当前在用的）解析出的时间轴。预览、对比用；按解析器版本缓存。
pub fn source_timeline(conn: &Connection, source_id: i64) -> Result<LyricTimeline> {
    let row = store::get(conn, source_id)?.with_context(|| format!("找不到歌词来源 {source_id}"))?;
    if let Some(cached) = store::cached_timeline(conn, source_id, &row.content_hash)? {
        return Ok(cached);
    }
    let raw = store::raw_text(conn, source_id)?.context("这个来源当时没有保存原文")?;
    let (lines, translations) = parse_raw(&raw, &row.raw_format)?;
    let mut tl = timeline::build(&row.song_id, lines, duration(conn, &row.song_id)?, row.offset_ms);
    tl.source_id = Some(row.id);
    tl.source_kind = row.kind.clone();
    tl.raw_format = row.raw_format.clone();
    if !translations.is_empty() {
        timeline::align_translation(&mut tl, &translations, "");
    }
    store::store_timeline(conn, source_id, &row.content_hash, &tl)?;
    Ok(tl)
}

/// 这首歌现在的统一时间轴：行和时间取自库里（`utterances`、来源给的逐字时间），
/// 换算回歌词时间；结束时间、译文取自选中的来源。显示、跳转、同步都读它。
pub fn song_timeline(conn: &Connection, song_id: &str) -> Result<LyricTimeline> {
    let selection = store::selection(conn, song_id)?;
    let offset = selection.as_ref().map_or(0, |s| s.offset_ms);
    let source = match selection.as_ref().and_then(|s| s.source_id) {
        Some(id) => store::get(conn, id)?,
        None => None,
    };
    let rows: Vec<(i64, Option<f64>, String)> = conn
        .prepare("SELECT id, time_sec, text FROM utterances WHERE song_id=?1 ORDER BY line_idx, id")?
        .query_map([song_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let timings = jp_corpus::word_timing::for_song(conn, song_id)?;
    let shift = offset as f64 / 1000.0;

    // 选中的来源原文：拿它的结束时间（SRT）和夹带的译文
    let parsed_source = match &source {
        Some(row) if row.has_raw => store::raw_text(conn, row.id)?.and_then(|raw| parse_raw(&raw, &row.raw_format).ok()),
        _ => None,
    };
    let explicit_end = |start: Option<f64>, text: &str| -> Option<f64> {
        let (lines, _) = parsed_source.as_ref()?;
        let start = start?;
        lines.iter().find(|l| l.text == text && l.start_sec.is_some_and(|s| (s - start).abs() < 0.006)).and_then(|l| l.end_sec)
    };

    let lines: Vec<RawLine> = rows
        .into_iter()
        .map(|(id, time, text)| {
            let lyric_time = time.map(|t| t - shift);
            let mut words: Vec<(usize, usize, f64, Option<f64>)> = Vec::new();
            let mine: Vec<_> = timings.iter().filter(|w| w.utterance_id == id).collect();
            // 同一处有手动校准时以它为准（和显示层一致）
            for w in mine.iter().filter(|w| w.source == "lrc") {
                let manual = mine.iter().find(|m| m.source == "manual" && m.char_start == w.char_start && m.char_end == w.char_end);
                let (start, end) = match manual {
                    Some(m) if m.end_sec.is_some() => (m.start_sec, m.end_sec),
                    _ => (w.start_sec - shift, w.end_sec.map(|e| e - shift)),
                };
                words.push((w.char_start as usize, w.char_end as usize, start, end));
            }
            words.sort_by_key(|a| a.0);
            RawLine { start_sec: lyric_time, end_sec: explicit_end(lyric_time, &text), text, words, utterance_id: Some(id) }
        })
        .collect();
    let mut tl = timeline::build(song_id, lines, duration(conn, song_id)?, offset);
    if let Some(row) = &source {
        tl.source_id = Some(row.id);
        tl.source_kind = row.kind.clone();
        tl.raw_format = row.raw_format.clone();
    }

    let all = store::list(conn, song_id)?;
    match store::chosen_translation(&all, selection.as_ref()) {
        Some(id) => {
            let row = all.iter().find(|r| r.id == id).expect("chosen_translation 只返回列表里的");
            if let Some(raw) = store::raw_text(conn, id)? {
                let (lines, _) = parse_raw(&raw, &row.raw_format)?;
                timeline::align_translation(&mut tl, &lines, &row.language);
            }
        }
        None if !selection.as_ref().is_some_and(|s| s.translation_off) => {
            if let Some((_, inline)) = &parsed_source
                && !inline.is_empty() {
                    timeline::align_translation(&mut tl, inline, "");
                }
        }
        None => {}
    }
    Ok(tl)
}

// ── 换来源 / 偏移 ─────────────────────────────────────────────────

fn attached_source(row: &SourceRow) -> LyricsSource {
    match (row.kind.as_str(), row.provider.as_str()) {
        ("sibling", _) => LyricsSource::Sibling,
        ("embedded", _) => LyricsSource::Embedded,
        ("online", "qq") => LyricsSource::Qq,
        ("online", "kugou") => LyricsSource::Kugou,
        ("online", "amll") => LyricsSource::Amll,
        ("online", "apple") => LyricsSource::Apple,
        ("online", "lrclib") => LyricsSource::Lrclib,
        ("online", _) => LyricsSource::Netease,
        ("user", "edit") => LyricsSource::Edited,
        ("user", _) => LyricsSource::Manual,
        _ => LyricsSource::Restored,
    }
}

/// 把某个来源挂成当前歌词。`by_user` 为真时记成你的选择，之后的自动流程不再换它。
///
/// 入库走和导入、补齐同一条路（分词、全文索引、作词作曲、分词校正一并处理）；
/// 这个来源记过偏移的话，挂上之后照那个偏移落库。
pub fn apply_source(
    conn: &mut Connection,
    lyrics_dir: &Path,
    song_id: &str,
    source_id: i64,
    by_user: bool,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Attached> {
    let row = store::get(conn, source_id)?.with_context(|| format!("找不到歌词来源 {source_id}"))?;
    ensure!(row.song_id == song_id, "来源 {source_id} 不是这首歌的");
    ensure!(row.role == "original", "这是一份译文，不能当原文用；在「译文」里选它");
    let raw = store::raw_text(conn, source_id)?.context("这个来源当时没有保存原文，没法重新挂上")?;
    let text = canonical_lrc(&raw, &row.raw_format)?;
    let (artist, title): (String, String) =
        conn.query_row("SELECT artist, title FROM songs WHERE id=?1", [song_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let source_type = if row.source_type.is_empty() {
        match row.kind.as_str() {
            "user" => "manual_file",
            "legacy" => "legacy",
            other => other,
        }
        .to_string()
    } else {
        row.source_type.clone()
    };
    let lineage = jp_import::LyricSource::new(&source_type, &row.provider, &row.source_ref, if by_user { "user" } else { "system" });
    let converted = text != raw;
    let attached = crate::lyrics::apply_with_lineage(
        conn,
        lyrics_dir,
        SongRef { id: song_id, artist: &artist, title: &title },
        &text,
        attached_source(&row),
        &row.provider,
        Some(lineage),
        analyzer,
        None,
        None,
        converted.then_some((row.raw_format.as_str(), raw.as_str())),
    )?;
    let tx = conn.transaction()?;
    // 版本登记时会把来源落到同一行（同一类、同一名字）。万一没落到（老数据名字对不上），
    // 也把选择指回用户点的这一行
    tx.execute("UPDATE lyric_selection SET source_id=?2 WHERE song_id=?1", params![song_id, source_id])?;
    if by_user {
        store::set_chosen_by(&tx, song_id, "user")?;
    }
    if row.offset_ms != 0 {
        store::set_offset_in(&tx, song_id, row.offset_ms)?;
    }
    tx.commit()?;
    Ok(attached)
}

/// 自动流程：重新找本地来源，选择不是你定的（也不是升级前就有的）时，按优先级换到最好的那份。
/// 换了返回挂上的结果。
pub fn reconcile(
    conn: &mut Connection,
    lyrics_dir: &Path,
    song_id: &str,
    audio: Option<&Path>,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Option<Attached>> {
    discover_local_at(conn, song_id, audio)?;
    if store::locked(conn, song_id)? {
        return Ok(None);
    }
    let rows = store::list(conn, song_id)?;
    let Some(best) = store::best_original(&rows) else { return Ok(None) };
    let current = store::selection(conn, song_id)?.and_then(|s| s.source_id);
    let current_rank = current
        .and_then(|id| rows.iter().find(|r| r.id == id))
        .map(|r| (store::kind_rank(&r.kind), r.precision.clone()));
    let better = match current_rank {
        None => true,
        Some((rank, _)) => store::kind_rank(&best.kind) < rank,
    };
    if !better || current == Some(best.id) {
        return Ok(None);
    }
    let id = best.id;
    Ok(Some(apply_source(conn, lyrics_dir, song_id, id, false, analyzer)?))
}

/// 「歌词来源」面板要的东西。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcesView {
    pub song_id: String,
    pub sources: Vec<SourceRow>,
    pub selection: Option<store::Selection>,
    /// 按优先级默认该用的那份；和当前选择不同时界面提示一句
    pub recommended: Option<i64>,
    /// 现在显示的译文（选择里指定的，或者自动取的第一份）
    pub translation_source_id: Option<i64>,
    pub lookups: Vec<store::Lookup>,
}

pub fn view(conn: &Connection, song_id: &str) -> Result<SourcesView> {
    let sources = store::list(conn, song_id)?;
    let selection = store::selection(conn, song_id)?;
    Ok(SourcesView {
        song_id: song_id.to_string(),
        recommended: store::best_original(&sources).map(|r| r.id),
        translation_source_id: store::chosen_translation(&sources, selection.as_ref()),
        lookups: store::lookups(conn, song_id)?,
        sources,
        selection,
    })
}

/// 先找一遍本地来源再列出来。打开面板时调：放了新文件进去，面板一打开就看得到。
pub fn refresh_and_view(conn: &Connection, song_id: &str, audio: Option<&Path>) -> Result<SourcesView> {
    discover_local_at(conn, song_id, audio)?;
    view(conn, song_id)
}

/// 调整体偏移（毫秒）。没有登记来源的老歌先补登记。
pub fn set_offset(conn: &mut Connection, song_id: &str, offset_ms: i64) -> Result<usize> {
    if store::selection(conn, song_id)?.is_none() {
        let tx = conn.transaction()?;
        store::backfill(&tx)?;
        tx.commit()?;
    }
    if store::selection(conn, song_id)?.is_none() {
        bail!("这首歌还没有歌词，没法调偏移");
    }
    store::set_offset(conn, song_id, offset_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jp-lyrics-sources-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn db(audio: &Path) -> jp_corpus::Corpus {
        let corpus = jp_corpus::Corpus::open_in_memory().unwrap();
        corpus.ensure_schema().unwrap();
        corpus
            .connection()
            .execute(
                "INSERT INTO songs(id,title,artist,audio_path,duration_sec) VALUES('s','曲','歌手',?1,30)",
                [audio.display().to_string()],
            )
            .unwrap();
        corpus
    }

    fn lines(conn: &Connection) -> Vec<(Option<f64>, String)> {
        conn.prepare("SELECT time_sec, text FROM utterances WHERE song_id='s' ORDER BY line_idx")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn local_files_include_language_suffixed_translations_and_skip_other_names() {
        let dir = scratch("files");
        let audio = dir.join("song.flac");
        for name in ["song.flac", "song.lrc", "song.zh-Hans.lrc", "song.ja.srt", "song.txt", "song2.lrc", "song.backup.lrc", "other.lrc"] {
            std::fs::write(dir.join(name), "x").unwrap();
        }
        let found: Vec<(String, String, &str)> = local_files(&audio)
            .into_iter()
            .map(|f| (f.path.file_name().unwrap().to_string_lossy().to_string(), f.language, f.role))
            .collect();
        assert_eq!(
            found,
            vec![
                ("song.ja.srt".into(), "ja".into(), "original"),
                ("song.lrc".into(), "".into(), "original"),
                ("song.txt".into(), "".into(), "original"),
                ("song.zh-Hans.lrc".into(), "zh-Hans".into(), "translation"),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovery_registers_sources_marks_vanished_ones_and_never_touches_a_user_edit() {
        let dir = scratch("discover");
        let audio = dir.join("song.flac");
        std::fs::write(&audio, b"not really audio").unwrap();
        std::fs::write(dir.join("song.lrc"), "[00:01.00]夜が明ける\n[00:04.00]君を待つ\n").unwrap();
        std::fs::write(dir.join("song.zh-Hans.lrc"), "[00:01.00]天亮了\n[00:04.00]等你\n").unwrap();
        let corpus = db(&audio);
        let conn = corpus.connection();
        store::upsert(
            conn,
            &SourceInput { song_id: "s", kind: "user", provider: "edit", role: "original", raw_text: Some("我的"), content_hash: "h", line_count: 1, user_modified: true, ..Default::default() },
        )
        .unwrap();

        let first = discover_local(conn, "s").unwrap();
        assert_eq!(first, Discovered { updated: 2, missing: 0 });
        assert_eq!(discover_local(conn, "s").unwrap(), Discovered::default(), "没变化时什么都不算");

        std::fs::remove_file(dir.join("song.lrc")).unwrap();
        assert_eq!(discover_local(conn, "s").unwrap().missing, 1);
        let rows = store::list(conn, "s").unwrap();
        let user = rows.iter().find(|r| r.kind == "user").unwrap();
        assert_eq!(store::raw_text(conn, user.id).unwrap().as_deref(), Some("我的"));
        assert!(rows.iter().find(|r| r.provider == "song.lrc").unwrap().missing);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn choosing_a_source_applies_it_locks_it_and_reconcile_then_leaves_it_alone() {
        let dir = scratch("select");
        let audio = dir.join("song.flac");
        std::fs::write(&audio, b"x").unwrap();
        let mut corpus = db(&audio);
        let conn = corpus.connection_mut();
        let lyrics_dir = dir.join("lyrics");

        // 先有一份在线的（自动挂上的）
        crate::lyrics::apply_lrc_text(conn, &lyrics_dir, SongRef { id: "s", artist: "歌手", title: "曲" }, "[00:01.00]オンライン\n", LyricsSource::Netease, "曲 — 歌手", None).unwrap();
        assert_eq!(store::selection(conn, "s").unwrap().unwrap().chosen_by, "auto");

        // 旁边放了一份 .lrc：自动流程按优先级换过去
        std::fs::write(dir.join("song.lrc"), "[00:02.00]となり\n").unwrap();
        assert!(reconcile(conn, &lyrics_dir, "s", None, None).unwrap().is_some());
        assert_eq!(lines(conn), vec![(Some(2.0), "となり".to_string())]);

        // 你手动选回在线那份：锁定，之后自动流程不换
        let online = store::list(conn, "s").unwrap().into_iter().find(|r| r.kind == "online").unwrap();
        apply_source(conn, &lyrics_dir, "s", online.id, true, None).unwrap();
        assert_eq!(lines(conn), vec![(Some(1.0), "オンライン".to_string())]);
        assert_eq!(store::selection(conn, "s").unwrap().unwrap().chosen_by, "user");
        assert!(reconcile(conn, &lyrics_dir, "s", None, None).unwrap().is_none());
        assert_eq!(lines(conn), vec![(Some(1.0), "オンライン".to_string())]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_offset_moves_line_and_source_word_times_but_not_manual_calibrations_and_round_trips() {
        let dir = scratch("offset");
        let mut corpus = jp_corpus::Corpus::open_in_memory().unwrap();
        corpus.ensure_schema().unwrap();
        let conn = corpus.connection_mut();
        conn.execute("INSERT INTO songs(id,title,artist,duration_sec) VALUES('s','曲','歌手',30)", []).unwrap();
        crate::lyrics::apply_lrc_text(conn, &dir, SongRef { id: "s", artist: "歌手", title: "曲" }, "[00:01.00]<00:01.00>夜<00:01.50>が\n[00:05.00]朝\n", LyricsSource::Sibling, "song.lrc", None).unwrap();
        let first: i64 = conn.query_row("SELECT id FROM utterances WHERE song_id='s' ORDER BY line_idx LIMIT 1", [], |r| r.get(0)).unwrap();
        jp_corpus::word_timing::set_manual(conn, first, 1, 2, 1.6).unwrap();

        set_offset(conn, "s", 250).unwrap();
        assert_eq!(lines(conn), vec![(Some(1.25), "夜が".to_string()), (Some(5.25), "朝".to_string())]);
        let lrc: f64 = conn.query_row("SELECT start_sec FROM word_timings WHERE utterance_id=?1 AND char_start=0 AND source='lrc'", [first], |r| r.get(0)).unwrap();
        let manual: f64 = conn.query_row("SELECT start_sec FROM word_timings WHERE utterance_id=?1 AND source='manual'", [first], |r| r.get(0)).unwrap();
        assert!((lrc - 1.25).abs() < 1e-9);
        assert!((manual - 1.6).abs() < 1e-9, "手动校准是对着音频打的，不跟着平移");

        // 时间轴：歌词时间不变，偏移另记；换回曲目时间和库里一致（跳转用的就是这一列）
        let tl = song_timeline(conn, "s").unwrap();
        assert_eq!(tl.offset_ms, 250);
        assert_eq!(tl.lines[0].start_ms, Some(1000));
        assert_eq!(timeline::lyric_to_track_ms(tl.lines[1].start_ms.unwrap(), tl.offset_ms), 5250);

        set_offset(conn, "s", -500).unwrap();
        set_offset(conn, "s", 0).unwrap();
        assert_eq!(lines(conn), vec![(Some(1.0), "夜が".to_string()), (Some(5.0), "朝".to_string())]);
        assert!(set_offset(conn, "s", 40_000).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_srt_source_keeps_its_end_times_and_inline_translations_in_the_timeline() {
        let dir = scratch("srt");
        let audio = dir.join("song.flac");
        std::fs::write(&audio, b"x").unwrap();
        std::fs::write(dir.join("song.srt"), "1\n00:00:01,000 --> 00:00:02,500\n夜が明ける\n天亮了\n\n2\n00:00:06,000 --> 00:00:08,000\n君を待つ\n等你\n").unwrap();
        let mut corpus = jp_corpus::Corpus::open_in_memory().unwrap();
        corpus.ensure_schema().unwrap();
        let conn = corpus.connection_mut();
        conn.execute("INSERT INTO songs(id,title,artist,audio_path,duration_sec) VALUES('s','曲','歌手',?1,30)", [audio.display().to_string()]).unwrap();
        discover_local(conn, "s").unwrap();
        let srt = store::list(conn, "s").unwrap().into_iter().find(|r| r.raw_format == "srt").unwrap();
        apply_source(conn, &dir.join("lyrics"), "s", srt.id, true, None).unwrap();
        assert_eq!(lines(conn), vec![(Some(1.0), "夜が明ける".to_string()), (Some(6.0), "君を待つ".to_string())]);

        let tl = song_timeline(conn, "s").unwrap();
        assert_eq!(tl.raw_format, "srt");
        assert_eq!(tl.lines[0].end_ms, Some(2500), "SRT 给的结束时间，不是下一行的开始");
        assert_eq!(tl.lines[1].end_ms, Some(8000));
        assert_eq!(tl.translation.state, timeline::TranslationState::Aligned);
        assert_eq!(tl.lines[0].translation.as_deref(), Some("天亮了"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plain_text_has_no_timeline_times() {
        let dir = scratch("plain");
        let mut corpus = jp_corpus::Corpus::open_in_memory().unwrap();
        corpus.ensure_schema().unwrap();
        let conn = corpus.connection_mut();
        conn.execute("INSERT INTO songs(id,title,artist,duration_sec) VALUES('s','曲','歌手',30)", []).unwrap();
        crate::lyrics::apply_lrc_text(conn, &dir, SongRef { id: "s", artist: "歌手", title: "曲" }, "一行目\n二行目\n", LyricsSource::Manual, "a.txt", None).unwrap();
        let tl = song_timeline(conn, "s").unwrap();
        assert_eq!(tl.precision, timeline::Precision::None);
        assert!(tl.lines.iter().all(|l| l.start_ms.is_none()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
