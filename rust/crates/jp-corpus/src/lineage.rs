//! 来源追踪：每份歌词哪来的、哪一版、校验结果如何；每行分词用的哪个分词器；
//! 谁在什么时候改过什么；每个元数据字段是从哪来的。
//!
//! 表结构见 `schema.sql` 里「语料质量与来源追踪」那一段，设计说明见 `docs/provenance.md`。
//!
//! 写入这些表的入口都在这里，调用方（导入、补歌词、分词校正、改曲目信息、刮削）
//! 只管在自己的事务里调。**这里的函数都不自己开事务**——它们记的是「刚才那件事」，
//! 那件事回滚了，记录也要跟着回滚。
//!
//! 几条约定：
//!
//! - **原文不动**。`lyric_versions.raw_text` 写进去就不再改；重新分词、校正分词只动 `tokens`。
//! - **只追加**。换歌词是加一版、把旧版的 `is_current` 置 0，不删旧版；修订历史只插不改。
//! - **手改的不被覆盖**。字段来源是 `manual` 的，自动流程（刮削、语言推断）不再写它。

use std::collections::BTreeMap;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::quality::{self, LineInput, Severity, Validation};

/// SQLite 侧的当前时间，和 `songs.added_at` 同一个格式
pub(crate) const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%SZ','now')";

/// `utterances.token_status` 的取值
pub mod token_status {
    /// 还没分词（导入时没有词典）
    pub const PENDING: &str = "pending";
    /// 分词器给的结果
    pub const AUTO: &str = "auto";
    /// 你校正过的（或者换歌词后把校正套回来的）
    pub const CORRECTED: &str = "corrected";
    /// 分过了，但这一行没有任何实词（纯符号、空白）
    pub const EMPTY: &str = "empty";
}

// ── 指纹 ─────────────────────────────────────────────────────────

/// FNV-1a 64。只拿来做「内容变没变」的指纹，不是安全用途。
#[derive(Clone)]
pub(crate) struct Fingerprint(u64);

impl Fingerprint {
    pub(crate) fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    pub(crate) fn write(&mut self, bytes: &[u8]) -> &mut Self {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // 分隔符：「ab」+「c」和「a」+「bc」不能是同一个指纹
        self.0 ^= 0xff;
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        self
    }
    pub(crate) fn str(&mut self, s: &str) -> &mut Self {
        self.write(s.as_bytes())
    }
    pub(crate) fn hex(&self) -> String {
        format!("{:016x}", self.0)
    }
}

/// 解析后歌词行的指纹：时间（到厘秒）+ 文本
pub fn content_hash(lines: &[LineInput<'_>]) -> String {
    let mut h = Fingerprint::new();
    for line in lines {
        h.str(&line.time_sec.map(|t| format!("{t:.2}")).unwrap_or_default()).str(line.text);
    }
    h.hex()
}

// ── 歌词来源与版本 ───────────────────────────────────────────────

/// 一份歌词是从哪来的。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricSource {
    /// 见 `lyric_versions.source_type` 的注释
    pub source_type: String,
    /// provider 名或文件名
    pub source_name: String,
    /// 路径、URL 或在线条目「曲名 — 歌手」
    pub source_ref: String,
    /// 谁让它挂上去的：import / user / system
    pub actor: String,
}

impl LyricSource {
    pub fn new(source_type: &str, source_name: &str, source_ref: &str, actor: &str) -> Self {
        Self {
            source_type: source_type.into(),
            source_name: source_name.into(),
            source_ref: source_ref.into(),
            actor: actor.into(),
        }
    }
}

/// 刚记下的一版歌词。调用方拿 `id` 写 `utterances.version_id`，拿 `validation` 写每行的时间戳置信度。
#[derive(Debug, Clone)]
pub struct NewVersion {
    pub id: i64,
    pub version_no: i64,
    pub validation: Validation,
    /// 换下来的那一版（第一次挂歌词是 `None`）
    pub replaced: Option<i64>,
}

