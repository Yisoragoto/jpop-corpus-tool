//! 歌词行在音频里的时间区间——检索结果「点了就能听」的依据。
//!
//! **精度只到行。** 库里的时间戳来自 LRC，每行一个开始时间，没有词级时间。
//! 所以 KWIC 命中能定位到「这个词所在的那一行」，不假装能定位到词本身。
//!
//! 行的结束时间要推出来，规则写在这里一处，Tauri 和 macOS 两个客户端都用它：
//!
//! 1. 下一条**更晚**的时间戳就是这一行的结束（[`SpanEnd::NextLine`]，可信）。
//! 2. 但如果离下一行太远——间奏、尾奏——直接放到下一行会多出十几秒纯伴奏。
//!    这时按字数估一个时长截断（[`SpanEnd::Estimated`]）。
//!    阈值来自用户真实语料（211 首、8,377 对相邻行）：行时长中位数 3.9 秒、p90 9 秒、p95 13.6 秒，
//!    平均每字约 0.42 秒（含行间停顿）。估计时长 = `3 + 0.25 × 字数`，夹在 \[5, 12\] 秒。
//! 3. 最后一行没有下一行：用曲目时长兜底，同样不超过估计时长。
//!
//! 「试听前后文」就是把区间往前后各扩几行，见 [`context_window`]。

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// 估计时长的下限、上限（秒）
const MIN_ESTIMATE_SEC: f64 = 5.0;
const MAX_ESTIMATE_SEC: f64 = 12.0;
/// 播放前提前多少秒起播。LRC 时间戳通常卡在第一个字发声处，提前一点听得到起音
pub const DEFAULT_PREROLL_SEC: f64 = 0.4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SpanEnd {
    /// 结束于下一行的开始时间
    NextLine,
    /// 离下一行太远（间奏）或没有下一行，按字数估的
    Estimated,
    /// 曲目在这之前就结束了
    TrackEnd,
}

/// 一行歌词在音频里的区间
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineSpan {
    pub utterance_id: i64,
    pub line_idx: i64,
    pub start_sec: f64,
    pub end_sec: f64,
    pub end_kind: SpanEnd,
}

/// 按字数估一行唱多久
pub fn estimated_duration(text: &str) -> f64 {
    let chars = text.chars().filter(|c| !c.is_whitespace()).count() as f64;
    (3.0 + 0.25 * chars).clamp(MIN_ESTIMATE_SEC, MAX_ESTIMATE_SEC)
}

/// 一行：(utterance_id, line_idx, time_sec, text)
pub type TimedLine = (i64, i64, Option<f64>, String);

/// 输入一首歌按行号排好的 (utterance_id, line_idx, time_sec, text)，算每行的区间。
/// 没有时间戳的行给 None。纯函数，单测和两个客户端的行为都以它为准。
pub fn spans_from_lines(
    lines: &[TimedLine],
    duration_sec: Option<f64>,
) -> Vec<Option<LineSpan>> {
    lines
        .iter()
        .enumerate()
        .map(|(i, (utterance_id, line_idx, time, text))| {
            let start = (*time)?;
            let estimate = start + estimated_duration(text);
            // 下一条更晚的时间戳。同一时刻的多行（双语 LRC）不算「下一行」
            let next = lines[i + 1..]
                .iter()
                .filter_map(|(_, _, t, _)| *t)
                .find(|t| *t > start);
            let (mut end, mut kind) = match next {
                Some(n) if n <= estimate => (n, SpanEnd::NextLine),
                _ => (estimate, SpanEnd::Estimated),
            };
            if let Some(total) = duration_sec.filter(|d| *d > start)
                && total < end
            {
                end = total;
                kind = SpanEnd::TrackEnd;
            }
            Some(LineSpan {
                utterance_id: *utterance_id,
                line_idx: *line_idx,
                start_sec: start,
                end_sec: end,
                end_kind: kind,
            })
        })
        .collect()
}

/// 一首歌所有行的区间（没有时间戳的行不出现）
pub fn line_spans(conn: &Connection, song_id: &str) -> Result<Vec<LineSpan>> {
    let lines = song_lines(conn, song_id)?;
    let duration = song_duration(conn, song_id)?;
    Ok(spans_from_lines(&lines, duration).into_iter().flatten().collect())
}

