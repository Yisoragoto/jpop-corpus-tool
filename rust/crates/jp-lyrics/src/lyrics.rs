//! 补齐歌词：音频旁边的 .lrc、在线搜、用户自己指定的文件，三条路一个出口。
//!
//! ## 为什么需要这一层
//!
//! 0.1.x 的歌词是 `legacy/scripts/01_fetch_lrc.py` 用 `syncedlyrics` 下回来的，
//! 存成 `raw/lyrics_lrc/{song_id}.lrc`，然后 `02_parse_lrc_tokenize.py` 才入库。
//! Tauri 版的导入**只认已经在磁盘上的 .lrc**（扫描时在音频旁边找），
//! 于是从别处拷进来的歌——音乐 App 的下载目录里只有音频——导进来是一行歌词都没有。
//! 用户看到的就是「以前导入会自动匹配歌词，现在不会了」。
//!
//! ## 四条路，一个出口
//!
//! | 来源 | 什么时候用 |
//! |---|---|
//! | `Embedded` 音频文件里内嵌的歌词 | 当初导入时还不认内嵌歌词（0.2.9 及以前） |
//! | `Sibling` 音频旁边的 `<同名>.lrc` | 导入之后才下了歌词、或者导入时漏了 |
//! | `Netease` 在线搜 | 旁边什么都没有（见 [`jp_scraper::lyrics`]） |
//! | `Manual` 用户自己选的文件 | 自动匹配挑不出来，或者用户有更好的一份 |
//!
//! 四条路都走 [`apply_lrc_text`]：**先把 .lrc 落到 `raw/lyrics_lrc/{song_id}.lrc`，
//! 再入库**。顺序是刻意的——
//!
//! * 落盘在前：写文件失败就什么都没改，重试一次即可；
//! * 一个事务里清旧挂新：歌词行、全文索引、分词、作词作曲必须同时成立，
//!   不然会得到一首「有歌词但搜不到」或者「分了一半词」的歌；
//! * 文件留着是为了可恢复：库坏了、或者想换个分词器重新入库，手上还有原始歌词，
//!   和 0.1.x 的目录结构也对得上。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde::Serialize;

/// 一首还没有歌词的歌。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingLyrics {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub duration_sec: Option<f64>,
    pub audio_path: String,
    /// 音频旁边就有一份 .lrc——这种不用上网
    pub sibling_lrc: Option<String>,
}

/// 歌词是哪来的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LyricsSource {
    /// 音频文件里内嵌的
    Embedded,
    /// 音频旁边的同名 .lrc
    Sibling,
    /// 在线搜到的
    Netease,
    Qq,
    Kugou,
    Amll,
    Apple,
    Lrclib,
    /// 用户自己选的文件
    Manual,
    /// 从这首歌以前的某一版恢复的
    Restored,
    /// 你在界面上逐行改过的
    Edited,
}

impl LyricsSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::Sibling => "sibling",
            Self::Netease => "netease",
            Self::Qq => "qq",
            Self::Kugou => "kugou",
            Self::Amll => "amll",
            Self::Apple => "apple",
            Self::Lrclib => "lrclib",
            Self::Manual => "manual",
            Self::Restored => "restored",
            Self::Edited => "edited",
        }
    }

    /// 记进 `lyric_versions` 的来源。`matched` 是界面上显示的那个名字（文件名、在线条目）
    pub(crate) fn lineage(self, matched: &str) -> jp_import::LyricSource {
        let (kind, name, reference) = match self {
            Self::Embedded => ("embedded", matched, ""),
            Self::Sibling => ("sibling", matched, ""),
            Self::Netease => ("online", "netease", matched),
            Self::Qq => ("online", "qq", matched),
            Self::Kugou => ("online", "kugou", matched),
            Self::Amll => ("online", "amll", matched),
            Self::Apple => ("online", "apple", matched),
            Self::Lrclib => ("online", "lrclib", matched),
            Self::Manual => ("manual_file", matched, ""),
            Self::Restored => ("restored", matched, ""),
            Self::Edited => ("manual_edit", matched, ""),
        };
        jp_import::LyricSource::new(kind, name, reference, "user")
    }
}

/// 一首歌挂上歌词之后的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attached {
    pub song_id: String,
    pub source: LyricsSource,
    /// 在线搜到的那条叫什么（「曲名 — 歌手」），本地来源是文件名
    pub matched: String,
    pub lyric_lines: usize,
    pub tokens: usize,
    pub credits: usize,
    pub corrections_restored: usize,
    /// 落盘的 .lrc 路径
    pub lrc_path: String,
}

/// 库里还没有歌词的歌。
///
/// 判据是 `utterances` 里一行都没有，而不是 `source_file` 为空：
/// 存量库里 `source_file` 全是 NULL（Python 侧没写过这一列），
/// 按它来判断会把 206 首有歌词的歌全都当成缺歌词。
pub fn missing(conn: &Connection) -> Result<Vec<MissingLyrics>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.title, s.artist, s.duration_sec, COALESCE(s.audio_path,'') \
         FROM songs s \
         WHERE NOT EXISTS (SELECT 1 FROM utterances u WHERE u.song_id = s.id) \
         ORDER BY s.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<f64>>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (song_id, title, artist, duration_sec, audio_path) = row?;
        let sibling_lrc = sibling_lrc(&audio_path).map(|p| p.display().to_string());
        out.push(MissingLyrics {
            song_id,
            title,
            artist,
            duration_sec,
            audio_path,
            sibling_lrc,
        });
    }
    Ok(out)
}

/// 音频旁边的同名 .lrc，有才返回。
///
/// 导入时找的就是这个（见 `jp_import::scan`）。在这里再找一次是为了
/// 「导入完才去下歌词」的情况：文件后来才出现，没必要为此重导一遍歌。
pub fn sibling_lrc(audio_path: &str) -> Option<PathBuf> {
    if audio_path.is_empty() {
        return None;
    }
    let candidate = Path::new(audio_path).with_extension("lrc");
    candidate.is_file().then_some(candidate)
}

