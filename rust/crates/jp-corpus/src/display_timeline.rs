//! Display only complete, validated, actual timing units. Never estimate clocks
//! or divide a word's interval among its characters. Ranges are Unicode scalars.
//!
//! Coverage includes whitespace/punctuation: an untimed separator is preserved in
//! the text, but the whole line falls back instead of manufacturing a time for it.
//! Only lrc/manual are trusted; unknown/algorithm sources are ignored. A manual
//! unit can replace an identical source range only with its own valid start AND
//! end. Seek-only manual anchors are ignored without disabling valid native units.
//! Next-line/track boundaries may constrain units; estimated listen windows may not.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::{LyricLine, word_timing::WordTiming};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayMode {
    Karaoke,
    LineSynced,
    Plain,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineUnit {
    pub char_start: usize,
    pub char_end: usize,
    pub start_sec: f64,
    pub end_sec: f64,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineTimeline {
    pub utterance_id: i64,
    pub mode: DisplayMode,
    pub units: Vec<TimelineUnit>,
}

fn valid_time(t: f64) -> bool {
    t.is_finite() && t >= 0.0
}

/// `end_sec` must be an actual upper bound, never an estimated singing duration.
/// Other utterances' rows are ignored, so repeated text cannot share clocks.
pub fn for_line(line: &LyricLine, timings: &[WordTiming], end_sec: Option<f64>) -> LineTimeline {
    let fallback = || LineTimeline {
        utterance_id: line.utterance_id,
        mode: if line.time_sec.is_some_and(valid_time) {
            DisplayMode::LineSynced
        } else {
            DisplayMode::Plain
        },
        units: Vec::new(),
    };
    let length = line.text.chars().count();
    if length == 0
        || line.time_sec.is_some_and(|t| !valid_time(t))
        || end_sec
            .is_some_and(|end| !valid_time(end) || line.time_sec.is_some_and(|start| end <= start))
    {
        return fallback();
    }

    let mut ranges = BTreeMap::<(i64, i64), &WordTiming>::new();
    for timing in timings
        .iter()
        .filter(|t| t.utterance_id == line.utterance_id)
    {
        if !matches!(timing.source.as_str(), "lrc" | "manual")
            || (timing.source == "manual" && timing.end_sec.is_none())
        {
            continue;
        }
        let key = (timing.char_start, timing.char_end);
        if let Some(previous) = ranges.get(&key) {
            if previous.source == timing.source
                && (previous.start_sec != timing.start_sec || previous.end_sec != timing.end_sec)
            {
                return fallback();
            }
            if previous.source == "manual" {
                continue;
            }
        }
        ranges.insert(key, timing);
    }

    // Resolve exact-range precedence before checking effective units, matching
    // the React path. An explicit manual interval may repair a missing native end.
    let mut units = Vec::with_capacity(ranges.len());
    let mut cursor = 0;
    let mut previous_end = line.time_sec.unwrap_or(0.0);
    for timing in ranges.into_values() {
        let (Ok(start), Ok(end)) = (
            usize::try_from(timing.char_start),
            usize::try_from(timing.char_end),
        ) else {
            return fallback();
        };
        let Some(until) = timing.end_sec else {
            return fallback();
        };
        if start >= end
            || end > length
            || !valid_time(timing.start_sec)
            || !valid_time(until)
            || until <= timing.start_sec
            || line.time_sec.is_some_and(|begin| timing.start_sec < begin)
            || end_sec.is_some_and(|limit| until > limit)
            || start != cursor
            || timing.start_sec < previous_end
        {
            return fallback();
        }
        units.push(TimelineUnit {
            char_start: start,
            char_end: end,
            start_sec: timing.start_sec,
            end_sec: until,
            source: timing.source.clone(),
        });
        cursor = end;
        previous_end = until;
    }
    if cursor != length {
        return fallback();
    }
    LineTimeline {
        utterance_id: line.utterance_id,
        mode: DisplayMode::Karaoke,
        units,
    }
}

/// One entry per current lyric row, with no change to stored timings or schema.
pub fn for_song(conn: &Connection, song_id: &str) -> Result<Vec<LineTimeline>> {
    let rows = crate::segments::song_lines(conn, song_id)?;
    let duration =
        crate::segments::song_duration(conn, song_id)?.filter(|d| valid_time(*d) && *d > 0.0);
    let mut by_line = HashMap::<i64, Vec<WordTiming>>::new();
    for timing in crate::word_timing::for_song(conn, song_id)? {
        by_line.entry(timing.utterance_id).or_default().push(timing);
    }
    let mut out = Vec::with_capacity(rows.len());
    for (i, (id, index, start, text)) in rows.iter().enumerate() {
        let next = start.filter(|t| valid_time(*t)).and_then(|start| {
            rows[i + 1..]
                .iter()
                .filter_map(|(_, _, time, _)| *time)
                .find(|time| valid_time(*time) && *time > start)
        });
        let end = match (next, duration) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let line = LyricLine {
            utterance_id: *id,
            line_idx: *index,
            time_sec: *start,
            text: text.clone(),
            tokens: Vec::new(),
        };
        out.push(for_line(
            &line,
            by_line.get(id).map_or(&[], Vec::as_slice),
            end,
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line() -> LyricLine {
        LyricLine {
            utterance_id: 1,
            line_idx: 0,
            time_sec: Some(1.0),
            text: "𠮷😀か\u{3099}光".into(),
            tokens: Vec::new(),
        }
    }

    fn unit(start: i64, end: i64, begin: f64, until: Option<f64>, source: &str) -> WordTiming {
        WordTiming {
            utterance_id: 1,
            char_start: start,
            char_end: end,
            start_sec: begin,
            end_sec: until,
            source: source.into(),
        }
    }

    fn source_units() -> Vec<WordTiming> {
        vec![
            unit(0, 2, 1.0, Some(1.4), "lrc"),
            unit(2, 5, 2.0, Some(2.4), "lrc"),
        ]
    }

    #[test]
    fn shared_cross_client_rule_fixtures() {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Case {
            name: String,
            line: LyricLine,
            end_sec: Option<f64>,
            timings: Vec<WordTiming>,
            expected: LineTimeline,
        }
        // This same resource is decoded by the native Swift tests and can be
        // consumed by the React tests; no implementation-derived expectations.
        let cases: Vec<Case> = serde_json::from_str(include_str!(
            "../../../../macos/Tests/JPCoreTests/Fixtures/LyricTimeline.json"
        ))
        .unwrap();
        assert!(cases.len() >= 14);
        for case in cases {
            assert_eq!(
                for_line(&case.line, &case.timings, case.end_sec),
                case.expected,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn unicode_words_keep_their_own_intervals_and_real_silence() {
        let timeline = for_line(&line(), &source_units(), Some(4.0));
        assert_eq!(timeline.mode, DisplayMode::Karaoke);
        assert_eq!(timeline.units.len(), 2, "never split a multi-scalar word");
        assert_eq!(
            (timeline.units[0].char_start, timeline.units[0].char_end),
            (0, 2)
        );
        assert_eq!(
            (timeline.units[0].start_sec, timeline.units[0].end_sec),
            (1.0, 1.4)
        );
        assert_eq!(
            (timeline.units[1].start_sec, timeline.units[1].end_sec),
            (2.0, 2.4)
        );
        let json = serde_json::to_value(&timeline).unwrap();
        assert_eq!(json["mode"], "karaoke");
        assert_eq!(json["units"][1]["charEnd"], 5);
    }

    #[test]
    fn every_missing_invalid_or_conflicting_unit_degrades_the_whole_line() {
        let bad = [
            unit(2, 5, 2.0, None, "lrc"),
            unit(2, 5, f64::NAN, Some(2.4), "lrc"),
            unit(2, 5, 2.0, Some(f64::INFINITY), "lrc"),
            unit(2, 5, 2.0, Some(1.9), "lrc"),
            unit(2, 5, 2.0, Some(2.0), "lrc"),
            unit(2, 6, 2.0, Some(2.4), "lrc"),
            unit(-1, 5, 2.0, Some(2.4), "lrc"),
            unit(2, 5, -1.0, Some(2.4), "lrc"),
            unit(2, 5, 1.3, Some(2.4), "lrc"),
            unit(3, 5, 2.0, Some(2.4), "lrc"),
            unit(1, 5, 2.0, Some(2.4), "lrc"),
            unit(2, 5, 2.0, Some(4.1), "lrc"),
            unit(2, 5, 2.0, Some(2.4), "estimate"),
            unit(2, 5, 2.0, Some(2.4), "future-source"),
        ];
        for invalid in bad {
            let mut units = source_units();
            units[1] = invalid.clone();
            let timeline = for_line(&line(), &units, Some(4.0));
            assert_eq!(timeline.mode, DisplayMode::LineSynced, "{invalid:?}");
            assert!(timeline.units.is_empty());
            assert!(
                serde_json::to_string(&timeline)
                    .unwrap()
                    .contains("lineSynced")
            );
        }
        assert_eq!(
            for_line(&line(), &source_units()[..1], None).mode,
            DisplayMode::LineSynced
        );
        let mut units = source_units();
        units.push(unit(0, 2, 1.1, Some(1.5), "lrc"));
        assert_eq!(
            for_line(&line(), &units, None).mode,
            DisplayMode::LineSynced
        );
    }

    #[test]
    fn only_explicit_manual_units_override_exact_source_ranges() {
        let mut units = source_units();
        units.push(unit(0, 2, 1.1, Some(1.5), "manual"));
        for units in [units.clone(), units.into_iter().rev().collect()] {
            let timeline = for_line(&line(), &units, None);
            assert_eq!(timeline.mode, DisplayMode::Karaoke);
            assert_eq!(timeline.units[0].source, "manual");
            assert_eq!(timeline.units[0].start_sec, 1.1);
        }
        let mut units = source_units();
        units.push(unit(0, 2, 1.1, None, "manual"));
        units.push(unit(-1, 999, f64::NAN, None, "algorithm"));
        let timeline = for_line(&line(), &units, None);
        assert_eq!(timeline.mode, DisplayMode::Karaoke);
        assert_eq!(
            timeline.units[0].start_sec, 1.0,
            "manual anchors do not change animation"
        );
        units.push(unit(0, 1, 1.1, Some(1.5), "manual"));
        assert_eq!(
            for_line(&line(), &units, None).mode,
            DisplayMode::LineSynced
        );
    }

    #[test]
    fn fallback_modes_do_not_manufacture_unit_times() {
        let mut line = line();
        assert_eq!(for_line(&line, &[], None).mode, DisplayMode::LineSynced);
        line.time_sec = None;
        assert_eq!(for_line(&line, &[], None).mode, DisplayMode::Plain);
        assert_eq!(
            for_line(&line, &source_units(), None).mode,
            DisplayMode::Karaoke
        );
        for time in [f64::NAN, f64::INFINITY, -1.0] {
            line.time_sec = Some(time);
            assert_eq!(
                for_line(&line, &source_units(), None).mode,
                DisplayMode::Plain
            );
        }
        line.time_sec = Some(1.0);
        assert_eq!(
            for_line(&line, &source_units(), Some(1.0)).mode,
            DisplayMode::LineSynced
        );
    }

    #[test]
    fn complete_coverage_includes_separators_and_never_uses_other_lines() {
        let mut line = line();
        line.text = "青 空".into();
        let mut units = vec![
            unit(0, 1, 1.0, Some(1.4), "lrc"),
            unit(2, 3, 2.0, Some(2.4), "lrc"),
        ];
        assert_eq!(for_line(&line, &units, None).mode, DisplayMode::LineSynced);
        units[0].char_end = 2;
        assert_eq!(for_line(&line, &units, None).mode, DisplayMode::Karaoke);
        for t in &mut units {
            t.utterance_id = 2;
        }
        assert_eq!(for_line(&line, &units, None).mode, DisplayMode::LineSynced);
    }

    #[test]
    fn database_query_returns_all_modes_and_uses_actual_bounds_not_estimates() {
        let corpus = crate::Corpus::open_in_memory().unwrap();
        corpus.ensure_schema().unwrap();
        let conn = corpus.connection();
        conn.execute_batch("INSERT INTO songs(id,title,artist,duration_sec) VALUES('timeline','試験','架空',40);\
            INSERT INTO utterances(id,song_id,line_idx,time_sec,text) VALUES\
            (1,'timeline',0,1,'青空'),(2,'timeline',1,25,'青空'),(3,'timeline',2,NULL,'文字');\
            INSERT INTO word_timings VALUES(1,0,1,1,10,'lrc'),(1,1,2,12,20,'lrc'),(2,0,1,25,26,'lrc');").unwrap();
        let timelines = for_song(conn, "timeline").unwrap();
        assert_eq!(
            timelines.iter().map(|t| t.mode).collect::<Vec<_>>(),
            [
                DisplayMode::Karaoke,
                DisplayMode::LineSynced,
                DisplayMode::Plain
            ]
        );
        assert_eq!(
            timelines[0].units[1].end_sec, 20.0,
            "the old five-second estimate is not a bound"
        );
        conn.execute(
            "UPDATE word_timings SET end_sec=26 WHERE utterance_id=1 AND char_start=1",
            [],
        )
        .unwrap();
        assert_eq!(
            for_song(conn, "timeline").unwrap()[0].mode,
            DisplayMode::LineSynced
        );
        conn.execute(
            "UPDATE word_timings SET end_sec=NULL WHERE utterance_id=1 AND char_start=1",
            [],
        )
        .unwrap();
        assert_eq!(
            for_song(conn, "timeline").unwrap()[0].mode,
            DisplayMode::LineSynced
        );
    }
}
