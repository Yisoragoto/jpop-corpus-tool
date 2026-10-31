//! 歌词来源：一首歌的每份歌词各存一行，再记「现在用的是哪一份」。
//!
//! ## 为什么要分开存
//!
//! 以前一首歌只有「当前挂上的那份歌词」（`utterances` + `lyric_versions` 的当前版）。
//! 旁边的 .lrc、内嵌歌词、在线搜到的各是一条路，挂上就把上一份换掉——
//! 想知道「这首歌还有哪几份可选」「我是不是手动改过」只能翻版本历史。
//!
//! 现在每份来源一行（`lyric_sources`），原文、格式、更新时间、是不是用户改过的都在这一行；
//! 选了哪一份记在 `lyric_selection`。选中的那份仍然走原来的入库路径写进 `utterances`，
//! 分词、检索、制卡、分词校正一概不用改。
//!
//! ## 优先级
//!
//! 默认按 [`kind_rank`]：用户导入 / 编辑 / 恢复的 → 音频旁边的同名文件 → 音频内嵌 → 在线。
//! 同一类里有逐字时间的胜过逐行，逐行胜过纯文本。
//!
//! **用户的选择和编辑不会被自动流程覆盖**，靠的是两件事：
//! 1. 用户的歌词是单独的 `kind = 'user'` 行，自动扫描、在线更新只改它们自己那一行；
//! 2. `lyric_selection.chosen_by` 是 `user` 或 `legacy` 时，自动流程不换来源。
//!    `legacy` 是升级前就在库里的歌词：整库重挑会把几百首歌重新分词、行号全换，
//!    所以存量的歌保持原样，你自己在「歌词来源」里换。
//!
//! ## 偏移
//!
//! 偏移落库时直接加进 `utterances.time_sec` 和来源给的逐字时间（[`set_offset`]），
//! 原文不动。这样所有按时间截音频、跳转的地方（单句循环、KWIC 试听、制卡音频）
//! 都自动对齐，不用每处各自记得加偏移。手动校准的逐字时间是对着音频打的，不跟着平移。

use anyhow::{Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::lineage::{LyricSource, NOW};
use crate::quality::LineInput;

/// 偏移的上限（毫秒）。超过半分钟多半是挑错了版本，而不是要调同步。
pub const MAX_OFFSET_MS: i64 = 30_000;

/// 在线查「没找到」之后，多久内批量补齐不再去问（天）。
/// 单首手动重试不看这个。
pub const NOT_FOUND_RETRY_DAYS: i64 = 14;

/// `lyric_versions.source_type` → 来源类别。
pub fn kind_of(source_type: &str) -> &'static str {
    match source_type {
        "manual_file" | "manual_edit" | "restored" => "user",
        "lrc_file" | "sibling" => "sibling",
        "embedded" => "embedded",
        "online" => "online",
        _ => "legacy",
    }
}

/// 越小越优先。
pub fn kind_rank(kind: &str) -> u8 {
    match kind {
        "user" => 0,
        "sibling" => 1,
        "embedded" => 2,
        "online" => 3,
        _ => 4,
    }
}

fn precision_rank(precision: &str) -> u8 {
    match precision {
        "word" => 0,
        "line" => 1,
        _ => 2,
    }
}

/// 看内容猜格式。扩展名靠不住（不少下载站把 LRC 存成 .txt）。
pub fn sniff_format(raw: &str) -> &'static str {
    let mut lrc = false;
    for line in raw.lines().take(400) {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.contains("-->") && line.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return "srt";
        }
        if let Some(rest) = line.strip_prefix('[') {
            let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
            if digits >= 1 && rest[digits..].starts_with(':') {
                lrc = true;
                if line.contains('<') && line.contains('>') && line.contains(':') {
                    let inner = line.split('<').nth(1).unwrap_or("");
                    if inner.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                        return "enhanced_lrc";
                    }
                }
            }
        }
    }
    if lrc { "lrc" } else { "text" }
}