/// 一段可以直接交给播放器的区间：从哪儿起播、到哪儿停、高亮哪几行
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenWindow {
    pub song_id: String,
    /// 起播位置（已含提前量）
    pub start_sec: f64,
    pub end_sec: f64,
    /// 命中的那一行
    pub focus_utterance_id: i64,
    pub focus_start_sec: f64,
    pub focus_end_sec: f64,
    /// 窗口覆盖的行，按时间顺序
    pub utterance_ids: Vec<i64>,
}

/// 以某一行为中心、前后各扩 `before` / `after` 行的试听区间。
/// 这一行没有时间戳时返回 None——不给一个假位置。
pub fn context_window(
    conn: &Connection,
    utterance_id: i64,
    before: usize,
    after: usize,
    preroll_sec: f64,
) -> Result<Option<ListenWindow>> {
    let song_id: Option<String> = conn
        .query_row("SELECT song_id FROM live_utterances WHERE id = ?1", params![utterance_id], |r| r.get(0))
        .ok();
    let Some(song_id) = song_id else { return Ok(None) };
    let spans = line_spans(conn, &song_id)?;
    let Some(pos) = spans.iter().position(|s| s.utterance_id == utterance_id) else {
        return Ok(None);
    };
    let first = pos.saturating_sub(before);
    let last = (pos + after).min(spans.len() - 1);
    // 试听、制卡截的那一段和循环用同一个区间：循环时调准一次，这里也跟着准
    let duration = song_duration(conn, &song_id)?;
    let whole = region_of(conn, &song_id, &spans, duration, first, last)?;
    let focus = region_of(conn, &song_id, &spans, duration, pos, pos)?;
    Ok(Some(ListenWindow {
        song_id,
        start_sec: (whole.start_sec - preroll_sec.max(0.0)).max(0.0),
        end_sec: whole.end_sec,
        focus_utterance_id: utterance_id,
        focus_start_sec: focus.start_sec,
        focus_end_sec: focus.end_sec,
        utterance_ids: spans[first..=last].iter().map(|s| s.utterance_id).collect(),
    }))
}

// ── 循环区间（#64）──────────────────────────────────────────────
//
// 行时间戳常常不准：开始晚了一点、结束早了一点，循环时听不全。所以循环区间这样算：
//
// - 起点 = 第一行的开始；
// - 终点 = 最后一行**最后一个字的真实结束时刻**（有逐字时间时），没有就用行区间的终点；
// - 再加上记住的微调（`line_loop_adjust`）：起点的记在第一行，终点的记在最后一行。
//
// 两个客户端的播放器只管执行这个区间，不各自再算。

/// 微调最多挪多少（毫秒）。再多就不是「时间轴差一点」，该去改歌词偏移了
pub const MAX_LOOP_DELTA_MS: i64 = 10_000;
/// 循环区间至少多长（秒），免得终点调到起点前面
const MIN_LOOP_SEC: f64 = 0.3;

/// 一段循环区间，交给播放器的起止已经算好了微调。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LoopRegion {
    pub song_id: String,
    pub first_utterance_id: i64,
    pub last_utterance_id: i64,
    /// 交给播放器的起止（含微调，夹在曲目范围内）
    pub start_sec: f64,
    pub end_sec: f64,
    /// 不含微调的起止
    pub base_start_sec: f64,
    pub base_end_sec: f64,
    /// 第一行记住的起点微调、最后一行记住的终点微调（毫秒，负数是提前）
    pub start_delta_ms: i64,
    pub end_delta_ms: i64,
    /// 终点取的是最后一个字的真实结束时刻
    pub end_from_words: bool,
}

/// 从 `first` 行到 `last` 行（可以是同一行）的循环区间。
/// 两行不在同一首歌、或者有一行没有时间戳时返回 None。
pub fn loop_region(conn: &Connection, first: i64, last: i64) -> Result<Option<LoopRegion>> {
    let Some(song_id) = song_of(conn, first)? else { return Ok(None) };
    if song_of(conn, last)?.as_deref() != Some(song_id.as_str()) {
        return Ok(None);
    }
    let spans = line_spans(conn, &song_id)?;
    let (Some(a), Some(b)) = (
        spans.iter().position(|s| s.utterance_id == first),
        spans.iter().position(|s| s.utterance_id == last),
    ) else {
        return Ok(None);
    };
    let duration = song_duration(conn, &song_id)?;
    Ok(Some(region_of(conn, &song_id, &spans, duration, a.min(b), a.max(b))?))
}

