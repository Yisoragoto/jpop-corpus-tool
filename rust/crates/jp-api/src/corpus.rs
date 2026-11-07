//! 语料总览这一类：整库的计数、按年份的统计。

use anyhow::Result;
use jp_corpus::{Corpus, Overview, YearStats};
use serde::Serialize;

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

/// 导航栏底部状态点上的数：库里有几首要人处理。只数在库的歌（已移出的不算）
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CorpusStatus {
    /// 有歌词但没分词：点词查不了、没有振假名
    pub untokenized: usize,
    /// 记的路径上找不到音频文件
    pub missing_audio: usize,
}

pub struct GetCorpusStatus;

impl Command for GetCorpusStatus {
    const TAURI: &'static str = "corpus_status";
    const FFI: &'static str = "corpusStatus";
    type Args = NoArgs;
    type Output = CorpusStatus;
    fn run(corpus: &Corpus, _: NoArgs) -> Result<CorpusStatus> {
        let conn = corpus.connection();
        Ok(CorpusStatus {
            untokenized: jp_import::maintain::untokenized_songs(conn)?.len(),
            missing_audio: jp_import::maintain::missing_audio(conn)?.len(),
        })
    }
}
