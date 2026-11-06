//! 检索：全局搜索（Windows 的 Ctrl+K、macOS 的搜索框）。

use anyhow::Result;
use jp_corpus::{Corpus, QuickSearchResults};
use serde::Deserialize;

use crate::Command;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
pub struct QuickSearchArgs {
    pub query: String,
    /// 每种结果最多几条；不传用 jp-corpus 的默认
    pub per_kind: Option<i64>,
}

/// 跨曲目、专辑、人物、词汇、歌词的搜索，按类型分组返回
pub struct QuickSearch;

impl Command for QuickSearch {
    const TAURI: &'static str = "quick_search";
    const FFI: &'static str = "quickSearch";
    type Args = QuickSearchArgs;
    type Output = QuickSearchResults;
    fn run(corpus: &Corpus, args: QuickSearchArgs) -> Result<QuickSearchResults> {
        corpus.quick_search(&args.query, args.per_kind)
    }
}
