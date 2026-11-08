//! 改读音（#63）：歌词里一个词的振假名注错了（当て字、義訓），改成对的。
//!
//! 按写法记，默认只管这首歌，可以勾「全库同一写法」。只改显示的读音，不改分词和统计。
//! 规则怎么盖到振假名上见 `jp_corpus::ruby`。

use anyhow::Result;
use jp_corpus::Corpus;
use jp_corpus::ruby::ReadingRule;
use serde::Deserialize;

use crate::Command;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
pub struct ReadingRuleArgs {
    pub song_id: String,
    /// 词的写法（分出来的那个词）
    pub surface: String,
}

/// 这首歌里这个写法现在用的读音规则（本首歌的优先，其次全库的）。没改过是 `null`。
pub struct GetReadingRule;

impl Command for GetReadingRule {
    const TAURI: &'static str = "reading_rule";
    const FFI: &'static str = "readingRule";
    type Args = ReadingRuleArgs;
    type Output = Option<ReadingRule>;
    fn run(corpus: &Corpus, args: ReadingRuleArgs) -> Result<Option<ReadingRule>> {
        jp_corpus::ruby::rule_for(corpus.connection(), &args.song_id, &args.surface)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
pub struct SetReadingRuleArgs {
    pub song_id: String,
    pub surface: String,
    pub reading: String,
    /// 全库同一写法都这么读；不勾只管这首歌
    #[serde(default)]
    pub whole_library: bool,
}

/// 改读音。勾了全库时，这首歌自己那条同写法的规则去掉（不然它会盖住全库的）。
pub struct SetReadingRule;

impl Command for SetReadingRule {
    const TAURI: &'static str = "reading_rule_set";
    const FFI: &'static str = "readingRuleSet";
    type Args = SetReadingRuleArgs;
    type Output = ReadingRule;
    fn run(corpus: &Corpus, args: SetReadingRuleArgs) -> Result<ReadingRule> {
        jp_corpus::ruby::set_rule(corpus.connection(), &args.song_id, &args.surface, &args.reading, args.whole_library)
    }
}

/// 恢复自动：去掉这首歌里这个写法现在用的那条规则，返回去掉的那条（没改过是 `null`）。
pub struct ClearReadingRule;

impl Command for ClearReadingRule {
    const TAURI: &'static str = "reading_rule_clear";
    const FFI: &'static str = "readingRuleClear";
    type Args = ReadingRuleArgs;
    type Output = Option<ReadingRule>;
    fn run(corpus: &Corpus, args: ReadingRuleArgs) -> Result<Option<ReadingRule>> {
        jp_corpus::ruby::clear_rule(corpus.connection(), &args.song_id, &args.surface)
    }
}
