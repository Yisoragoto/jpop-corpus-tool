//! 曲库：单首曲目。

use anyhow::Result;
use jp_corpus::segments::LoopRegion;
use jp_corpus::{Corpus, Track};
use serde::Deserialize;

use crate::Command;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
pub struct TrackArgs {
    pub song_id: String,
}

/// 按 id 取一首歌；没有这首时是 `null`。
///
/// 两边名字历史上就不一样：Tauri 叫 `get_track`，FFI 叫 `track`。
pub struct GetTrack;

impl Command for GetTrack {
    const TAURI: &'static str = "get_track";
    const FFI: &'static str = "track";
    type Args = TrackArgs;
    type Output = Option<Track>;
    fn run(corpus: &Corpus, args: TrackArgs) -> Result<Option<Track>> {
        corpus.track(&args.song_id)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
pub struct LoopRegionArgs {
    /// 循环的第一行和最后一行；单行循环两个一样
    pub first_utterance_id: i64,
    pub last_utterance_id: i64,
}

/// 从第一行到最后一行的循环区间（含记住的微调）。两行不在同一首、没有时间戳时是 `null`。
/// 循环、试听、制卡截的那一段都用它，规则见 `jp_corpus::segments`。
pub struct GetLoopRegion;

impl Command for GetLoopRegion {
    const TAURI: &'static str = "loop_region";
    const FFI: &'static str = "loopRegion";
    type Args = LoopRegionArgs;
    type Output = Option<LoopRegion>;
    fn run(corpus: &Corpus, args: LoopRegionArgs) -> Result<Option<LoopRegion>> {
        corpus.loop_region(args.first_utterance_id, args.last_utterance_id)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
pub struct LoopAdjustArgs {
    pub first_utterance_id: i64,
    pub last_utterance_id: i64,
    /// 起点、终点挪多少毫秒（负数是提前）。是整个微调量，不是增量；两个都是 0 就是重置
    pub start_delta_ms: i64,
    pub end_delta_ms: i64,
}

/// 记住循环区间的微调，返回调完的区间。起点的记在第一行，终点的记在最后一行。
pub struct SetLoopAdjust;

impl Command for SetLoopAdjust {
    const TAURI: &'static str = "loop_adjust_set";
    const FFI: &'static str = "loopAdjustSet";
    type Args = LoopAdjustArgs;
    type Output = Option<LoopRegion>;
    fn run(corpus: &Corpus, args: LoopAdjustArgs) -> Result<Option<LoopRegion>> {
        corpus.set_loop_adjust(
            args.first_utterance_id,
            args.last_utterance_id,
            args.start_delta_ms,
            args.end_delta_ms,
        )
    }
}
