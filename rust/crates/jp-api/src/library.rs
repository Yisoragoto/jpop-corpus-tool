//! 曲库：单首曲目。

use anyhow::Result;
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
