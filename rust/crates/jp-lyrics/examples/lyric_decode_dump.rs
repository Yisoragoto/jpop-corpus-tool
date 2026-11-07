//! 把一份库里存着的每份歌词原文重新解析一遍，逐行写出时间和逐字时间。
//!
//! 给解析器的重构对账用（#61）：同一份库副本，旧代码和新代码各跑一次，两份输出 diff 应当为空。
//! 浮点数用 `{:?}` 写出，逐位可比。**只读库副本**，别指向正在用的库。
//!
//! ```text
//! cargo run -p jp-lyrics --example lyric_decode_dump -- <corpus.db 副本> <输出文件> [--embedded]
//! ```
//!
//! 输出每行一条记录：`类别 \t 主键 \t 内容`。
//! - `V`：`lyric_versions.raw_text` 走 LRC 解析（语料入库的那条路）
//! - `S`：`lyric_versions.source_raw` 按 `raw_format` 走时间轴解析
//! - `C`：`lyric_versions.source_raw` 转成的入库 LRC 文本
//! - `L`：`lyric_sources.raw_text` 按 `raw_format` 走时间轴解析
//! - `D`：每份原文（`raw_text` / `source_raw` / 来源原文）按内容嗅出的格式（`sniff_format`）
//! - `E`：（`--embedded`）每首歌音频里内嵌的歌词，读出来的原文和 LRC 解析结果
//! - `F`：（`--files 目录…`）目录里的每个 .lrc 走读文件 + LRC 解析（导入那条路）

use std::fmt::Write as _;
use std::io::Write as _;

use jp_corpus::lyric_timeline::RawLine;
use rusqlite::Connection;

fn raw_lines(out: &mut String, lines: &[RawLine]) {
    for l in lines {
        let _ = write!(out, "[{:?}|{:?}|{:?}|", l.start_sec, l.end_sec, l.text);
        for w in &l.words {
            let _ = write!(out, "({},{},{:?},{:?})", w.0, w.1, w.2, w.3);
        }
        out.push(']');
    }
}

fn lrc_lines(out: &mut String, lines: &[jp_import::lrc::LyricLine]) {
    for l in lines {
        let _ = write!(out, "[{:?}|{:?}|", l.time_sec, l.text);
        for w in &l.words {
            let _ = write!(out, "({},{},{:?},{:?})", w.start, w.end, w.time_sec, w.end_sec);
        }
        out.push(']');
    }
}

fn parsed(out: &mut String, text: &str) {
    let p = jp_import::lrc::parse(text);
    lrc_lines(out, &p.lines);
    out.push_str(" T:");
    lrc_lines(out, &p.translations);
    out.push_str(" K:");
    for c in &p.credits {
        let _ = write!(out, "{}={:?};", c.role, c.name);
    }
}

fn timeline(out: &mut String, raw: &str, format: &str) {
    match jp_lyrics::sources::parse_raw(raw, format) {
        Ok((lines, translations)) => {
            raw_lines(out, &lines);
            out.push_str(" T:");
            raw_lines(out, &translations);
        }
        Err(e) => {
            let _ = write!(out, "ERR {e:#}");
        }
    }
}

fn walk(dir: &std::path::Path, depth: usize, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth > 0 {
                walk(&path, depth - 1, out);
            }
        } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("lrc")) {
            out.push(path);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(args.len() >= 3, "usage: lyric_decode_dump <corpus.db copy> <output> [--embedded] [--files DIR...]");
    let conn = Connection::open_with_flags(&args[1], rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut out = std::io::BufWriter::new(std::fs::File::create(&args[2])?);
    let mut counts = [0usize; 6];

    let mut stmt = conn.prepare("SELECT id, raw_text, raw_format, source_raw FROM lyric_versions ORDER BY id")?;
    // 版本 id、raw_text、raw_format、source_raw
    type VersionRow = (i64, Option<String>, Option<String>, Option<String>);
    let rows: Vec<VersionRow> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
    for (id, raw_text, format, source_raw) in rows {
        if let Some(text) = raw_text {
            let mut line = String::new();
            parsed(&mut line, &text);
            writeln!(out, "V\t{id}\t{line}")?;
            writeln!(out, "D\tv{id}\t{}", jp_corpus::lyric_sources::sniff_format(&text))?;
            counts[0] += 1;
        }
        if let Some(raw) = source_raw {
            let format = format.unwrap_or_default();
            let mut line = String::new();
            timeline(&mut line, &raw, &format);
            writeln!(out, "S\t{id}\t{line}")?;
            writeln!(out, "D\ts{id}\t{}", jp_corpus::lyric_sources::sniff_format(&raw))?;
            counts[1] += 1;
            let canonical = jp_lyrics::sources::canonical_lrc(&raw, &format).map_err(|e| format!("{e:#}"));
            writeln!(out, "C\t{id}\t{canonical:?}")?;
            counts[2] += 1;
        }
    }

    let mut stmt = conn.prepare("SELECT id, raw_text, raw_format FROM lyric_sources WHERE raw_text IS NOT NULL ORDER BY id")?;
    let rows: Vec<(i64, String, String)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    for (id, raw, format) in rows {
        let mut line = String::new();
        timeline(&mut line, &raw, &format);
        writeln!(out, "L\t{id}\t{line}")?;
        writeln!(out, "D\tl{id}\t{}", jp_corpus::lyric_sources::sniff_format(&raw))?;
        counts[3] += 1;
    }

    if args.iter().any(|a| a == "--embedded") {
        let mut stmt = conn.prepare("SELECT id, audio_path FROM songs WHERE COALESCE(audio_path,'')<>'' ORDER BY id")?;
        let rows: Vec<(String, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        for (id, path) in rows {
            let Some(found) = jp_import::embedded::read_lyrics(std::path::Path::new(&path)) else { continue };
            let mut line = format!("synced={} raw={:?} ", found.synced, found.text);
            parsed(&mut line, &found.text);
            writeln!(out, "E\t{id}\t{line}")?;
            counts[4] += 1;
        }
    }
    if let Some(at) = args.iter().position(|a| a == "--files") {
        let mut files = Vec::new();
        for dir in &args[at + 1..] {
            walk(std::path::Path::new(dir), 6, &mut files);
        }
        files.sort();
        for path in files {
            let line = match jp_import::lrc::parse_file(&path) {
                Ok(p) => {
                    let mut line = String::new();
                    lrc_lines(&mut line, &p.lines);
                    line.push_str(" T:");
                    lrc_lines(&mut line, &p.translations);
                    line
                }
                Err(e) => format!("ERR {e:#}"),
            };
            writeln!(out, "F	{}	{line}", path.display())?;
            counts[5] += 1;
        }
    }
    out.flush()?;
    eprintln!("V={} S={} C={} L={} E={} F={}", counts[0], counts[1], counts[2], counts[3], counts[4], counts[5]);
    Ok(())
}