/// 改循环区间的微调，返回改完的区间。起点的记在第一行，终点的记在最后一行；
/// 两个都是 0 就是「重置」，那一行不再留记录。超出 ±10 秒的夹到 ±10 秒。
pub fn set_loop_adjust(
    conn: &Connection,
    first: i64,
    last: i64,
    start_delta_ms: i64,
    end_delta_ms: i64,
) -> Result<Option<LoopRegion>> {
    let Some(region) = loop_region(conn, first, last)? else { return Ok(None) };
    let start = start_delta_ms.clamp(-MAX_LOOP_DELTA_MS, MAX_LOOP_DELTA_MS);
    let end = end_delta_ms.clamp(-MAX_LOOP_DELTA_MS, MAX_LOOP_DELTA_MS);
    conn.execute(
        &format!(
            "INSERT INTO line_loop_adjust (utterance_id, start_delta_ms, end_delta_ms, updated_at) \
             VALUES (?1, ?2, 0, {NOW}) \
             ON CONFLICT(utterance_id) DO UPDATE SET start_delta_ms = excluded.start_delta_ms, updated_at = excluded.updated_at"
        ),
        params![region.first_utterance_id, start],
    )?;
    conn.execute(
        &format!(
            "INSERT INTO line_loop_adjust (utterance_id, start_delta_ms, end_delta_ms, updated_at) \
             VALUES (?1, 0, ?2, {NOW}) \
             ON CONFLICT(utterance_id) DO UPDATE SET end_delta_ms = excluded.end_delta_ms, updated_at = excluded.updated_at"
        ),
        params![region.last_utterance_id, end],
    )?;
    conn.execute(
        "DELETE FROM line_loop_adjust WHERE utterance_id IN (?1, ?2) AND start_delta_ms = 0 AND end_delta_ms = 0",
        params![region.first_utterance_id, region.last_utterance_id],
    )?;
    loop_region(conn, first, last)
}

/// 一首歌的循环微调全删掉。换歌词、删歌时调用（行没了，记在行上的微调也没意义了）
pub fn delete_loop_adjust_for_song(conn: &Connection, song_id: &str) -> Result<usize> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'line_loop_adjust')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(0);
    }
    Ok(conn.execute(
        "DELETE FROM line_loop_adjust WHERE utterance_id IN (SELECT id FROM utterances WHERE song_id = ?1)",
        params![song_id],
    )?)
}

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%SZ','now')";

fn song_of(conn: &Connection, utterance_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT song_id FROM live_utterances WHERE id = ?1", params![utterance_id], |r| r.get(0))
        .optional()?)
}