/// 把一段 LRC 文本挂到一首已经在库里的歌上。三条来源的共同出口。
///
/// 先落盘 `raw/lyrics_lrc/{song_id}.lrc`，再在**一个事务**里清掉旧歌词、挂上新的。
/// `clear_lyrics` 让这个函数是可重入的：同一首歌换一份歌词，不会留下两份歌词行，
/// 也不会在全文索引里留下搜得到、点不开的幽灵行。
///
/// 用户校正过的分词不会因为换歌词而丢——校正按「歌手 + 曲名 + 原文」匹配，
/// `attach_lyrics` 末尾会把它们套回新的行号上。
/// 这首歌是谁的哪一首。
///
/// 三样东西总是一起走：id 决定落盘的文件名，歌手 + 曲名决定分词校正按什么匹配。
/// 分开当三个 `&str` 传的话，顺序记错了编译器也不会拦（三个都是 `&str`）。
#[derive(Debug, Clone, Copy)]
pub struct SongRef<'a> {
    pub id: &'a str,
    pub artist: &'a str,
    pub title: &'a str,
}

pub fn apply_lrc_text(
    conn: &mut Connection,
    lyrics_dir: &Path,
    song: SongRef<'_>,
    lrc_text: &str,
    source: LyricsSource,
    matched: &str,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Attached> {
    apply_lrc_inner(conn, lyrics_dir, song, lrc_text, source, matched, analyzer, None, None, None)
}

#[allow(clippy::too_many_arguments)]
fn apply_lrc_inner(
    conn: &mut Connection,
    lyrics_dir: &Path,
    song: SongRef<'_>,
    lrc_text: &str,
    source: LyricsSource,
    matched: &str,
    analyzer: Option<&jp_tokenizer::Analyzer>,
    saved: Option<&[crate::timed_lyrics::SavedLine]>,
    restored_from: Option<i64>,
    original: Option<(&str,&str)>,
) -> Result<Attached> {
    let SongRef { id: song_id, artist, title } = song;
    if lrc_text.trim().is_empty() {
        bail!("歌词是空的");
    }
    std::fs::create_dir_all(lyrics_dir)
        .with_context(|| format!("建不出歌词目录：{}", lyrics_dir.display()))?;
    let dest = lyrics_dir.join(format!("{song_id}.lrc"));
    let tx = conn.transaction()?;
    crate::timed_lyrics::archive_before(&tx, &crate::timed_lyrics::snapshot(&tx, song_id)?)?;
    jp_import::maintain::clear_lyrics(&tx, song_id)?;
    let stats = if let Some(lines) = saved {
        let stats = jp_import::attach_lines(&tx, song_id, lrc_text, crate::timed_lyrics::saved_lines(lines), &source.lineage(matched), analyzer, artist, title)?;
        crate::timed_lyrics::restore_timings(&tx, song_id, lines)?;
        stats
    } else {
        jp_import::attach_lyrics_text(&tx, song_id, lrc_text, &source.lineage(matched), analyzer, artist, title)?
    };
    if stats.lyric_lines == 0 {
        // 解析出 0 行：这份文件不是歌词（空文件、纯标签、纯时间轴）。
        // 回滚，让这首歌继续算「缺歌词」——否则它会从补齐名单里消失，
        // 而用户永远不知道为什么还是没有歌词。
        bail!("这份歌词解析出 0 行，没有改动");
    }
    if let Some(version) = restored_from {
        tx.execute("UPDATE lyric_versions SET (raw_format,source_raw,sync_granularity,timing_origin,timing_validation_json) = (SELECT raw_format,source_raw,sync_granularity,timing_origin,timing_validation_json FROM lyric_versions WHERE id=?2 AND song_id=?1) WHERE song_id=?1 AND is_current=1",params![song_id,version])?;
    }
    if let Some((format,raw)) = original {
        tx.execute("UPDATE lyric_versions SET raw_format=?2,source_raw=?3 WHERE song_id=?1 AND is_current=1",params![song_id,format,raw])?;
    }
    // source_file 记一笔来处。存量库这一列是空的，不靠它判断有无歌词（见 `missing`），
    // 但重装、迁库时它是唯一能回答「这份歌词哪来的」的东西。
    tx.execute(
        "UPDATE songs SET source_file=?2 WHERE id=?1",
        params![song_id, dest.display().to_string()],
    )?;
    crate::timed_lyrics::commit_with_file(tx, &dest, lrc_text.as_bytes())?;

    Ok(Attached {
        song_id: song_id.to_string(),
        source,
        matched: matched.to_string(),
        lyric_lines: stats.lyric_lines,
        tokens: stats.tokens,
        credits: stats.credits,
        corrections_restored: stats.corrections_restored,
        lrc_path: dest.display().to_string(),
    })
}

/// 界面上逐行编辑之后的一行。
#[derive(Debug, Clone, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditedLine {
    pub time_sec: Option<f64>,
    pub text: String,
}

