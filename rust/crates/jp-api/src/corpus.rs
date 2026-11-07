//! 语料总览这一类：整库的计数、按年份的统计。

use anyhow::Result;
use jp_corpus::{Corpus, Overview, YearStats};

use crate::{Command, NoArgs};

/// 语料总览：曲目、专辑、人、歌词行、词数……
pub struct GetOverview;

impl Command for GetOverview {
    const TAURI: &'static str = "overview";
    const FFI: &'static str = "overview";
    type Args = NoArgs;
    type Output = Overview;
    fn run(corpus: &Corpus, _: NoArgs) -> Result<Overview> {
        corpus.overview()
    }
}

/// 按年份的统计（分析页的时间线）
pub struct GetTimeline;

impl Command for GetTimeline {
    const TAURI: &'static str = "timeline";
    const FFI: &'static str = "timeline";
    type Args = NoArgs;
    type Output = Vec<YearStats>;
    fn run(corpus: &Corpus, _: NoArgs) -> Result<Vec<YearStats>> {
        corpus.timeline()
    }
}