/// 记住的微调 (起点, 终点)，毫秒。没调过、或者库里还没有这张表时是 (0, 0)
fn adjust_of(conn: &Connection, utterance_id: i64) -> Result<(i64, i64)> {
    let row = conn
        .query_row(
            "SELECT start_delta_ms, end_delta_ms FROM line_loop_adjust WHERE utterance_id = ?1",
            params![utterance_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional();
    match row {
        Ok(v) => Ok(v.unwrap_or((0, 0))),
        Err(rusqlite::Error::SqliteFailure(_, Some(msg))) if msg.contains("no such table") => Ok((0, 0)),
        Err(e) => Err(e.into()),
    }
}

/// 这一行最后一个字的真实结束时刻。同一处手动校准和导入的都有时以手动的为准；
/// 最后一段没有结束时刻（只记了开始）就算没有，回退到行区间
fn last_word_end(conn: &Connection, utterance_id: i64) -> Result<Option<f64>> {
    let mut last: Option<(i64, Option<f64>, bool)> = None;
    for w in crate::word_timing::for_utterances(conn, &[utterance_id])? {
        let manual = w.source == crate::word_timing::source::MANUAL;
        match last {
            Some((start, _, _)) if w.char_start < start => {}
            Some((start, _, true)) if w.char_start == start && !manual => {}
            _ => last = Some((w.char_start, w.end_sec, manual)),
        }
    }
    Ok(last.and_then(|(_, end, _)| end))
}

/// `spans[a..=b]` 这几行的循环区间
fn region_of(
    conn: &Connection,
    song_id: &str,
    spans: &[LineSpan],
    duration: Option<f64>,
    a: usize,
    b: usize,
) -> Result<LoopRegion> {
    let (first, last) = (&spans[a], &spans[b]);
    let base_start = first.start_sec;
    let word_end = last_word_end(conn, last.utterance_id)?.filter(|end| *end > last.start_sec);
    let base_end = word_end.unwrap_or(last.end_sec).max(base_start + MIN_LOOP_SEC);
    let (start_delta_ms, _) = adjust_of(conn, first.utterance_id)?;
    let (_, end_delta_ms) = adjust_of(conn, last.utterance_id)?;
    let start = (base_start + start_delta_ms as f64 / 1000.0).max(0.0);
    let mut end = base_end + end_delta_ms as f64 / 1000.0;
    if let Some(total) = duration.filter(|d| *d > start) {
        end = end.min(total);
    }
    let end = end.max(start + MIN_LOOP_SEC);
    Ok(LoopRegion {
        song_id: song_id.to_string(),
        first_utterance_id: first.utterance_id,
        last_utterance_id: last.utterance_id,
        start_sec: start,
        end_sec: end,
        base_start_sec: base_start,
        base_end_sec: base_end,
        start_delta_ms,
        end_delta_ms,
        end_from_words: word_end.is_some(),
    })
}

pub(crate) fn song_lines(conn: &Connection, song_id: &str) -> Result<Vec<TimedLine>> {
    let mut stmt = conn.prepare(
        "SELECT id, line_idx, time_sec, text FROM live_utterances WHERE song_id = ?1 ORDER BY line_idx, id",
    )?;
    let rows = stmt.query_map(params![song_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn song_duration(conn: &Connection, song_id: &str) -> Result<Option<f64>> {
    Ok(conn
        .query_row("SELECT duration_sec FROM live_songs WHERE id = ?1", params![song_id], |r| {
            r.get::<_, Option<f64>>(0)
        })
        .unwrap_or(None))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: i64, t: Option<f64>, text: &str) -> TimedLine {
        (id, id, t, text.to_string())
    }

    #[test]
    fn a_line_ends_where_the_next_one_starts() {
        let spans = spans_from_lines(&[line(1, Some(10.0), "あいうえお"), line(2, Some(13.5), "かきく")], Some(200.0));
        let first = spans[0].as_ref().unwrap();
        assert_eq!(first.end_sec, 13.5);
        assert_eq!(first.end_kind, SpanEnd::NextLine);
    }

    #[test]
    fn a_long_gap_is_an_interlude_and_gets_cut_by_the_estimate() {
        // 下一行在 40 秒后：中间是间奏，不能放 40 秒
        let spans = spans_from_lines(&[line(1, Some(10.0), "あいうえお"), line(2, Some(50.0), "x")], Some(200.0));
        let first = spans[0].as_ref().unwrap();
        assert_eq!(first.end_kind, SpanEnd::Estimated);
        assert!(first.end_sec - first.start_sec <= MAX_ESTIMATE_SEC);
        assert!(first.end_sec - first.start_sec >= MIN_ESTIMATE_SEC);
    }

    #[test]
    fn same_timestamp_lines_do_not_end_each_other_and_untimed_lines_have_no_span() {
        let spans = spans_from_lines(
            &[line(1, Some(10.0), "a"), line(2, Some(10.0), "b"), line(3, None, "c"), line(4, Some(14.0), "d")],
            None,
        );
        assert_eq!(spans[0].as_ref().unwrap().end_sec, 14.0);
        assert!(spans[2].is_none());
        // 最后一行没有下一行、没有时长：按估计
        assert_eq!(spans[3].as_ref().unwrap().end_kind, SpanEnd::Estimated);
    }

    #[test]
    fn the_track_end_caps_the_last_line() {
        let spans = spans_from_lines(&[line(1, Some(100.0), "あいうえおかきくけこ")], Some(102.0));
        let last = spans[0].as_ref().unwrap();
        assert_eq!(last.end_sec, 102.0);
        assert_eq!(last.end_kind, SpanEnd::TrackEnd);
    }

    #[test]
    fn context_window_spans_neighbours_and_is_none_without_timestamps() {
        let corpus = crate::fixture::corpus().unwrap();
        let conn = corpus.connection();
        let id: i64 = conn
            .query_row(
                "SELECT id FROM utterances WHERE song_id='001' AND time_sec IS NOT NULL ORDER BY line_idx LIMIT 1 OFFSET 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let w = context_window(conn, id, 1, 1, DEFAULT_PREROLL_SEC).unwrap().unwrap();
        assert_eq!(w.utterance_ids.len(), 3);
        assert!(w.start_sec < w.focus_start_sec && w.focus_end_sec <= w.end_sec);
        assert!(context_window(conn, -1, 1, 1, 0.0).unwrap().is_none());
    }

    /// 001 里有时间戳的前两行
    fn two_timed_lines(conn: &Connection) -> (i64, i64) {
        let ids: Vec<i64> = conn
            .prepare("SELECT id FROM utterances WHERE song_id='001' AND time_sec IS NOT NULL ORDER BY line_idx LIMIT 2")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        (ids[0], ids[1])
    }

    #[test]
    fn loop_region_ends_at_the_last_sung_word_and_manual_wins() {
        let corpus = crate::fixture::corpus().unwrap();
        let conn = corpus.connection();
        let (a, _) = two_timed_lines(conn);
        let plain = loop_region(conn, a, a).unwrap().unwrap();
        assert!(!plain.end_from_words);
        let start = plain.start_sec;
        conn.execute(
            "INSERT INTO word_timings (utterance_id, char_start, char_end, start_sec, end_sec, source) VALUES \
             (?1, 0, 1, ?2, ?3, 'lrc'), (?1, 1, 2, ?3, ?4, 'lrc'), (?1, 1, 2, ?3, ?5, 'manual')",
            params![a, start, start + 0.5, start + 1.0, start + 1.4],
        )
        .unwrap();
        let r = loop_region(conn, a, a).unwrap().unwrap();
        assert!(r.end_from_words);
        assert!((r.end_sec - (start + 1.4)).abs() < 1e-9, "{r:?}");
    }

    #[test]
    fn loop_adjust_is_stored_on_the_edge_lines_and_reset_removes_it() {
        let corpus = crate::fixture::corpus().unwrap();
        let conn = corpus.connection();
        let (a, b) = two_timed_lines(conn);
        let base = loop_region(conn, a, b).unwrap().unwrap();

        let r = set_loop_adjust(conn, a, b, -300, 500).unwrap().unwrap();
        assert!((r.start_sec - (base.start_sec - 0.3).max(0.0)).abs() < 1e-9);
        assert!((r.end_sec - (base.end_sec + 0.5)).abs() < 1e-9);
        assert_eq!((r.start_delta_ms, r.end_delta_ms), (-300, 500));
        // 起点记在第一行、终点记在最后一行：单独循环第一行，只带起点的微调
        let single = loop_region(conn, a, a).unwrap().unwrap();
        assert_eq!((single.start_delta_ms, single.end_delta_ms), (-300, 0));
        // 试听区间也用调过的
        let w = context_window(conn, a, 0, 1, 0.0).unwrap().unwrap();
        assert!((w.end_sec - r.end_sec).abs() < 1e-9 && (w.start_sec - r.start_sec).abs() < 1e-9);

        // 太大的夹到 ±10 秒；终点不会调到起点前面
        let r = set_loop_adjust(conn, a, b, 0, -99_000).unwrap().unwrap();
        assert_eq!(r.end_delta_ms, -MAX_LOOP_DELTA_MS);
        assert!(r.end_sec > r.start_sec);

        set_loop_adjust(conn, a, b, 0, 0).unwrap();
        let left: i64 = conn.query_row("SELECT COUNT(*) FROM line_loop_adjust", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn loop_adjust_goes_with_the_lines() {
        let corpus = crate::fixture::corpus().unwrap();
        let conn = corpus.connection();
        let (a, b) = two_timed_lines(conn);
        set_loop_adjust(conn, a, b, 200, 200).unwrap();
        crate::word_timing::delete_for_song(conn, "001").unwrap();
        let left: i64 = conn.query_row("SELECT COUNT(*) FROM line_loop_adjust", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
        // 跨歌、没有时间戳的行没有区间
        assert!(loop_region(conn, a, -1).unwrap().is_none());
    }
}