/// 记一版新歌词：校验、插入、把旧版设为非当前、记一条修订、按歌词推断语言。
///
/// `raw_text` 是挂上时的原文（解析前）。`lines` 是解析出来、马上要写进 `utterances` 的那些行。
/// **不写 `utterances`**——那是调用方的事，它还要顺手分词。
pub fn record_version(
    conn: &Connection,
    song_id: &str,
    source: &LyricSource,
    raw_text: Option<&str>,
    lines: &[LineInput<'_>],
) -> Result<NewVersion> {
    let duration: Option<f64> = conn
        .query_row("SELECT duration_sec FROM songs WHERE id=?1", params![song_id], |r| r.get(0))
        .optional()?
        .flatten();
    let validation = quality::validate(lines, duration);
    let replaced: Option<i64> = conn
        .query_row(
            "SELECT id FROM lyric_versions WHERE song_id=?1 AND is_current=1 ORDER BY version_no DESC LIMIT 1",
            params![song_id],
            |r| r.get(0),
        )
        .optional()?;
    let version_no: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version_no),0)+1 FROM lyric_versions WHERE song_id=?1",
        params![song_id],
        |r| r.get(0),
    )?;
    conn.execute("UPDATE lyric_versions SET is_current=0 WHERE song_id=?1", params![song_id])?;
    conn.execute(
        &format!(
            "INSERT INTO lyric_versions \
             (song_id, version_no, source_type, source_name, source_ref, imported_at, raw_text, content_hash, \
              line_count, timed_line_count, completeness, validation_json, validated_at, is_current) \
             VALUES (?1,?2,?3,?4,?5,{NOW},?6,?7,?8,?9,?10,?11,{NOW},1)"
        ),
        params![
            song_id,
            version_no,
            source.source_type,
            source.source_name,
            source.source_ref,
            raw_text,
            content_hash(lines),
            validation.line_count as i64,
            validation.timed_line_count as i64,
            validation.completeness,
            serde_json::to_string(&validation)?,
        ],
    )?;
    let id = conn.last_insert_rowid();

    let kind = if replaced.is_some() { "lyrics_replaced" } else { "lyrics_attached" };
    log(
        conn,
        &Revision {
            song_id: song_id.into(),
            kind: kind.into(),
            version_id: Some(id),
            before: replaced.map(|v| serde_json::json!({ "versionId": v })),
            after: Some(serde_json::json!({
                "versionNo": version_no,
                "sourceType": source.source_type,
                "sourceName": source.source_name,
                "lines": validation.line_count,
                "completeness": validation.completeness,
            })),
            actor: source.actor.clone(),
            ..Default::default()
        },
    )?;
    apply_language(conn, song_id, &validation.language)?;
    Ok(NewVersion { id, version_no, validation, replaced })
}

/// 歌词被清掉了（马上要换一份，或者就是不要了）：当前版不再是当前版。原文留着。
pub fn retire_current(conn: &Connection, song_id: &str) -> Result<()> {
    conn.execute("UPDATE lyric_versions SET is_current=0 WHERE song_id=?1", params![song_id])?;
    Ok(())
}

/// 按歌词推断的语言写进 `songs.language`。你手动定过的不覆盖。
fn apply_language(conn: &Connection, song_id: &str, guess: &quality::LanguageGuess) -> Result<()> {
    if guess.code == "unknown" || source_of(conn, "song", song_id, "language")?.as_deref() == Some("manual") {
        return Ok(());
    }
    conn.execute("UPDATE songs SET language=?1 WHERE id=?2", params![guess.code, song_id])?;
    set_provenance(conn, "song", song_id, "language", "heuristic", "lyrics-script", Some(guess.confidence))
}

// ── 分词器 ───────────────────────────────────────────────────────

/// 一种分词配置。字段和 `jp_tokenizer::TokenizerInfo` 一一对应（这一层不依赖分词器 crate）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TokenizerRun {
    pub engine: String,
    pub engine_version: String,
    pub dict_description: String,
    pub dict_created_at: String,
    pub dict_size: u64,
    pub split_mode: String,
    pub config_hash: String,
    pub pos_scheme: String,
}

impl TokenizerRun {
    /// 指纹：上面这些字段一样，分词结果就该一样
    pub fn fingerprint(&self) -> String {
        Fingerprint::new()
            .str(&self.engine)
            .str(&self.engine_version)
            .str(&self.dict_description)
            .str(&self.dict_created_at)
            .str(&self.dict_size.to_string())
            .str(&self.split_mode)
            .str(&self.config_hash)
            .str(&self.pos_scheme)
            .hex()
    }
}

/// 库里登记过的一种分词配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenizerRunRow {
    pub id: i64,
    pub fingerprint: String,
    #[serde(flatten)]
    pub run: TokenizerRun,
    pub first_used_at: String,
    /// 现在还指着它的歌词行数（只在概览里填）
    #[serde(default)]
    pub line_count: i64,
}

