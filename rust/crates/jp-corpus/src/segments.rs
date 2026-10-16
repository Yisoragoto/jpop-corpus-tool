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
use rusqlite::{Connection, params};
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
        .query_row("SELECT song_id FROM utterances WHERE id = ?1", params![utterance_id], |r| r.get(0))
        .ok();
    let Some(song_id) = song_id else { return Ok(None) };
    let spans = line_spans(conn, &song_id)?;
    let Some(pos) = spans.iter().position(|s| s.utterance_id == utterance_id) else {
        return Ok(None);
    };
    let first = pos.saturating_sub(before);
    let last = (pos + after).min(spans.len() - 1);
    let focus = &spans[pos];
    Ok(Some(ListenWindow {
        song_id,
        start_sec: (spans[first].start_sec - preroll_sec.max(0.0)).max(0.0),
        end_sec: spans[last].end_sec,
        focus_utterance_id: utterance_id,
        focus_start_sec: focus.start_sec,
        focus_end_sec: focus.end_sec,
        utterance_ids: spans[first..=last].iter().map(|s| s.utterance_id).collect(),
    }))
}

pub(crate) fn song_lines(conn: &Connection, song_id: &str) -> Result<Vec<TimedLine>> {
    let mut stmt = conn.prepare(
        "SELECT id, line_idx, time_sec, text FROM utterances WHERE song_id = ?1 ORDER BY line_idx, id",
    )?;
    let rows = stmt.query_map(params![song_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn song_duration(conn: &Connection, song_id: &str) -> Result<Option<f64>> {
    Ok(conn
        .query_row("SELECT duration_sec FROM songs WHERE id = ?1", params![song_id], |r| {
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
}