/// 把逐行编辑的结果存成新的一版歌词。
///
/// 改错字、补漏的行、删重复贴的行、挪时间戳都走这里。和换一份歌词是同一件事：
/// 清掉旧行、挂上新行、重新分词、把校正过的分词按原文套回去（没改的行校正不丢），
/// 旧版原文留在 `lyric_versions` 里能恢复。另外记一条 `lyrics_edited` 修订，
/// 存改动前后的全部行，修订历史里看得出改了什么。
///
/// 行**不经过 LRC 解析器**（它会按时间重排、丢掉像署名的行）；
/// 原文记的是这些行写成的 LRC，落盘的 `raw/lyrics_lrc/{id}.lrc` 也换成它。
pub fn save_edited_lines(
    conn: &mut Connection,
    lyrics_dir: &Path,
    song_id: &str,
    lines: &[EditedLine],
    note: &str,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Attached> {
    let mut clean = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let text = line.text.trim();
        if text.is_empty() {
            continue; // 空行不是歌词
        }
        if let Some(t) = line.time_sec
            && !(t.is_finite() && t >= 0.0)
        {
            bail!("第 {} 行的时间不对：{t}", idx + 1);
        }
        if text.contains('\n') {
            bail!("第 {} 行里有换行；一行歌词请拆成两行", idx + 1);
        }
        clean.push(jp_import::lrc::LyricLine::new(line.time_sec, text));
    }
    if clean.is_empty() {
        bail!("一行歌词都没有；要删掉歌词请用「换歌词」");
    }
    let (artist, title) = song_names(conn, song_id)?;
    let before: Vec<(Option<f64>, String)> = {
        let mut stmt = conn.prepare("SELECT time_sec, text FROM utterances WHERE song_id=?1 ORDER BY line_idx, id")?;
        stmt.query_map(params![song_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?
    };
    let after: Vec<(Option<f64>, String)> = clean.iter().map(|l| (l.time_sec, l.text.clone())).collect();
    if before == after {
        bail!("没有改动");
    }

    let raw = jp_import::lrc::format(&clean);
    let timing_before = crate::timed_lyrics::snapshot(conn, song_id)?;
    std::fs::create_dir_all(lyrics_dir)
        .with_context(|| format!("建不出歌词目录：{}", lyrics_dir.display()))?;
    let dest = lyrics_dir.join(format!("{song_id}.lrc"));

    let tx = conn.transaction()?;
    crate::timed_lyrics::archive_before(&tx, &crate::timed_lyrics::snapshot(&tx, song_id)?)?;
    jp_import::maintain::clear_lyrics(&tx, song_id)?;
    let source = LyricsSource::Edited.lineage(note.trim());
    let stats = jp_import::attach_lines(&tx, song_id, &raw, clean, &source, analyzer, &artist, &title)?;
    crate::timed_lyrics::preserve_unchanged(&tx, &timing_before)?;
    let version_id: Option<i64> = tx.query_row(
        "SELECT id FROM lyric_versions WHERE song_id=?1 AND is_current=1",
        params![song_id],
        |r| r.get(0),
    )?;
    let rows = |v: &[(Option<f64>, String)]| serde_json::to_value(v).unwrap_or_default();
    jp_corpus::lineage::log(
        &tx,
        &jp_corpus::lineage::Revision {
            song_id: song_id.into(),
            kind: "lyrics_edited".into(),
            version_id,
            before: Some(rows(&before)),
            after: Some(rows(&after)),
            actor: "user".into(),
            note: note.trim().into(),
            ..Default::default()
        },
    )?;
    tx.execute("UPDATE songs SET source_file=?2 WHERE id=?1", params![song_id, dest.display().to_string()])?;
    // 先落盘再提交：文件写不进去就整个回滚，库和盘上的 .lrc 不会对不上
    crate::timed_lyrics::commit_with_file(tx, &dest, raw.as_bytes())?;

    Ok(Attached {
        song_id: song_id.to_string(),
        source: LyricsSource::Edited,
        matched: note.trim().to_string(),
        lyric_lines: stats.lyric_lines,
        tokens: stats.tokens,
        credits: stats.credits,
        corrections_restored: stats.corrections_restored,
        lrc_path: dest.display().to_string(),
    })
}

/// 打轴页里的一个时间单位：这一行第 `char_start..char_end` 个字从 `start_sec` 唱到 `end_sec`。
/// 还没打到结尾的单位 `end_sec` 为空。
#[derive(Debug, Clone, Copy, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimedUnit {
    pub char_start: usize,
    pub char_end: usize,
    pub start_sec: f64,
    pub end_sec: Option<f64>,
}

/// 打轴页存回来的一行。正文必须和库里现在的这一行一模一样：打轴只改时间。
#[derive(Debug, Clone, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimedEditLine {
    pub time_sec: Option<f64>,
    pub text: String,
    #[serde(default)]
    pub units: Vec<TimedUnit>,
}

/// 时间统一到毫秒：落盘的增强 LRC 是毫秒精度，库里的数也取同一个值，两边不会差零点几毫秒。
fn to_ms(sec: f64) -> f64 {
    (sec * 1000.0).round() / 1000.0
}