/// 一行来源。原文不送给界面（太长），要看用 [`raw_text`]。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRow {
    pub id: i64,
    pub song_id: String,
    pub kind: String,
    pub provider: String,
    pub source_ref: String,
    pub role: String,
    pub language: String,
    pub source_type: String,
    pub raw_format: String,
    pub has_raw: bool,
    pub content_hash: String,
    pub line_count: i64,
    pub timed_line_count: i64,
    pub precision: String,
    pub user_modified: bool,
    pub missing: bool,
    pub offset_ms: i64,
    pub updated_at: String,
}

/// 写一个来源时要给的东西。
#[derive(Debug, Clone, Default)]
pub struct SourceInput<'a> {
    pub song_id: &'a str,
    pub kind: &'a str,
    pub provider: &'a str,
    pub source_ref: &'a str,
    pub role: &'a str,
    pub language: &'a str,
    pub source_type: &'a str,
    pub raw_format: &'a str,
    pub raw_text: Option<&'a str>,
    pub content_hash: &'a str,
    pub line_count: i64,
    pub timed_line_count: i64,
    pub precision: &'a str,
    pub user_modified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub source_id: Option<i64>,
    pub translation_source_id: Option<i64>,
    pub translation_off: bool,
    /// user / auto / legacy
    pub chosen_by: String,
    /// 已经加进 `utterances.time_sec` 的偏移
    pub offset_ms: i64,
    pub updated_at: String,
}

const COLUMNS: &str = "id,song_id,kind,provider,source_ref,role,language,source_type,raw_format,raw_text IS NOT NULL,\
    content_hash,line_count,timed_line_count,precision,user_modified,missing,offset_ms,updated_at";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<SourceRow> {
    Ok(SourceRow {
        id: r.get(0)?,
        song_id: r.get(1)?,
        kind: r.get(2)?,
        provider: r.get(3)?,
        source_ref: r.get(4)?,
        role: r.get(5)?,
        language: r.get(6)?,
        source_type: r.get(7)?,
        raw_format: r.get(8)?,
        has_raw: r.get(9)?,
        content_hash: r.get(10)?,
        line_count: r.get(11)?,
        timed_line_count: r.get(12)?,
        precision: r.get(13)?,
        user_modified: r.get::<_, i64>(14)? != 0,
        missing: r.get::<_, i64>(15)? != 0,
        offset_ms: r.get(16)?,
        updated_at: r.get(17)?,
    })
}

fn table_exists(conn: &Connection) -> Result<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name='lyric_selection'", [], |_| Ok(()))
        .optional()?
        .is_some())
}

/// 一首歌的全部来源，按默认优先级排好（原文在前、译文在后）。
pub fn list(conn: &Connection, song_id: &str) -> Result<Vec<SourceRow>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM lyric_sources WHERE song_id=?1"))?;
    let mut rows: Vec<SourceRow> = stmt.query_map([song_id], row)?.collect::<rusqlite::Result<_>>()?;
    rows.sort_by_key(|r| (r.role != "original", kind_rank(&r.kind), precision_rank(&r.precision), std::cmp::Reverse(r.updated_at.clone()), r.id));
    Ok(rows)
}

pub fn get(conn: &Connection, source_id: i64) -> Result<Option<SourceRow>> {
    Ok(conn
        .query_row(&format!("SELECT {COLUMNS} FROM lyric_sources WHERE id=?1"), [source_id], row)
        .optional()?)
}

pub fn raw_text(conn: &Connection, source_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT raw_text FROM lyric_sources WHERE id=?1", [source_id], |r| r.get(0))
        .optional()?
        .flatten())
}