/// 取这种配置的 id，没有就登记一行
pub fn ensure_tokenizer_run(conn: &Connection, run: &TokenizerRun) -> Result<i64> {
    let fingerprint = run.fingerprint();
    if let Some(id) = conn
        .query_row("SELECT id FROM tokenizer_runs WHERE fingerprint=?1", params![fingerprint], |r| r.get(0))
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        &format!(
            "INSERT INTO tokenizer_runs (fingerprint, engine, engine_version, dict_description, dict_created_at, \
             dict_size, split_mode, config_hash, pos_scheme, first_used_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,{NOW})"
        ),
        params![
            fingerprint,
            run.engine,
            run.engine_version,
            run.dict_description,
            run.dict_created_at,
            run.dict_size as i64,
            run.split_mode,
            run.config_hash,
            run.pos_scheme
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 这些 id 对应的分词配置（顺序按 id）
pub fn tokenizer_runs(conn: &Connection, ids: &[i64]) -> Result<Vec<TokenizerRunRow>> {
    let mut out = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT id, fingerprint, engine, engine_version, dict_description, dict_created_at, dict_size, \
         split_mode, config_hash, pos_scheme, first_used_at FROM tokenizer_runs WHERE id=?1",
    )?;
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    for id in ids {
        if let Some(row) = stmt.query_row(params![id], tokenizer_row).optional()? {
            out.push(row);
        }
    }
    Ok(out)
}

fn tokenizer_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TokenizerRunRow> {
    Ok(TokenizerRunRow {
        id: r.get(0)?,
        fingerprint: r.get(1)?,
        run: TokenizerRun {
            engine: r.get(2)?,
            engine_version: r.get(3)?,
            dict_description: r.get(4)?,
            dict_created_at: r.get(5)?,
            dict_size: r.get::<_, i64>(6)? as u64,
            split_mode: r.get(7)?,
            config_hash: r.get(8)?,
            pos_scheme: r.get(9)?,
        },
        first_used_at: r.get(10)?,
        line_count: 0,
    })
}

/// 记下一行歌词的分词状态和分词器
pub fn mark_tokens(conn: &Connection, utterance_id: i64, status: &str, run_id: Option<i64>) -> Result<()> {
    conn.execute(
        "UPDATE utterances SET token_status=?2, tokenizer_run_id=COALESCE(?3, tokenizer_run_id) WHERE id=?1",
        params![utterance_id, status, run_id],
    )?;
    Ok(())
}

// ── 修订历史 ─────────────────────────────────────────────────────

/// 修订历史里的一条
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Revision {
    pub id: i64,
    pub song_id: String,
    pub song_artist: String,
    pub song_title: String,
    pub kind: String,
    pub version_id: Option<i64>,
    pub utterance_id: Option<i64>,
    pub line_idx: Option<i64>,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub actor: String,
    pub note: String,
    pub created_at: String,
}

/// 记一条修订。歌手、曲名没给就从 `songs` 里取（删歌之前记的那条也能看出是哪首）。
pub fn log(conn: &Connection, rev: &Revision) -> Result<()> {
    let (artist, title) = if rev.song_artist.is_empty() && rev.song_title.is_empty() {
        conn.query_row("SELECT artist, title FROM songs WHERE id=?1", params![rev.song_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .optional()?
        .unwrap_or_default()
    } else {
        (rev.song_artist.clone(), rev.song_title.clone())
    };
    let json = |v: &Option<serde_json::Value>| v.as_ref().map(|v| v.to_string()).unwrap_or_default();
    conn.execute(
        &format!(
            "INSERT INTO corpus_revisions (song_id, song_artist, song_title, kind, version_id, utterance_id, line_idx, \
             before_json, after_json, actor, note, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,{NOW})"
        ),
        params![
            rev.song_id,
            artist,
            title,
            rev.kind,
            rev.version_id,
            rev.utterance_id,
            rev.line_idx,
            json(&rev.before),
            json(&rev.after),
            rev.actor,
            rev.note
        ],
    )?;
    Ok(())
}

/// 一首歌的修订历史，新的在前
pub fn revisions(conn: &Connection, song_id: &str, limit: i64) -> Result<Vec<Revision>> {
    let mut stmt = conn.prepare(
        "SELECT id, song_id, song_artist, song_title, kind, version_id, utterance_id, line_idx, before_json, \
         after_json, actor, note, created_at FROM corpus_revisions WHERE song_id=?1 ORDER BY id DESC LIMIT ?2",
    )?;
    let parse = |s: String| (!s.is_empty()).then(|| serde_json::from_str(&s).ok()).flatten();
    let rows = stmt.query_map(params![song_id, limit], |r| {
        Ok(Revision {
            id: r.get(0)?,
            song_id: r.get(1)?,
            song_artist: r.get(2)?,
            song_title: r.get(3)?,
            kind: r.get(4)?,
            version_id: r.get(5)?,
            utterance_id: r.get(6)?,
            line_idx: r.get(7)?,
            before: parse(r.get(8)?),
            after: parse(r.get(9)?),
            actor: r.get(10)?,
            note: r.get(11)?,
            created_at: r.get(12)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

// ── 字段来源 ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldProvenance {
    pub field: String,
    pub source: String,
    pub source_ref: String,
    pub confidence: Option<f64>,
    pub updated_at: String,
}

/// 记下一个字段是从哪来的（覆盖这个字段原来的记录）
pub fn set_provenance(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
    field: &str,
    source: &str,
    source_ref: &str,
    confidence: Option<f64>,
) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT OR REPLACE INTO field_provenance (entity_type, entity_id, field, source, source_ref, confidence, updated_at) \
             VALUES (?1,?2,?3,?4,?5,?6,{NOW})"
        ),
        params![entity_type, entity_id, field, source, source_ref, confidence],
    )?;
    Ok(())
}

/// 这个字段记的来源；没记过是 `None`
pub fn source_of(conn: &Connection, entity_type: &str, entity_id: &str, field: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT source FROM field_provenance WHERE entity_type=?1 AND entity_id=?2 AND field=?3",
            params![entity_type, entity_id, field],
            |r| r.get(0),
        )
        .optional()?)
}

/// 一个对象所有字段的来源，按字段名排
pub fn provenance(conn: &Connection, entity_type: &str, entity_id: &str) -> Result<Vec<FieldProvenance>> {
    let mut stmt = conn.prepare(
        "SELECT field, source, source_ref, confidence, updated_at FROM field_provenance \
         WHERE entity_type=?1 AND entity_id=?2 ORDER BY field",
    )?;
    let rows = stmt.query_map(params![entity_type, entity_id], |r| {
        Ok(FieldProvenance {
            field: r.get(0)?,
            source: r.get(1)?,
            source_ref: r.get(2)?,
            confidence: r.get(3)?,
            updated_at: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 删歌时一起删：版本和字段来源挂在歌的 id 上，修订历史留着（它是日志）。
pub fn forget_song(conn: &Connection, song_id: &str) -> Result<()> {
    crate::identity::forget(conn,"song",song_id)?;
    conn.execute("DELETE FROM lyric_versions WHERE song_id=?1", params![song_id])?;
    conn.execute("DELETE FROM field_provenance WHERE entity_type='song' AND entity_id=?1", params![song_id])?;
    Ok(())
}

// ── 读：一首歌、整个库 ───────────────────────────────────────────

/// 一版歌词的概要（不含原文）
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricVersion {
    pub id: i64,
    pub version_no: i64,
    pub source_type: String,
    pub source_name: String,
    pub source_ref: String,
    pub imported_at: String,
    /// 原文留着没有（老库补登记的没有）
    pub has_raw_text: bool,
    pub content_hash: String,
    pub line_count: i64,
    pub timed_line_count: i64,
    pub completeness: Option<f64>,
    pub validation: Option<Validation>,
    pub validated_at: String,
    pub is_current: bool,
    pub note: String,
    pub raw_format: String,
    pub sync_granularity: String,
    pub timing_origin: String,
    pub timing_validation: serde_json::Value,
}

/// 一首歌的全部歌词版本，新的在前
pub fn versions(conn: &Connection, song_id: &str) -> Result<Vec<LyricVersion>> {
    let mut stmt = conn.prepare(
        "SELECT id, version_no, source_type, source_name, source_ref, imported_at, raw_text IS NOT NULL, content_hash, \
         line_count, timed_line_count, completeness, validation_json, validated_at, is_current, note, \
         raw_format, sync_granularity, timing_origin, timing_validation_json \
         FROM lyric_versions WHERE song_id=?1 ORDER BY version_no DESC",
    )?;
    let rows = stmt.query_map(params![song_id], |r| {
        Ok(LyricVersion {
            id: r.get(0)?,
            version_no: r.get(1)?,
            source_type: r.get(2)?,
            source_name: r.get(3)?,
            source_ref: r.get(4)?,
            imported_at: r.get(5)?,
            has_raw_text: r.get(6)?,
            content_hash: r.get(7)?,
            line_count: r.get(8)?,
            timed_line_count: r.get(9)?,
            completeness: r.get(10)?,
            validation: serde_json::from_str(&r.get::<_, String>(11)?).ok(),
            validated_at: r.get(12)?,
            is_current: r.get(13)?,
            note: r.get(14)?,
            raw_format: r.get(15)?,
            sync_granularity: r.get(16)?,
            timing_origin: r.get(17)?,
            timing_validation: serde_json::from_str(&r.get::<_, String>(18)?).unwrap_or_default(),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 一版歌词的原文。`(song_id, 原文)`；版本不存在是 `None`，原文没留下是 `Some((id, None))`
pub fn version_raw(conn: &Connection, version_id: i64) -> Result<Option<(String, Option<String>)>> {
    Ok(conn
        .query_row("SELECT song_id, raw_text FROM lyric_versions WHERE id=?1", params![version_id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?)
}

/// 一行歌词的质量信息
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineQuality {
    pub utterance_id: i64,
    pub line_idx: i64,
    pub time_sec: Option<f64>,
    pub time_confidence: Option<f64>,
    pub token_status: Option<String>,
    pub tokenizer_run_id: Option<i64>,
}

/// 一首歌的来源与质量：版本、逐行状态、用到的分词器、字段来源、修订历史
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongLineage {
    pub song_id: String,
    pub language: Option<String>,
    pub versions: Vec<LyricVersion>,
    pub lines: Vec<LineQuality>,
    /// 各分词状态的行数
    pub token_status: BTreeMap<String, i64>,
    pub tokenizers: Vec<TokenizerRunRow>,
    pub provenance: Vec<FieldProvenance>,
    pub revisions: Vec<Revision>,
}

pub fn song_lineage(conn: &Connection, song_id: &str) -> Result<SongLineage> {
    let language: Option<String> = conn
        .query_row("SELECT language FROM songs WHERE id=?1", params![song_id], |r| r.get(0))
        .optional()?
        .flatten();
    let lines: Vec<LineQuality> = {
        let mut stmt = conn.prepare(
            "SELECT id, line_idx, time_sec, time_confidence, token_status, tokenizer_run_id \
             FROM utterances WHERE song_id=?1 ORDER BY line_idx, id",
        )?;
        stmt.query_map(params![song_id], |r| {
            Ok(LineQuality {
                utterance_id: r.get(0)?,
                line_idx: r.get(1)?,
                time_sec: r.get(2)?,
                time_confidence: r.get(3)?,
                token_status: r.get(4)?,
                tokenizer_run_id: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    let mut token_status = BTreeMap::new();
    for line in &lines {
        *token_status.entry(line.token_status.clone().unwrap_or_else(|| "unknown".into())).or_default() += 1;
    }
    let run_ids: Vec<i64> = lines.iter().filter_map(|l| l.tokenizer_run_id).collect();
    let mut tokenizers = tokenizer_runs(conn, &run_ids)?;
    for t in &mut tokenizers {
        t.line_count = lines.iter().filter(|l| l.tokenizer_run_id == Some(t.id)).count() as i64;
    }
    Ok(SongLineage {
        song_id: song_id.into(),
        language,
        versions: versions(conn, song_id)?,
        lines,
        token_status,
        tokenizers,
        provenance: provenance(conn, "song", song_id)?,
        revisions: revisions(conn, song_id, 200)?,
    })
}

/// 质量清单里的一首歌
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongQuality {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub completeness: Option<f64>,
    pub source_type: String,
    pub warnings: i64,
    pub errors: i64,
    /// 最要紧的几个问题代码
    pub codes: Vec<String>,
}

/// 整个库的语料质量
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityOverview {
    pub songs: i64,
    pub songs_with_lyrics: i64,
    /// 有当前歌词版本的歌
    pub songs_with_version: i64,
    /// 老库补登记、原文没留下的
    pub legacy_versions: i64,
    pub avg_completeness: Option<f64>,
    /// (来源类型, 首数)
    pub by_source: Vec<(String, i64)>,
    /// (语言, 首数)；没判断的是空串
    pub by_language: Vec<(String, i64)>,
    /// (问题代码, 出现在几首歌里)
    pub issues: Vec<(String, i64)>,
    /// (分词状态, 行数)
    pub token_status: Vec<(String, i64)>,
    pub lines_low_time_confidence: i64,
    pub tokenizers: Vec<TokenizerRunRow>,
    /// 完整度最低、或者有错误的歌，最多 `limit` 首
    pub worst: Vec<SongQuality>,
}

/// 完整度低于这个值的歌进「需要看一眼」清单
pub const LOW_COMPLETENESS: f64 = 0.8;
/// 时间戳置信度低于这个值的行算「不可靠」
pub const LOW_TIME_CONFIDENCE: f64 = 0.5;

pub fn quality_overview(conn: &Connection, limit: usize) -> Result<QualityOverview> {
    let one = |sql: &str| -> Result<i64> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };
    let pairs = |sql: &str| -> Result<Vec<(String, i64)>> {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    };

    let mut issue_counts: BTreeMap<String, i64> = BTreeMap::new();
    let mut worst = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT s.id, s.title, s.artist, v.completeness, v.source_type, v.validation_json \
             FROM lyric_versions v JOIN songs s ON s.id = v.song_id WHERE v.is_current=1 \
             ORDER BY v.completeness, s.id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<f64>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        for row in rows {
            let (song_id, title, artist, completeness, source_type, json) = row?;
            let validation: Option<Validation> = serde_json::from_str(&json).ok();
            let (mut warnings, mut errors, mut codes) = (0, 0, Vec::<String>::new());
            if let Some(v) = &validation {
                let mut seen = std::collections::BTreeSet::new();
                for issue in &v.issues {
                    match issue.severity {
                        Severity::Error => errors += 1,
                        Severity::Warning => warnings += 1,
                        Severity::Info => {}
                    }
                    if seen.insert(issue.code.clone()) {
                        *issue_counts.entry(issue.code.clone()).or_default() += 1;
                        if issue.severity >= Severity::Warning {
                            codes.push(issue.code.clone());
                        }
                    }
                }
            }
            if worst.len() < limit && (errors > 0 || completeness.is_some_and(|c| c < LOW_COMPLETENESS)) {
                worst.push(SongQuality { song_id, title, artist, completeness, source_type, warnings, errors, codes });
            }
        }
    }
    let mut issues: Vec<(String, i64)> = issue_counts.into_iter().collect();
    issues.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let mut tokenizers = {
        let mut stmt = conn.prepare(
            "SELECT id, fingerprint, engine, engine_version, dict_description, dict_created_at, dict_size, \
             split_mode, config_hash, pos_scheme, first_used_at FROM tokenizer_runs ORDER BY id",
        )?;
        stmt.query_map([], tokenizer_row)?.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for t in &mut tokenizers {
        t.line_count = conn.query_row("SELECT COUNT(*) FROM utterances WHERE tokenizer_run_id=?1", params![t.id], |r| r.get(0))?;
    }

    Ok(QualityOverview {
        songs: one("SELECT COUNT(*) FROM songs")?,
        songs_with_lyrics: one("SELECT COUNT(DISTINCT song_id) FROM utterances")?,
        songs_with_version: one("SELECT COUNT(DISTINCT song_id) FROM lyric_versions WHERE is_current=1")?,
        legacy_versions: one("SELECT COUNT(*) FROM lyric_versions WHERE is_current=1 AND raw_text IS NULL")?,
        avg_completeness: conn.query_row(
            "SELECT ROUND(AVG(completeness), 3) FROM lyric_versions WHERE is_current=1",
            [],
            |r| r.get(0),
        )?,
        by_source: pairs(
            "SELECT source_type, COUNT(*) FROM lyric_versions WHERE is_current=1 GROUP BY source_type ORDER BY 2 DESC, 1",
        )?,
        by_language: pairs("SELECT COALESCE(language,''), COUNT(*) FROM songs GROUP BY 1 ORDER BY 2 DESC, 1")?,
        issues,
        token_status: pairs(
            "SELECT COALESCE(token_status,'unknown'), COUNT(*) FROM utterances GROUP BY 1 ORDER BY 2 DESC, 1",
        )?,
        lines_low_time_confidence: conn.query_row(
            "SELECT COUNT(*) FROM utterances WHERE time_confidence IS NOT NULL AND time_confidence < ?1",
            params![LOW_TIME_CONFIDENCE],
            |r| r.get(0),
        )?,
        tokenizers,
        worst,
    })
}

// ── 重新校验、老库补登记 ─────────────────────────────────────────

/// 拿库里现有的歌词行重新校验当前版（比如补上了歌曲时长之后）。
/// 写回版本的校验结果和每行的时间戳置信度；**不动原文、不动分词**。返回是否有当前版。
pub fn revalidate(conn: &Connection, song_id: &str) -> Result<bool> {
    let Some(version_id) = conn
        .query_row(
            "SELECT id FROM lyric_versions WHERE song_id=?1 AND is_current=1",
            params![song_id],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    else {
        return Ok(false);
    };
    let (rows, validation) = validate_stored(conn, song_id)?;
    let ids: Vec<i64> = rows.iter().map(|r| r.0).collect();
    conn.execute(
        &format!(
            "UPDATE lyric_versions SET completeness=?2, validation_json=?3, validated_at={NOW}, \
             line_count=?4, timed_line_count=?5 WHERE id=?1"
        ),
        params![
            version_id,
            validation.completeness,
            serde_json::to_string(&validation)?,
            validation.line_count as i64,
            validation.timed_line_count as i64
        ],
    )?;
    write_line_confidence(conn, &ids, &validation)?;
    Ok(true)
}

type StoredLine = (i64, Option<f64>, String);

/// 库里这首歌现有的歌词行 → ((行 id, 时间, 文本), 校验结果)
fn validate_stored(conn: &Connection, song_id: &str) -> Result<(Vec<StoredLine>, Validation)> {
    let rows: Vec<StoredLine> = {
        let mut stmt = conn.prepare("SELECT id, time_sec, text FROM utterances WHERE song_id=?1 ORDER BY line_idx, id")?;
        stmt.query_map(params![song_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let duration: Option<f64> = conn
        .query_row("SELECT duration_sec FROM songs WHERE id=?1", params![song_id], |r| r.get(0))
        .optional()?
        .flatten();
    let inputs: Vec<LineInput<'_>> = rows.iter().map(|(_, t, text)| LineInput { time_sec: *t, text }).collect();
    let validation = quality::validate(&inputs, duration);
    Ok((rows, validation))
}

/// 每行的时间戳置信度写回 `utterances`（`ids` 和校验时的行一一对应）
pub fn write_line_confidence(conn: &Connection, ids: &[i64], validation: &Validation) -> Result<()> {
    let mut stmt = conn.prepare("UPDATE utterances SET time_confidence=?2 WHERE id=?1")?;
    for (id, c) in ids.iter().zip(&validation.line_confidence) {
        stmt.execute(params![id, c])?;
    }
    Ok(())
}

/// 老库补登记：有歌词行、但没有任何歌词版本的歌，补一版 `legacy`，并补上逐行状态。
///
/// 原文当时没留下（`raw_text` 为 NULL），来处只知道 `songs.source_file`（存量库里多半是空的）。
/// 分词状态按现状推：有校正的是 corrected，有分词的是 auto，否则 pending。
/// 分词器不知道是哪个——那些行的 `tokenizer_run_id` 留空，分析记录里会照实写「未知」。
///
/// **在一个事务里调**。重复调是空操作。返回补了几首。
pub fn backfill_legacy(conn: &Connection) -> Result<usize> {
    let songs: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT s.id, COALESCE(s.source_file,'') FROM songs s \
             WHERE EXISTS (SELECT 1 FROM utterances u WHERE u.song_id = s.id) \
               AND NOT EXISTS (SELECT 1 FROM lyric_versions v WHERE v.song_id = s.id) ORDER BY s.id",
        )?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    let has_corrections = conn
        .query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name='token_corrections'", [], |_| Ok(()))
        .optional()?
        .is_some();
    for (song_id, source_file) in &songs {
        let (rows, validation) = validate_stored(conn, song_id)?;
        let ids: Vec<i64> = rows.iter().map(|r| r.0).collect();
        let inputs: Vec<LineInput<'_>> = rows.iter().map(|(_, t, text)| LineInput { time_sec: *t, text }).collect();
        let name = std::path::Path::new(source_file)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        conn.execute(
            &format!(
                "INSERT INTO lyric_versions (song_id, version_no, source_type, source_name, source_ref, imported_at, \
                 raw_text, content_hash, line_count, timed_line_count, completeness, validation_json, validated_at, \
                 is_current, note) VALUES (?1, 1, 'legacy', ?2, ?3, COALESCE((SELECT added_at FROM songs WHERE id=?1),''), \
                 NULL, ?4, ?5, ?6, ?7, ?8, {NOW}, 1, '老库补登记：原文和分词器版本当时没有记录')"
            ),
            params![
                song_id,
                name,
                source_file,
                content_hash(&inputs),
                validation.line_count as i64,
                validation.timed_line_count as i64,
                validation.completeness,
                serde_json::to_string(&validation)?
            ],
        )?;
        let version_id = conn.last_insert_rowid();
        conn.execute("UPDATE utterances SET version_id=?2 WHERE song_id=?1", params![song_id, version_id])?;
        write_line_confidence(conn, &ids, &validation)?;
        let corrected = if has_corrections { "EXISTS (SELECT 1 FROM token_corrections c WHERE c.utterance_id = utterances.id)" } else { "0" };
        conn.execute(
            &format!(
                "UPDATE utterances SET token_status = CASE \
                   WHEN {corrected} THEN 'corrected' \
                   WHEN EXISTS (SELECT 1 FROM tokens t WHERE t.utterance_id = utterances.id) THEN 'auto' \
                   ELSE 'pending' END \
                 WHERE song_id=?1 AND token_status IS NULL"
            ),
            params![song_id],
        )?;
        apply_language(conn, song_id, &validation.language)?;
        log(
            conn,
            &Revision {
                song_id: song_id.clone(),
                kind: "legacy_registered".into(),
                version_id: Some(version_id),
                after: Some(serde_json::json!({ "lines": validation.line_count, "completeness": validation.completeness })),
                actor: "system".into(),
                ..Default::default()
            },
        )?;
    }
    Ok(songs.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::Corpus::ensure_schema_on(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO songs (id, title, artist, duration_sec) VALUES ('001', '夜に駆ける', 'YOASOBI', 200.0);",
        )
        .unwrap();
        conn
    }

    fn src(kind: &str) -> LyricSource {
        LyricSource::new(kind, "a.lrc", "E:/a.lrc", "import")
    }

    const LINES: [LineInput<'static>; 2] =
        [LineInput { time_sec: Some(1.0), text: "沈むように" }, LineInput { time_sec: Some(2.0), text: "溶けてゆくように" }];

    #[test]
    fn a_second_version_replaces_the_first_without_deleting_it() {
        let conn = db();
        let v1 = record_version(&conn, "001", &src("lrc_file"), Some("[00:01]沈むように"), &LINES).unwrap();
        assert_eq!(v1.version_no, 1);
        assert_eq!(v1.replaced, None);
        let v2 = record_version(&conn, "001", &src("online"), Some("raw2"), &LINES[..1]).unwrap();
        assert_eq!(v2.version_no, 2);
        assert_eq!(v2.replaced, Some(v1.id));

        let all = versions(&conn, "001").unwrap();
        assert_eq!(all.len(), 2, "旧版留着");
        assert!(all[0].is_current && !all[1].is_current);
        assert_eq!(version_raw(&conn, v1.id).unwrap().unwrap().1.as_deref(), Some("[00:01]沈むように"), "原文一字不改");

        let kinds: Vec<String> = revisions(&conn, "001", 10).unwrap().into_iter().map(|r| r.kind).collect();
        assert_eq!(kinds, ["lyrics_replaced", "lyrics_attached"]);
    }

    #[test]
    fn the_same_lyrics_hash_the_same() {
        let a = content_hash(&LINES);
        assert_eq!(a, content_hash(&LINES));
        assert_ne!(a, content_hash(&LINES[..1]));
        let shifted = [LineInput { time_sec: Some(1.5), text: "沈むように" }, LINES[1]];
        assert_ne!(a, content_hash(&shifted), "时间轴变了也是另一份");
    }

    #[test]
    fn language_is_inferred_but_a_manual_choice_wins() {
        let conn = db();
        record_version(&conn, "001", &src("lrc_file"), None, &LINES).unwrap();
        let lang: String = conn.query_row("SELECT language FROM songs WHERE id='001'", [], |r| r.get(0)).unwrap();
        assert_eq!(lang, "ja");
        assert_eq!(source_of(&conn, "song", "001", "language").unwrap().as_deref(), Some("heuristic"));

        conn.execute("UPDATE songs SET language='en' WHERE id='001'", []).unwrap();
        set_provenance(&conn, "song", "001", "language", "manual", "", None).unwrap();
        record_version(&conn, "001", &src("online"), None, &LINES).unwrap();
        let lang: String = conn.query_row("SELECT language FROM songs WHERE id='001'", [], |r| r.get(0)).unwrap();
        assert_eq!(lang, "en", "你定的语言不被推断覆盖");
    }

    #[test]
    fn a_tokenizer_configuration_is_registered_once() {
        let conn = db();
        let run = TokenizerRun { engine: "sudachi.rs".into(), engine_version: "0.6.11".into(), split_mode: "C".into(), ..Default::default() };
        let a = ensure_tokenizer_run(&conn, &run).unwrap();
        assert_eq!(a, ensure_tokenizer_run(&conn, &run).unwrap());
        let other = TokenizerRun { dict_description: "core 20250101".into(), ..run.clone() };
        assert_ne!(a, ensure_tokenizer_run(&conn, &other).unwrap(), "换了词典就是另一套");
    }

    #[test]
    fn legacy_songs_get_a_version_and_line_states_once() {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO utterances (id, song_id, line_idx, time_sec, text) VALUES
               (1,'001',0,1.0,'沈むように'),(2,'001',1,1.0,'沈むように'),(3,'001',2,3.0,'溶けてゆく');
             INSERT INTO tokens (utterance_id, token_idx, surface, lemma, pos) VALUES (1,0,'沈む','沈む','VERB');
             INSERT INTO token_corrections (utterance_id, tokens_json) VALUES (2,'[]');",
        )
        .unwrap();
        assert_eq!(backfill_legacy(&conn).unwrap(), 1);
        assert_eq!(backfill_legacy(&conn).unwrap(), 0, "第二次没有要补的");

        let lineage = song_lineage(&conn, "001").unwrap();
        assert_eq!(lineage.versions.len(), 1);
        let v = &lineage.versions[0];
        assert_eq!(v.source_type, "legacy");
        assert!(!v.has_raw_text);
        assert!(v.validation.as_ref().unwrap().issues.iter().any(|i| i.code == "stacked_duplicate"));
        let status: Vec<_> = lineage.lines.iter().map(|l| l.token_status.as_deref().unwrap()).collect();
        assert_eq!(status, ["auto", "corrected", "pending"]);
        assert_eq!(lineage.lines[1].time_confidence, Some(0.5));
        assert_eq!(lineage.language.as_deref(), Some("ja"));
    }

    #[test]
    fn the_overview_lists_songs_that_need_a_look() {
        let conn = db();
        conn.execute_batch("INSERT INTO songs (id, title, artist) VALUES ('002', 'B', 'X');").unwrap();
        record_version(&conn, "001", &src("lrc_file"), None, &LINES).unwrap(); // 200 秒只有 2 行
        let garbled = [LineInput { time_sec: None, text: "\u{FFFD}\u{FFFD}" }];
        record_version(&conn, "002", &src("online"), None, &garbled).unwrap();
        let o = quality_overview(&conn, 10).unwrap();
        assert_eq!(o.songs, 2);
        assert_eq!(o.songs_with_version, 2);
        assert_eq!(o.worst.len(), 2);
        assert!(o.issues.iter().any(|(c, n)| c == "too_few_lines" && *n == 1));
        assert!(o.by_source.contains(&("online".into(), 1)));
    }

    #[test]
    fn deleting_a_song_keeps_its_revision_log() {
        let conn = db();
        record_version(&conn, "001", &src("lrc_file"), None, &LINES).unwrap();
        set_provenance(&conn, "song", "001", "year", "tag", "", None).unwrap();
        forget_song(&conn, "001").unwrap();
        assert!(versions(&conn, "001").unwrap().is_empty());
        assert!(provenance(&conn, "song", "001").unwrap().is_empty());
        let log = revisions(&conn, "001", 10).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].song_title, "夜に駆ける", "日志里存了曲名，删了也认得出");
    }
}