fn lrc_stamp(sec: f64) -> String {
    let ms = (sec.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

/// 写成增强 LRC：`[行首]<字开始>字<字结束>`。结束和下一个字的开始相同时只写一个时间；
/// 中间有停顿就写 `<结束><开始>` 两个，和 `jp_import::lrc::parse` 读回来的完全一致。
fn enhanced_lrc(lines: &[jp_import::lrc::LyricLine]) -> String {
    let mut out = String::new();
    for line in lines {
        if let Some(t) = line.time_sec {
            out.push_str(&format!("[{}]", lrc_stamp(t)));
        }
        let mut marks: Vec<(usize, u8, f64)> = Vec::new();
        for (i, w) in line.words.iter().enumerate() {
            let joined = i > 0 && line.words[i - 1].end == w.start && line.words[i - 1].end_sec == Some(w.time_sec);
            if !joined {
                marks.push((w.start, 1, w.time_sec));
            }
            if let Some(end) = w.end_sec {
                marks.push((w.end, 0, end));
            }
        }
        // 同一个位置先写结束再写开始
        marks.sort_by_key(|m| (m.0, m.1));
        let mut marks = marks.into_iter().peekable();
        for (at, ch) in line.text.chars().enumerate() {
            while let Some((_, _, time)) = marks.next_if(|m| m.0 == at) {
                out.push_str(&format!("<{}>", lrc_stamp(time)));
            }
            out.push(ch);
        }
        for (_, _, time) in marks {
            out.push_str(&format!("<{}>", lrc_stamp(time)));
        }
        out.push('\n');
    }
    out
}

/// 检查一行的时间单位能不能存：范围在行内、按位置排好、互不重叠、时间有效且不倒退。
/// **不要求覆盖整行**——打了一半的行也能存，播放时这一行照旧整行高亮，不补造时间。
fn check_units(line_no: usize, text: &str, units: &[TimedUnit]) -> Result<()> {
    let len = text.chars().count();
    let mut cursor = 0usize;
    let mut last = f64::NEG_INFINITY;
    for unit in units {
        if unit.char_start >= unit.char_end || unit.char_end > len {
            bail!("第 {line_no} 行有一个时间单位超出了这一行的文字");
        }
        if unit.char_start < cursor {
            bail!("第 {line_no} 行的时间单位有重叠");
        }
        if !(unit.start_sec.is_finite() && unit.start_sec >= 0.0) {
            bail!("第 {line_no} 行有一个开始时间不对：{}", unit.start_sec);
        }
        if unit.start_sec < last {
            bail!("第 {line_no} 行的时间倒退了：后面的字比前面的字先唱");
        }
        if let Some(end) = unit.end_sec {
            if !(end.is_finite() && end > unit.start_sec) {
                bail!("第 {line_no} 行有一个字的结束不晚于开始");
            }
            last = end;
        } else {
            last = unit.start_sec;
        }
        cursor = unit.char_end;
    }
    Ok(())
}

/// 把打轴结果存成新的一版歌词。
///
/// 只改时间，不改字：行数和每行正文必须和库里现在的一样，否则说明打开打轴之后歌词换过了，拒绝。
/// 和逐行编辑走同一条路（[`save_edited_lines`]）：旧版进历史可恢复、重新分词、校正按原文套回；
/// 来源记 `manual_edit`，所以批量补逐字歌词不会覆盖它。
///
/// 每行的逐字时间写进增强 LRC（落盘的 `.lrc` 也是它），入库后就是普通的 `lrc` 单位，
/// 播放器按原来的规则验证，不需要另一套时间轴。有单位的行，行首时间取第一个字的开始。
/// 以前只记了起点的手动校准（查词跳转用）原样保留。
pub fn save_timed_lines(
    conn: &mut Connection,
    lyrics_dir: &Path,
    song_id: &str,
    lines: &[TimedEditLine],
    note: &str,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Attached> {
    let current: Vec<(i64, Option<f64>, String)> = {
        let mut stmt = conn.prepare("SELECT id, time_sec, text FROM utterances WHERE song_id=?1 ORDER BY line_idx, id")?;
        stmt.query_map(params![song_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?
    };
    if current.is_empty() {
        bail!("这首歌还没有歌词，没法打轴");
    }
    if current.len() != lines.len() || current.iter().zip(lines).any(|(c, l)| c.2 != l.text) {
        bail!("打开打轴之后歌词改过了，请关掉打轴重新打开再存");
    }

    let mut clean = Vec::with_capacity(lines.len());
    for (idx, line) in lines.iter().enumerate() {
        if let Some(t) = line.time_sec
            && !(t.is_finite() && t >= 0.0)
        {
            bail!("第 {} 行的时间不对：{t}", idx + 1);
        }
        let units: Vec<TimedUnit> = line
            .units
            .iter()
            .map(|u| TimedUnit { start_sec: to_ms(u.start_sec), end_sec: u.end_sec.map(to_ms), ..*u })
            .collect();
        check_units(idx + 1, &line.text, &units)?;
        let words: Vec<jp_import::lrc::WordTime> = units
            .iter()
            .map(|u| jp_import::lrc::WordTime { start: u.char_start, end: u.char_end, time_sec: u.start_sec, end_sec: u.end_sec })
            .collect();
        let time_sec = match words.first() {
            Some(first) => Some(first.time_sec),
            None => line.time_sec.map(to_ms),
        };
        let mut lyric = jp_import::lrc::LyricLine::new(time_sec, line.text.clone());
        lyric.words = words;
        clean.push(lyric);
    }

    let before: Vec<(Option<f64>, String)> = current.iter().map(|(_, t, x)| (*t, x.clone())).collect();
    let after: Vec<(Option<f64>, String)> = clean.iter().map(|l| (l.time_sec, l.text.clone())).collect();
    let raw = enhanced_lrc(&clean);
    let units: usize = clean.iter().map(|l| l.words.len()).sum();
    // 只记了起点的手动校准：按行号原样带过去（正文没变，行号就是同一行）
    let anchors: Vec<(usize, jp_corpus::word_timing::WordTiming)> = {
        let ids: Vec<i64> = current.iter().map(|c| c.0).collect();
        jp_corpus::word_timing::for_utterances(conn, &ids)?
            .into_iter()
            .filter(|t| t.source == jp_corpus::word_timing::source::MANUAL && t.end_sec.is_none())
            .filter_map(|t| ids.iter().position(|id| *id == t.utterance_id).map(|i| (i, t)))
            .collect()
    };
    let (artist, title) = song_names(conn, song_id)?;
    std::fs::create_dir_all(lyrics_dir)
        .with_context(|| format!("建不出歌词目录：{}", lyrics_dir.display()))?;
    let dest = lyrics_dir.join(format!("{song_id}.lrc"));

    let tx = conn.transaction()?;
    crate::timed_lyrics::archive_before(&tx, &crate::timed_lyrics::snapshot(&tx, song_id)?)?;
    jp_import::maintain::clear_lyrics(&tx, song_id)?;
    let note = if note.trim().is_empty() { "手动打轴" } else { note.trim() };
    let source = LyricsSource::Edited.lineage(note);
    let stats = jp_import::attach_lines(&tx, song_id, &raw, clean, &source, analyzer, &artist, &title)?;
    let ids: Vec<i64> = tx
        .prepare("SELECT id FROM utterances WHERE song_id=?1 ORDER BY line_idx, id")?
        .query_map(params![song_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for (index, anchor) in anchors {
        if let Some(id) = ids.get(index) {
            jp_corpus::word_timing::set_manual(&tx, *id, anchor.char_start, anchor.char_end, anchor.start_sec)?;
        }
    }
    let report = serde_json::json!({"status":"manual", "algorithmGenerated":false, "unitCount":units});
    tx.execute(
        "UPDATE lyric_versions SET timing_origin='manual', timing_validation_json=?2 WHERE song_id=?1 AND is_current=1",
        params![song_id, report.to_string()],
    )?;
    let version_id: Option<i64> = tx.query_row(
        "SELECT id FROM lyric_versions WHERE song_id=?1 AND is_current=1",
        params![song_id],
        |r| r.get(0),
    )?;
    let rows = |v: &[(Option<f64>, String)]| serde_json::to_value(v).unwrap_or_default();
    jp_corpus::lineage::log(
        &tx,
        &jp_corpus::lineage::Revision {
            song_id: song_id.into(),
            kind: "lyrics_timed".into(),
            version_id,
            before: Some(rows(&before)),
            after: Some(rows(&after)),
            actor: "user".into(),
            note: note.into(),
            ..Default::default()
        },
    )?;
    tx.execute("UPDATE songs SET source_file=?2 WHERE id=?1", params![song_id, dest.display().to_string()])?;
    crate::timed_lyrics::commit_with_file(tx, &dest, raw.as_bytes())?;

    Ok(Attached {
        song_id: song_id.to_string(),
        source: LyricsSource::Edited,
        matched: note.to_string(),
        lyric_lines: stats.lyric_lines,
        tokens: stats.tokens,
        credits: stats.credits,
        corrections_restored: stats.corrections_restored,
        lrc_path: dest.display().to_string(),
    })
}

/// 把这首歌以前的某一版歌词恢复成当前版。
///
/// 恢复也是「挂一版新的」（来源记 restored）：历史只追加，回到旧版不会抹掉中间那几版。
/// 老库补登记的版本没留原文，恢复不了。
pub fn restore_version(
    conn: &mut Connection,
    lyrics_dir: &Path,
    song_id: &str,
    version_id: i64,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Attached> {
    let (owner, raw) = jp_corpus::lineage::version_raw(conn, version_id)?
        .with_context(|| format!("找不到歌词版本 {version_id}"))?;
    if owner != song_id {
        bail!("歌词版本 {version_id} 不是这首歌的");
    }
    let Some(raw) = raw else {
        bail!("这一版是老库补登记的，当时没有保存原文，恢复不了");
    };
    let version_no: i64 = conn.query_row(
        "SELECT version_no FROM lyric_versions WHERE id=?1",
        params![version_id],
        |r| r.get(0),
    )?;
    let (artist, title) = song_names(conn, song_id)?;
    let saved = crate::timed_lyrics::version_timings(conn, song_id, version_id)?;
    apply_lrc_inner(
        conn,
        lyrics_dir,
        SongRef { id: song_id, artist: &artist, title: &title },
        &raw,
        LyricsSource::Restored,
        &format!("第 {version_no} 版"),
        analyzer,
        saved.as_deref(),
        Some(version_id),
        None,
    )
}

/// 用户自己选的一份 .lrc / .txt。
///
/// 读进来再按 UTF-8 写出去（`jp_import::parse_lrc_file` 会处理 Shift-JIS / GBK），
/// **不是**直接复制字节：落盘那一份要能被下一次重建语料库原样读回来。
pub fn attach_from_file(
    conn: &mut Connection,
    lyrics_dir: &Path,
    song_id: &str,
    path: &Path,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Attached> {
    let (artist, title) = song_names(conn, song_id)?;
    let converted = crate::timed_lyrics::file_to_lrc(path)?;
    let text = match &converted {
        Some(file) => file.text.clone(),
        None => read_text(path)?,
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    apply_lrc_inner(
        conn,
        lyrics_dir,
        SongRef { id: song_id, artist: &artist, title: &title },
        &text,
        LyricsSource::Manual,
        &name,
        analyzer,
        None,
        None,
        converted.as_ref().map(|f|(f.format.as_str(),f.original.as_str())),
    )
}

/// 读文本，编码按 UTF-8 → CP932 → GBK 试，和 `jp_import::lrc::parse_file` 同一套规则。
/// 带 BOM 的 UTF-16 也认：`Encoding::decode` 先嗅 BOM（见 `jp_import::lrc` 里 `decode` 的注释）。
pub(crate) fn read_text(path: &Path) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("读不了歌词文件：{}", path.display()))?;
    if let Ok(text) = std::str::from_utf8(&bytes) {
        return Ok(text.trim_start_matches('\u{feff}').to_string());
    }
    for encoding in [encoding_rs::SHIFT_JIS, encoding_rs::GBK] {
        let (text, _, had_errors) = encoding.decode(&bytes);
        if !had_errors {
            return Ok(text.trim_start_matches('\u{feff}').to_string());
        }
    }
    bail!(
        "{} 的编码认不出来（试过 UTF-8、CP932、GBK）。请另存为 UTF-8 再导入——\
         硬读会得到一整份乱码歌词，比没有歌词更糟",
        path.display()
    )
}

fn song_names(conn: &Connection, song_id: &str) -> Result<(String, String)> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT artist, title FROM songs WHERE id=?1",
        params![song_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()?
    .with_context(|| format!("找不到曲目 {song_id}"))
}

/// 一首歌走一遍「先本地、再在线」：内嵌的 → 旁边的 .lrc → 在线搜。
///
/// 本地优先是因为它必然对：文件就在那首歌旁边，不存在挑错的风险，还不用联网。
/// 内嵌和外挂之间的取舍和导入时一样（见 `jp_import::ScannedTrack::lyrics_pick`）：
/// 内嵌的优先，只有它没时间轴、旁边又有 .lrc 时让给 .lrc。
pub fn fill_one(
    conn: &mut Connection,
    lyrics_dir: &Path,
    provider: &jp_scraper::lyrics::NeteaseLyrics,
    target: &MissingLyrics,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Option<Attached>> {
    let sibling = sibling_lrc(&target.audio_path);
    let embedded = (!target.audio_path.is_empty())
        .then(|| jp_import::embedded::read_lyrics(Path::new(&target.audio_path)))
        .flatten()
        .filter(|e| e.synced || sibling.is_none());
    if let Some(embedded) = embedded {
        let name = Path::new(&target.audio_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        return Ok(Some(apply_lrc_text(
            conn,
            lyrics_dir,
            SongRef { id: &target.song_id, artist: &target.artist, title: &target.title },
            &embedded.text,
            LyricsSource::Embedded,
            &name,
            analyzer,
        )?));
    }
    if let Some(local) = sibling {
        let text = read_text(&local)?;
        let name = local
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        return Ok(Some(apply_lrc_text(
            conn,
            lyrics_dir,
            SongRef { id: &target.song_id, artist: &target.artist, title: &target.title },
            &text,
            LyricsSource::Sibling,
            &name,
            analyzer,
        )?));
    }

    let found = provider
        .best_lyrics(&target.title, &target.artist, target.duration_sec)
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    let Some((hit, text)) = found else {
        return Ok(None);
    };
    let matched = format!("{} — {}", hit.title, hit.artist);
    Ok(Some(apply_lrc_text(
        conn,
        lyrics_dir,
        SongRef { id: &target.song_id, artist: &target.artist, title: &target.title },
        &text,
        LyricsSource::Netease,
        &matched,
        analyzer,
    )?))
}

/// 搜歌词用的客户端。
///
/// 限流 600ms：一首歌两次请求（搜索 + 取歌词），整库两百首约三分半。
/// 这是个公开接口，不是给我们用的，排队比被封好。
pub fn provider() -> jp_scraper::lyrics::NeteaseLyrics {
    let client = jp_scraper::HttpClient::new(Box::new(jp_scraper::UreqTransport))
        .with_rate_limiter(Arc::new(jp_scraper::RateLimiter::new(
            std::time::Duration::from_millis(600),
        )));
    jp_scraper::lyrics::NeteaseLyrics::new(client)
}

/// 一次批量补齐的进度。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsProgress {
    pub done: usize,
    pub total: usize,
    pub song_id: String,
    pub title: String,
    /// filled / notFound / failed
    pub status: String,
    /// 这首歌的歌词哪来的（失败和没找到时是空串）
    pub source: String,
    pub matched: String,
    pub lyric_lines: usize,
    /// **这一首**的失败原因，成功时是空串
    pub message: String,
    /// 作业**整体**断在半路的原因，正常结束时是空串。
    ///
    /// 和 `message` 分开是因为原来它俩是同一个字段：整批黄了和某一首没补上
    /// 在界面看来一模一样，而这两件事该说的话完全不同。
    /// 名字和刮削、Anki、词典三处统一。
    pub error: String,
    // 跑到这一步的累计数，省得前端自己攒
    pub filled: usize,
    pub not_found: usize,
    pub failed: usize,
    pub finished: bool,
    pub cancelled: bool,
}

pub type LyricsJob = crate::job::Job;

/// 在后台线程补一批歌词。立刻返回，进度走 `lyrics://progress` 事件。
///
/// **自己开一条数据库连接**，和刮削一样的理由：占着 UI 那条连接几分钟的话，
/// 期间所有查询都会卡住。分词器也在线程里单独建一份——`Analyzer` 借不出
/// `'static` 的引用，而重建一次只要一秒多。
pub fn spawn_fill(
    report: impl Fn(&LyricsProgress) + Send + Sync + 'static,
    job: Arc<LyricsJob>,
    db_path: PathBuf,
    lyrics_dir: PathBuf,
    song_ids: Option<Vec<String>>,
) -> Result<()> {
    // 线程崩了也要让界面收一条终态，否则进度条永远转着（见 `crate::job`）
    let report = Arc::new(report);
    let crashed = Arc::clone(&report);
    let guard = job
        .start_with(move || {
            crashed(&
                LyricsProgress {
                    finished: true,
                    error: "补齐歌词的后台线程崩了，详情见日志".into(),
                    ..Default::default()
                },
            );
        })
        .ok_or_else(|| anyhow::anyhow!("已经有一个补齐歌词的作业在跑"))?;

    std::thread::spawn(move || {
        // 收尾事件里要报的那几个数。闭包里每处理一首就更新，
        // 中途出错也还是这一份——**不能在收尾时另起一个 default()**，
        // 否则用户看到的汇总永远是「补上 0，挑不出来 0」。
        let mut tally = LyricsProgress::default();
        let outcome = (|| -> Result<bool> {
            let mut conn = rusqlite::Connection::open(&db_path)?;
            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

            let mut targets = missing(&conn)?;
            if let Some(wanted) = &song_ids {
                targets.retain(|t| wanted.contains(&t.song_id));
            }
            tally.total = targets.len();

            let project_root = db_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let analyzer = jp_tokenizer::locate_sudachipy(&project_root)
                .and_then(|(res, dict)| jp_tokenizer::Analyzer::from_sudachipy(&res, &dict).ok());
            if analyzer.is_none() {
                eprintln!("补齐歌词：没有分词器，歌词会入库但不分词");
            }
            let provider = provider();

            for target in &targets {
                if job.cancelled() {
                    return Ok(true);
                }
                let result = fill_one(
                    &mut conn,
                    &lyrics_dir,
                    &provider,
                    target,
                    analyzer.as_ref(),
                );
                tally.done += 1;
                let progress = match result {
                    Ok(Some(attached)) => {
                        tally.filled += 1;
                        LyricsProgress {
                            status: "filled".into(),
                            source: attached.source.as_str().into(),
                            matched: attached.matched,
                            lyric_lines: attached.lyric_lines,
                            ..Default::default()
                        }
                    }
                    Ok(None) => {
                        tally.not_found += 1;
                        LyricsProgress {
                            status: "notFound".into(),
                            ..Default::default()
                        }
                    }
                    // 一首歌失败不该中断整批：网络会抽，个别文件会坏
                    Err(err) => {
                        tally.failed += 1;
                        LyricsProgress {
                            status: "failed".into(),
                            message: format!("{err:#}"),
                            ..Default::default()
                        }
                    }
                };
                report(&
                    LyricsProgress {
                        done: tally.done,
                        total: tally.total,
                        song_id: target.song_id.clone(),
                        title: target.title.clone(),
                        filled: tally.filled,
                        not_found: tally.not_found,
                        failed: tally.failed,
                        ..progress
                    },
                );
            }
            Ok(false)
        })();

        let (cancelled, error) = match outcome {
            Ok(cancelled) => (cancelled, String::new()),
            Err(err) => {
                eprintln!("补齐歌词作业中断: {err:#}");
                // 整批断了要说出来，不能只是悄悄「完成」
                (false, format!("{err:#}"))
            }
        };
        // 先放开运行权再发终态：顺序和原来的 `job.finish()` 一致，
        // 免得界面收到「完成」时再问 `lyrics_fill_running` 还答「在跑」
        drop(guard);
        report(&
            LyricsProgress {
                done: tally.done,
                total: tally.total,
                finished: true,
                cancelled,
                error,
                ..tally
            },
        );
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 和真库同构：补齐歌词碰得到的表。
    ///
    /// **走 `jp_corpus` 的 `schema.sql`，不要在这里手抄建表语句**——
    /// 手抄的副本漂了不会报错，只会让测试在一个和生产不同的形状上通过。
    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        jp_corpus::Corpus::ensure_schema_on(&conn).unwrap();
        conn.execute(
            "INSERT INTO songs (id, title, artist, audio_path, duration_sec) \
             VALUES ('001','ネイティブダンサー','サカナクション','E:/music/a.flac', 264.2)",
            [],
        )
        .unwrap();
        conn
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jp-app-lyrics-{}-{:?}-{name}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn manual_timing_becomes_a_protected_version_the_player_accepts() {
        let mut conn = db();
        let dir = scratch("timing");
        apply_lrc_text(
            &mut conn,
            &dir,
            SongRef { id: "001", artist: "サカナクション", title: "ネイティブダンサー" },
            "[00:10.00]夜に駆ける\n[00:20.00]ねえ\n",
            LyricsSource::Manual,
            "x.lrc",
            None,
        )
        .unwrap();
        let ids: Vec<i64> = conn
            .prepare("SELECT id FROM utterances WHERE song_id='001' ORDER BY line_idx")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        // 查词跳转用的起点校准：打轴后必须还在
        jp_corpus::word_timing::set_manual(&conn, ids[1], 0, 2, 20.25).unwrap();

        let unit = |s: usize, e: usize, a: f64, b: Option<f64>| TimedUnit { char_start: s, char_end: e, start_sec: a, end_sec: b };
        let lines = vec![
            TimedEditLine {
                time_sec: Some(10.0),
                text: "夜に駆ける".into(),
                // 第二、三个单位之间停了 0.2 秒
                units: vec![unit(0, 1, 10.1004, Some(10.4)), unit(1, 2, 10.4, Some(10.6)), unit(2, 5, 10.8, Some(11.5))],
            },
            TimedEditLine { time_sec: Some(20.0), text: "ねえ".into(), units: vec![] },
        ];
        save_timed_lines(&mut conn, &dir, "001", &lines, "", None).unwrap();

        let on_disk = std::fs::read_to_string(dir.join("001.lrc")).unwrap();
        assert_eq!(on_disk, "[00:10.100]<00:10.100>夜<00:10.400>に<00:10.600><00:10.800>駆ける<00:11.500>\n[00:20.000]ねえ\n");
        // 落盘的文件读回来就是存进去的时间
        let parsed = jp_import::lrc::parse(&on_disk);
        let words: Vec<_> = parsed.lines[0].words.iter().map(|w| (w.start, w.end, w.time_sec, w.end_sec)).collect();
        assert_eq!(words, vec![(0, 1, 10.1, Some(10.4)), (1, 2, 10.4, Some(10.6)), (2, 5, 10.8, Some(11.5))]);

        let first: Option<f64> = conn.query_row("SELECT time_sec FROM utterances WHERE song_id='001' AND line_idx=0", [], |r| r.get(0)).unwrap();
        assert_eq!(first, Some(10.1), "行首取第一个字的开始");
        let shown = jp_corpus::display_timeline::for_song(&conn, "001").unwrap();
        assert_eq!(shown[0].mode, jp_corpus::display_timeline::DisplayMode::Karaoke);
        assert_eq!(shown[0].units.len(), 3);
        let timings = jp_corpus::word_timing::for_song(&conn, "001").unwrap();
        assert!(timings.iter().any(|t| t.utterance_id == shown[1].utterance_id && t.source == "manual" && t.start_sec == 20.25));

        let versions = jp_corpus::lineage::versions(&conn, "001").unwrap();
        assert_eq!(versions[0].source_type, "manual_edit");
        assert_eq!(versions[0].timing_origin, "manual");
        assert!(crate::lyric_upgrade::protected(&conn, "001").unwrap(), "批量补逐字不能覆盖手动打轴");
        assert_eq!(jp_corpus::lineage::revisions(&conn, "001", 5).unwrap()[0].kind, "lyrics_timed");

        // 正文变了、单位重叠、时间倒退：都拒绝，库不动
        let count = versions.len();
        let mut changed = lines.clone();
        changed[1].text = "ねぇ".into();
        assert!(save_timed_lines(&mut conn, &dir, "001", &changed, "", None).unwrap_err().to_string().contains("歌词改过了"));
        let mut overlap = lines.clone();
        overlap[0].units = vec![unit(0, 2, 10.1, Some(10.4)), unit(1, 3, 10.4, Some(10.6))];
        assert!(save_timed_lines(&mut conn, &dir, "001", &overlap, "", None).is_err());
        let mut backwards = lines.clone();
        backwards[0].units = vec![unit(0, 1, 10.5, Some(10.6)), unit(1, 2, 10.4, None)];
        assert!(save_timed_lines(&mut conn, &dir, "001", &backwards, "", None).is_err());
        assert_eq!(jp_corpus::lineage::versions(&conn, "001").unwrap().len(), count);
    }

    #[test]
    fn a_song_without_lyrics_is_listed_and_disappears_once_filled() {
        let mut conn = db();
        let dir = scratch("fill");
        assert_eq!(missing(&conn).unwrap().len(), 1);

        let attached = apply_lrc_text(
            &mut conn,
            &dir,
            SongRef { id: "001", artist: "サカナクション", title: "ネイティブダンサー" },
            "作词 : 山口　一郎\n[00:15.00]踊りませんか\n[00:20.00]夜が終わるまで\n",
            LyricsSource::Netease,
            "ネイティブダンサー — サカナクション",
            None,
        )
        .unwrap();
        assert_eq!(attached.lyric_lines, 2);
        assert_eq!(attached.credits, 1, "LRC 里的作词没进 track_credits");
        assert!(missing(&conn).unwrap().is_empty(), "挂上歌词后还算缺歌词");

        // 落盘的那一份是给「可恢复」用的，必须真在
        let on_disk = dir.join("001.lrc");
        assert!(on_disk.is_file());
        assert!(std::fs::read_to_string(&on_disk).unwrap().contains("踊りませんか"));

        // 全文检索能搜到——外部内容表没有触发器，漏同步这里就是 0
        let hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM utterances_fts WHERE utterances_fts MATCH '踊りま'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1);
    }

    /// 换一份歌词：不能变成两份。
    #[test]
    fn applying_twice_replaces_instead_of_duplicating() {
        let mut conn = db();
        let dir = scratch("twice");
        let args = ("サカナクション", "ネイティブダンサー");
        for text in ["[00:15.00]一回目\n", "[00:15.00]二回目\n[00:20.00]もう一行\n"] {
            apply_lrc_text(
                &mut conn,
                &dir,
                SongRef { id: "001", artist: args.0, title: args.1 },
                text,
                LyricsSource::Manual,
                "x.lrc",
                None,
            )
            .unwrap();
        }
        let lines: Vec<String> = conn
            .prepare("SELECT text FROM utterances WHERE song_id='001' ORDER BY line_idx")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(lines, vec!["二回目", "もう一行"]);
        let ghosts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM utterances_fts WHERE utterances_fts MATCH '一回目'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ghosts, 0, "旧歌词还留在全文索引里");
    }

    #[test]
    fn edited_lines_become_a_new_version_exactly_as_typed() {
        let mut conn = db();
        let dir = scratch("edit");
        apply_lrc_text(
            &mut conn,
            &dir,
            SongRef { id: "001", artist: "サカナクション", title: "ネイティブダンサー" },
            "[00:10.00]夜に駆けるる\n[00:10.00]夜に駆けるる\n[00:20.00]作詞の夢\n",
            LyricsSource::Manual,
            "x.lrc",
            None,
        )
        .unwrap();

        let edited = [
            EditedLine { time_sec: Some(10.0), text: "夜に駆ける".into() }, // 改错字
            // 删掉重复贴的那一行
            EditedLine { time_sec: Some(25.0), text: "作詞：みたいな歌詞".into() }, // 像署名，解析器会吞，这里必须原样留
            EditedLine { time_sec: Some(15.0), text: "  ".into() },              // 空行丢掉
            EditedLine { time_sec: None, text: "（間奏）".into() },
        ];
        let done = save_edited_lines(&mut conn, &dir, "001", &edited, "改错字", None).unwrap();
        assert_eq!(done.lyric_lines, 3);
        let lines: Vec<(Option<f64>, String)> = conn
            .prepare("SELECT time_sec, text FROM utterances WHERE song_id='001' ORDER BY line_idx")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            lines,
            vec![
                (Some(10.0), "夜に駆ける".to_string()),
                (Some(25.0), "作詞：みたいな歌詞".to_string()),
                (None, "（間奏）".to_string())
            ],
            "顺序和内容原样，不经解析器"
        );

        let versions = jp_corpus::lineage::versions(&conn, "001").unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].source_type, "manual_edit");
        assert_eq!(versions[0].source_name, "改错字");
        let log = jp_corpus::lineage::revisions(&conn, "001", 5).unwrap();
        assert_eq!(log[0].kind, "lyrics_edited");
        assert_eq!(log[0].before.as_ref().unwrap().as_array().unwrap().len(), 3);
        assert_eq!(log[0].after.as_ref().unwrap()[0][1], "夜に駆ける");
        let on_disk = std::fs::read_to_string(dir.join("001.lrc")).unwrap();
        assert!(on_disk.starts_with("[00:10.00]夜に駆ける\n"), "{on_disk}");

        // 没改动、全是空行、时间不对：都拒绝，库不动
        assert!(save_edited_lines(&mut conn, &dir, "001", &edited, "", None).unwrap_err().to_string().contains("没有改动"));
        let blank = [EditedLine { time_sec: None, text: " ".into() }];
        assert!(save_edited_lines(&mut conn, &dir, "001", &blank, "", None).is_err());
        let bad = [EditedLine { time_sec: Some(-1.0), text: "あ".into() }];
        assert!(save_edited_lines(&mut conn, &dir, "001", &bad, "", None).is_err());
        assert_eq!(jp_corpus::lineage::versions(&conn, "001").unwrap().len(), 2);
    }

    /// 解析出 0 行的文件不能让这首歌「看起来有歌词了」——
    /// 它会从补齐名单里消失，而用户永远不知道为什么还是没词。
    #[test]
    fn a_file_that_parses_to_nothing_changes_nothing() {
        let mut conn = db();
        let dir = scratch("empty");
        let err = apply_lrc_text(
            &mut conn,
            &dir,
            SongRef { id: "001", artist: "サカナクション", title: "ネイティブダンサー" },
            "[ar:サカナクション]\n[ti:ネイティブダンサー]\n",
            LyricsSource::Manual,
            "tags-only.lrc",
            None,
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("0 行"), "{err:#}");
        assert_eq!(missing(&conn).unwrap().len(), 1, "这首歌还该在补齐名单里");
    }

    /// Shift-JIS 的歌词文件要能读对。硬按 UTF-8 读会得到一整份乱码，
    /// 而且因为「有歌词」了，补齐流程再也不会来看它。
    #[test]
    fn a_shift_jis_file_is_decoded_not_mangled() {
        let dir = scratch("sjis");
        let path = dir.join("sjis.lrc");
        std::fs::write(
            &path,
            b"\x5b\x30\x30\x3a\x31\x35\x2e\x30\x30\x5d\x8c\x4e\x82\xf0\x91\xd2\x82\xc1\x82\xc4\x82\xa2\x82\xe9\x0a",
        )
        .unwrap();
        assert_eq!(read_text(&path).unwrap().trim(), "[00:15.00]君を待っている");

        let mut conn = db();
        let attached = attach_from_file(&mut conn, &dir, "001", &path, None).unwrap();
        assert_eq!(attached.lyric_lines, 1);
        let text: String = conn
            .query_row("SELECT text FROM utterances WHERE song_id='001'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(text, "君を待っている");
    }

    /// 手动挑的歌词文件是记事本「Unicode」另存的（带 BOM 的 UTF-16 LE）。
    /// 本来就能读（encoding_rs 嗅 BOM），这条是钉住它，别哪天改成不嗅 BOM 的解码
    #[test]
    fn a_utf16_file_with_a_bom_is_decoded() {
        let dir = scratch("utf16");
        let path = dir.join("utf16.lrc");
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend("[00:15.00]君を待っている\n".encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(read_text(&path).unwrap().trim(), "[00:15.00]君を待っている");
    }

    #[test]
    fn a_sibling_lrc_is_found_next_to_the_audio() {
        let dir = scratch("sibling");
        let audio = dir.join("song.flac");
        std::fs::write(&audio, b"x").unwrap();
        assert!(sibling_lrc(&audio.display().to_string()).is_none());
        std::fs::write(dir.join("song.lrc"), "[00:01.00]あ\n").unwrap();
        assert!(sibling_lrc(&audio.display().to_string()).is_some());
        // 空路径（库里有音频丢了的歌）不该 panic
        assert!(sibling_lrc("").is_none());
    }
}