/// 写一个来源（同一首歌、同一类、同一 provider、同一角色和语言算同一行）。
///
/// 已有的行是用户改过的、而这次写入不是用户做的：**原文不动**，只返回它的 id。
/// 内容没变时也不刷新更新时间，界面上的「更新于」才说的是内容真的变了的那一刻。
pub fn upsert(conn: &Connection, input: &SourceInput<'_>) -> Result<i64> {
    let existing: Option<(i64, bool, String)> = conn
        .query_row(
            "SELECT id,user_modified,content_hash FROM lyric_sources \
             WHERE song_id=?1 AND kind=?2 AND provider=?3 AND role=?4 AND language=?5",
            params![input.song_id, input.kind, input.provider, input.role, input.language],
            |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0, r.get(2)?)),
        )
        .optional()?;
    match existing {
        Some((id, true, _)) if !input.user_modified => Ok(id),
        Some((id, _, hash)) if hash == input.content_hash => {
            conn.execute(
                "UPDATE lyric_sources SET missing=0, source_ref=?2, user_modified=MAX(user_modified,?3) WHERE id=?1",
                params![id, input.source_ref, input.user_modified as i64],
            )?;
            Ok(id)
        }
        Some((id, _, _)) => {
            conn.execute(
                &format!(
                    "UPDATE lyric_sources SET source_ref=?2, source_type=?3, raw_format=?4, raw_text=?5, content_hash=?6, \
                     line_count=?7, timed_line_count=?8, precision=?9, user_modified=?10, missing=0, updated_at={NOW} WHERE id=?1"
                ),
                params![
                    id,
                    input.source_ref,
                    input.source_type,
                    input.raw_format,
                    input.raw_text,
                    input.content_hash,
                    input.line_count,
                    input.timed_line_count,
                    input.precision,
                    input.user_modified as i64
                ],
            )?;
            conn.execute("DELETE FROM lyric_timeline_cache WHERE source_id=?1", [id])?;
            Ok(id)
        }
        None => {
            conn.execute(
                &format!(
                    "INSERT INTO lyric_sources (song_id,kind,provider,source_ref,role,language,source_type,raw_format,raw_text,\
                     content_hash,line_count,timed_line_count,precision,user_modified,updated_at) \
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,{NOW})"
                ),
                params![
                    input.song_id,
                    input.kind,
                    input.provider,
                    input.source_ref,
                    input.role,
                    input.language,
                    input.source_type,
                    input.raw_format,
                    input.raw_text,
                    input.content_hash,
                    input.line_count,
                    input.timed_line_count,
                    input.precision,
                    input.user_modified as i64
                ],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

/// 本地文件这一轮没找到：标成缺失（行留着，原文还能看、还能选）。
pub fn mark_missing(conn: &Connection, song_id: &str, kind: &str, keep: &[i64]) -> Result<usize> {
    let mut stmt = conn.prepare("SELECT id FROM lyric_sources WHERE song_id=?1 AND kind=?2 AND missing=0")?;
    let ids: Vec<i64> = stmt.query_map(params![song_id, kind], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let mut n = 0;
    for id in ids.into_iter().filter(|id| !keep.contains(id)) {
        n += conn.execute("UPDATE lyric_sources SET missing=1 WHERE id=?1", [id])?;
    }
    Ok(n)
}

pub fn selection(conn: &Connection, song_id: &str) -> Result<Option<Selection>> {
    if !table_exists(conn)? {
        return Ok(None);
    }
    Ok(conn
        .query_row(
            "SELECT source_id,translation_source_id,translation_off,chosen_by,offset_ms,updated_at FROM lyric_selection WHERE song_id=?1",
            [song_id],
            |r| {
                Ok(Selection {
                    source_id: r.get(0)?,
                    translation_source_id: r.get(1)?,
                    translation_off: r.get::<_, i64>(2)? != 0,
                    chosen_by: r.get(3)?,
                    offset_ms: r.get(4)?,
                    updated_at: r.get(5)?,
                })
            },
        )
        .optional()?)
}

/// 这首歌的来源是不是由用户（或升级前的库）定的：自动流程不该换它。
pub fn locked(conn: &Connection, song_id: &str) -> Result<bool> {
    Ok(selection(conn, song_id)?.is_some_and(|s| s.chosen_by == "user" || s.chosen_by == "legacy"))
}

pub fn set_chosen_by(conn: &Connection, song_id: &str, chosen_by: &str) -> Result<()> {
    ensure!(matches!(chosen_by, "user" | "auto" | "legacy"), "不认识的选择方式：{chosen_by}");
    conn.execute(
        &format!("UPDATE lyric_selection SET chosen_by=?2, updated_at={NOW} WHERE song_id=?1"),
        params![song_id, chosen_by],
    )?;
    Ok(())
}

/// 歌词挂上了一版：登记成来源、设成当前选择。`lineage::record_version` 里调，和版本同一个事务。
///
/// 选择方式：用户导入 / 编辑 / 恢复的记 `user`；其他记 `auto`。
/// 用户在候选里点「用这个」之类的显式选择，由调用方随后 [`set_chosen_by`] 改成 `user`。
/// 刚落库的时间没加偏移，所以选择里的偏移归零。
pub fn register_applied(conn: &Connection, song_id: &str, source: &LyricSource, raw_text: Option<&str>, lines: &[LineInput<'_>]) -> Result<Option<i64>> {
    if !table_exists(conn)? {
        return Ok(None);
    }
    let kind = kind_of(&source.source_type);
    let provider = if source.source_type == "manual_edit" {
        "edit".to_string()
    } else if source.source_type == "restored" {
        "restored".to_string()
    } else {
        source.source_name.clone()
    };
    let timed = lines.iter().filter(|l| l.time_sec.is_some()).count() as i64;
    let format = raw_text.map(sniff_format).unwrap_or("");
    let hash = raw_text.map(raw_hash).unwrap_or_default();
    let id = upsert(
        conn,
        &SourceInput {
            song_id,
            kind,
            provider: &provider,
            source_ref: &source.source_ref,
            role: "original",
            language: "",
            source_type: &source.source_type,
            raw_format: format,
            raw_text,
            content_hash: &hash,
            line_count: lines.len() as i64,
            timed_line_count: timed,
            precision: if format == "enhanced_lrc" { "word" } else if timed > 0 { "line" } else { "none" },
            user_modified: kind == "user",
        },
    )?;
    let chosen_by = if kind == "user" { "user" } else { "auto" };
    conn.execute(
        &format!(
            "INSERT INTO lyric_selection (song_id,source_id,chosen_by,offset_ms,updated_at) VALUES (?1,?2,?3,0,{NOW}) \
             ON CONFLICT(song_id) DO UPDATE SET source_id=excluded.source_id, chosen_by=excluded.chosen_by, offset_ms=0, updated_at=excluded.updated_at"
        ),
        params![song_id, id, chosen_by],
    )?;
    Ok(Some(id))
}

/// 入库之后才知道的格式和精度（逐字 / 逐行），以及转换前的原文（yrc、qrc、srt…）。
pub fn update_current_format(conn: &Connection, song_id: &str, raw_format: &str, precision: &str, original: Option<&str>) -> Result<()> {
    let Some(sel) = selection(conn, song_id)? else { return Ok(()) };
    let Some(id) = sel.source_id else { return Ok(()) };
    let precision = match precision {
        "word" | "karaoke" => "word",
        "line" | "line_synced" | "lineSynced" => "line",
        "none" | "plain" | "unsynced" => "none",
        other => other,
    };
    match original {
        Some(raw) => {
            conn.execute(
                "UPDATE lyric_sources SET raw_format=?2, precision=?3, raw_text=?4, content_hash=?5 WHERE id=?1",
                params![id, raw_format, precision, raw, raw_hash(raw)],
            )?;
        }
        None => {
            conn.execute("UPDATE lyric_sources SET raw_format=?2, precision=?3 WHERE id=?1", params![id, raw_format, precision])?;
        }
    }
    Ok(())
}

/// 歌词被清掉了：选择里不再指向任何来源（选择方式留着，用户锁定的歌清掉再挂也还是锁定的）。
pub fn on_cleared(conn: &Connection, song_id: &str) -> Result<()> {
    if !table_exists(conn)? {
        return Ok(());
    }
    conn.execute(
        &format!("UPDATE lyric_selection SET source_id=NULL, offset_ms=0, updated_at={NOW} WHERE song_id=?1"),
        [song_id],
    )?;
    Ok(())
}

/// 删歌：来源、选择、查询记录、缓存一起删。
pub fn forget(conn: &Connection, song_id: &str) -> Result<()> {
    if !table_exists(conn)? {
        return Ok(());
    }
    conn.execute("DELETE FROM lyric_timeline_cache WHERE source_id IN (SELECT id FROM lyric_sources WHERE song_id=?1)", [song_id])?;
    conn.execute("DELETE FROM lyric_sources WHERE song_id=?1", [song_id])?;
    conn.execute("DELETE FROM lyric_selection WHERE song_id=?1", [song_id])?;
    conn.execute("DELETE FROM lyric_lookups WHERE song_id=?1", [song_id])?;
    Ok(())
}

/// 改整体偏移。正值让歌词晚一点出现。
///
/// 在一个事务里把差值加到这首歌的行时间和来源给的逐字时间上；手动校准的逐字时间不动。
/// 记进选择（已经落库的偏移）和当前来源（下次重新挑它时照这个偏移落库）。返回改了几行。
pub fn set_offset(conn: &mut Connection, song_id: &str, offset_ms: i64) -> Result<usize> {
    let tx = conn.transaction()?;
    let n = set_offset_in(&tx, song_id, offset_ms)?;
    tx.commit()?;
    Ok(n)
}

/// [`set_offset`] 的事务内版本，给已经开着事务的调用方。
pub fn set_offset_in(conn: &Connection, song_id: &str, offset_ms: i64) -> Result<usize> {
    ensure!(offset_ms.abs() <= MAX_OFFSET_MS, "偏移最多 ±{} 秒", MAX_OFFSET_MS / 1000);
    let Some(sel) = selection(conn, song_id)? else {
        bail!("这首歌还没有登记歌词来源，没法调偏移");
    };
    let delta = offset_ms - sel.offset_ms;
    let mut changed = 0;
    if delta != 0 {
        let sec = delta as f64 / 1000.0;
        changed = conn.execute(
            "UPDATE utterances SET time_sec = time_sec + ?2 WHERE song_id=?1 AND time_sec IS NOT NULL",
            params![song_id, sec],
        )?;
        conn.execute(
            "UPDATE word_timings SET start_sec = start_sec + ?2, end_sec = CASE WHEN end_sec IS NULL THEN NULL ELSE end_sec + ?2 END \
             WHERE source <> 'manual' AND utterance_id IN (SELECT id FROM utterances WHERE song_id=?1)",
            params![song_id, sec],
        )?;
        crate::lineage::log(
            conn,
            &crate::lineage::Revision {
                song_id: song_id.into(),
                kind: "lyrics_offset".into(),
                before: Some(serde_json::json!({ "offsetMs": sel.offset_ms })),
                after: Some(serde_json::json!({ "offsetMs": offset_ms })),
                actor: "user".into(),
                ..Default::default()
            },
        )?;
    }
    conn.execute(
        &format!("UPDATE lyric_selection SET offset_ms=?2, updated_at={NOW} WHERE song_id=?1"),
        params![song_id, offset_ms],
    )?;
    if let Some(id) = sel.source_id {
        conn.execute("UPDATE lyric_sources SET offset_ms=?2 WHERE id=?1", params![id, offset_ms])?;
    }
    Ok(changed)
}

/// 选译文。`None` = 不要译文。
pub fn select_translation(conn: &Connection, song_id: &str, source_id: Option<i64>) -> Result<()> {
    if let Some(id) = source_id {
        let row = get(conn, id)?.ok_or_else(|| anyhow::anyhow!("找不到来源 {id}"))?;
        ensure!(row.song_id == song_id && row.role == "translation", "来源 {id} 不是这首歌的译文");
    }
    conn.execute(
        &format!(
            "INSERT INTO lyric_selection (song_id,translation_source_id,translation_off,updated_at) VALUES (?1,?2,?3,{NOW}) \
             ON CONFLICT(song_id) DO UPDATE SET translation_source_id=excluded.translation_source_id, \
             translation_off=excluded.translation_off, updated_at=excluded.updated_at"
        ),
        params![song_id, source_id, source_id.is_none() as i64],
    )?;
    Ok(())
}

/// 默认该用哪一份原文：按类别、再按精度。缺文件的、没有原文的不算。
pub fn best_original(rows: &[SourceRow]) -> Option<&SourceRow> {
    rows.iter()
        .filter(|r| r.role == "original" && r.has_raw && !r.missing && r.line_count > 0)
        .min_by_key(|r| (kind_rank(&r.kind), precision_rank(&r.precision), std::cmp::Reverse(r.updated_at.clone())))
}

/// 默认的译文：选择里指定的；没指定、也没说不要时，取第一份可用的。
pub fn chosen_translation(rows: &[SourceRow], sel: Option<&Selection>) -> Option<i64> {
    if sel.is_some_and(|s| s.translation_off) {
        return None;
    }
    if let Some(id) = sel.and_then(|s| s.translation_source_id)
        && rows.iter().any(|r| r.id == id) {
            return Some(id);
        }
    rows.iter()
        .filter(|r| r.role == "translation" && r.has_raw && !r.missing)
        .min_by_key(|r| (kind_rank(&r.kind), r.id))
        .map(|r| r.id)
}

pub fn raw_hash(raw: &str) -> String {
    let mut h = crate::lineage::Fingerprint::new();
    h.str(raw);
    h.hex()
}

// ── 在线查询记录 ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lookup {
    pub provider: String,
    /// found / not_found / failed / ambiguous
    pub status: String,
    pub message: String,
    pub checked_at: String,
    /// 只有 not_found 有：这之前批量补齐不再问
    pub retry_after: String,
}

/// 记一次在线查询的结果。`not_found` 带负缓存，`failed` 不带（网络会恢复，随时可以重试）。
pub fn record_lookup(conn: &Connection, song_id: &str, provider: &str, status: &str, message: &str) -> Result<()> {
    ensure!(matches!(status, "found" | "not_found" | "failed" | "ambiguous"), "不认识的查询结果：{status}");
    let retry = if status == "not_found" {
        format!("strftime('%Y-%m-%dT%H:%M:%SZ','now','+{NOT_FOUND_RETRY_DAYS} days')")
    } else {
        "''".to_string()
    };
    conn.execute(
        &format!(
            "INSERT INTO lyric_lookups (song_id,provider,status,message,checked_at,retry_after) VALUES (?1,?2,?3,?4,{NOW},{retry}) \
             ON CONFLICT(song_id,provider) DO UPDATE SET status=excluded.status, message=excluded.message, \
             checked_at=excluded.checked_at, retry_after=excluded.retry_after"
        ),
        params![song_id, provider, status, message],
    )?;
    Ok(())
}

pub fn lookups(conn: &Connection, song_id: &str) -> Result<Vec<Lookup>> {
    if !table_exists(conn)? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT provider,status,message,checked_at,retry_after FROM lyric_lookups WHERE song_id=?1 ORDER BY provider")?;
    let rows = stmt
        .query_map([song_id], |r| {
            Ok(Lookup { provider: r.get(0)?, status: r.get(1)?, message: r.get(2)?, checked_at: r.get(3)?, retry_after: r.get(4)? })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// 批量补齐该不该跳过这首：上次没找到、负缓存还没过期。网络失败不跳过。
pub fn recently_not_found(conn: &Connection, song_id: &str, provider: &str) -> Result<bool> {
    if !table_exists(conn)? {
        return Ok(false);
    }
    Ok(conn
        .query_row(
            &format!("SELECT 1 FROM lyric_lookups WHERE song_id=?1 AND provider=?2 AND status='not_found' AND retry_after > {NOW}"),
            params![song_id, provider],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

// ── 时间轴缓存 ────────────────────────────────────────────────────

pub fn cached_timeline(conn: &Connection, source_id: i64, content_hash: &str) -> Result<Option<crate::lyric_timeline::LyricTimeline>> {
    let json: Option<String> = conn
        .query_row(
            "SELECT timeline_json FROM lyric_timeline_cache WHERE source_id=?1 AND parser_version=?2 AND content_hash=?3",
            params![source_id, crate::lyric_timeline::PARSER_VERSION, content_hash],
            |r| r.get(0),
        )
        .optional()?;
    Ok(json.and_then(|j| serde_json::from_str(&j).ok()))
}

pub fn store_timeline(conn: &Connection, source_id: i64, content_hash: &str, timeline: &crate::lyric_timeline::LyricTimeline) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT INTO lyric_timeline_cache (source_id,parser_version,content_hash,timeline_json,built_at) VALUES (?1,?2,?3,?4,{NOW}) \
             ON CONFLICT(source_id) DO UPDATE SET parser_version=excluded.parser_version, content_hash=excluded.content_hash, \
             timeline_json=excluded.timeline_json, built_at=excluded.built_at"
        ),
        params![source_id, crate::lyric_timeline::PARSER_VERSION, content_hash, serde_json::to_string(timeline)?],
    )?;
    Ok(())
}

// ── 老库登记 ──────────────────────────────────────────────────────

/// 升级前就有歌词、但还没有来源记录的歌：把当前版登记成一个来源。
///
/// 选择方式记 `legacy`（用户导入 / 编辑 / 恢复的记 `user`），自动流程不会去换它——
/// 存量几百首歌按新优先级重挑一遍，会把它们整首重新分词、行号全换。
/// **在一个事务里调**。重复调是空操作。返回登记了几首。
/// 补登记时读出的一行当前版：歌曲、来源类型、名字、引用、原文、格式、行数、带时间行数、粒度
type CurrentVersionRow = (String, String, String, String, Option<String>, String, i64, i64, String);

pub fn backfill(conn: &Connection) -> Result<usize> {
    if !table_exists(conn)? {
        return Ok(0);
    }
    let rows: Vec<CurrentVersionRow> = {
        let mut stmt = conn.prepare(
            "SELECT v.song_id, v.source_type, v.source_name, v.source_ref, COALESCE(v.source_raw, v.raw_text), \
                    v.raw_format, v.line_count, v.timed_line_count, v.sync_granularity \
             FROM lyric_versions v \
             WHERE v.is_current=1 AND NOT EXISTS (SELECT 1 FROM lyric_selection s WHERE s.song_id=v.song_id) \
               AND EXISTS (SELECT 1 FROM utterances u WHERE u.song_id=v.song_id) ORDER BY v.song_id",
        )?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (song_id, source_type, name, reference, raw, format, lines, timed, granularity) in &rows {
        let kind = kind_of(source_type);
        let provider = match source_type.as_str() {
            "manual_edit" => "edit",
            "restored" => "restored",
            _ => name.as_str(),
        };
        let format = if format.is_empty() { raw.as_deref().map(sniff_format).unwrap_or("") } else { format.as_str() };
        let precision = match granularity.as_str() {
            "karaoke" | "word" => "word",
            _ if *timed > 0 => "line",
            _ => "none",
        };
        let hash = raw.as_deref().map(raw_hash).unwrap_or_default();
        let id = upsert(
            conn,
            &SourceInput {
                song_id,
                kind,
                provider,
                source_ref: reference,
                role: "original",
                language: "",
                source_type,
                raw_format: format,
                raw_text: raw.as_deref(),
                content_hash: &hash,
                line_count: *lines,
                timed_line_count: *timed,
                precision,
                user_modified: kind == "user",
            },
        )?;
        let chosen_by = if kind == "user" { "user" } else { "legacy" };
        conn.execute(
            &format!("INSERT OR IGNORE INTO lyric_selection (song_id,source_id,chosen_by,offset_ms,updated_at) VALUES (?1,?2,?3,0,{NOW})"),
            params![song_id, id, chosen_by],
        )?;
    }
    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> crate::Corpus {
        let corpus = crate::Corpus::open_in_memory().unwrap();
        corpus.ensure_schema().unwrap();
        corpus.connection().execute("INSERT INTO songs(id,title,artist) VALUES('s','曲','歌手')", []).unwrap();
        corpus
    }

    fn input<'a>(kind: &'a str, provider: &'a str, raw: &'a str, hash: &'a str, user: bool) -> SourceInput<'a> {
        SourceInput {
            song_id: "s",
            kind,
            provider,
            role: "original",
            raw_format: "lrc",
            raw_text: Some(raw),
            content_hash: hash,
            line_count: 1,
            timed_line_count: 1,
            precision: "line",
            user_modified: user,
            ..Default::default()
        }
    }

    #[test]
    fn the_priority_is_user_then_sibling_then_embedded_then_online() {
        let corpus = db();
        let conn = corpus.connection();
        for (kind, provider) in [("online", "netease"), ("embedded", "a.flac"), ("sibling", "a.lrc"), ("user", "edit")] {
            upsert(conn, &input(kind, provider, "[00:01.00]x", kind, kind == "user")).unwrap();
        }
        let rows = list(conn, "s").unwrap();
        assert_eq!(rows.iter().map(|r| r.kind.as_str()).collect::<Vec<_>>(), vec!["user", "sibling", "embedded", "online"]);
        assert_eq!(best_original(&rows).unwrap().kind, "user");
        let without_user: Vec<_> = rows.into_iter().filter(|r| r.kind != "user").collect();
        assert_eq!(best_original(&without_user).unwrap().kind, "sibling");
    }

    #[test]
    fn within_a_kind_word_timing_beats_line_timing_beats_plain_text() {
        let corpus = db();
        let conn = corpus.connection();
        let mut plain = input("sibling", "a.txt", "x", "1", false);
        plain.precision = "none";
        upsert(conn, &plain).unwrap();
        let mut word = input("sibling", "a.lrc", "y", "2", false);
        word.precision = "word";
        upsert(conn, &word).unwrap();
        assert_eq!(best_original(&list(conn, "s").unwrap()).unwrap().provider, "a.lrc");
    }

    #[test]
    fn an_automatic_update_never_overwrites_a_row_the_user_edited() {
        let corpus = db();
        let conn = corpus.connection();
        let id = upsert(conn, &input("user", "edit", "我改过的", "u1", true)).unwrap();
        let again = upsert(conn, &input("user", "edit", "自动来的", "u2", false)).unwrap();
        assert_eq!(id, again);
        assert_eq!(raw_text(conn, id).unwrap().as_deref(), Some("我改过的"));
        // 用户自己再改一次是可以的
        upsert(conn, &input("user", "edit", "又改了", "u3", true)).unwrap();
        assert_eq!(raw_text(conn, id).unwrap().as_deref(), Some("又改了"));
    }

    #[test]
    fn registering_a_user_version_locks_the_selection_and_automatic_ones_do_not() {
        let corpus = db();
        let conn = corpus.connection();
        let lines = [LineInput { time_sec: Some(1.0), text: "x" }];
        register_applied(conn, "s", &LyricSource::new("online", "netease", "曲 — 歌手", "user"), Some("[00:01.00]x"), &lines).unwrap();
        assert!(!locked(conn, "s").unwrap());
        register_applied(conn, "s", &LyricSource::new("manual_edit", "", "", "user"), Some("[00:01.00]x"), &lines).unwrap();
        let sel = selection(conn, "s").unwrap().unwrap();
        assert_eq!(sel.chosen_by, "user");
        assert!(locked(conn, "s").unwrap());
        assert_eq!(get(conn, sel.source_id.unwrap()).unwrap().unwrap().kind, "user");
    }

    #[test]
    fn not_found_is_cached_but_a_failure_is_not() {
        let corpus = db();
        let conn = corpus.connection();
        record_lookup(conn, "s", "netease", "failed", "timeout").unwrap();
        assert!(!recently_not_found(conn, "s", "netease").unwrap());
        record_lookup(conn, "s", "netease", "not_found", "").unwrap();
        assert!(recently_not_found(conn, "s", "netease").unwrap());
        assert!(!recently_not_found(conn, "s", "qq").unwrap());
        assert!(record_lookup(conn, "s", "netease", "maybe", "").is_err());
    }

    #[test]
    fn sniffing_tells_lrc_srt_enhanced_and_text_apart() {
        assert_eq!(sniff_format("[00:01.00]あ\n"), "lrc");
        assert_eq!(sniff_format("[00:01.00]<00:01.00>あ<00:01.50>い\n"), "enhanced_lrc");
        assert_eq!(sniff_format("1\n00:00:01,000 --> 00:00:02,000\nあ\n"), "srt");
        assert_eq!(sniff_format("ただの歌詞\n二行目"), "text");
        assert_eq!(sniff_format("[ar:歌手]\nただの歌詞"), "text");
    }
}
