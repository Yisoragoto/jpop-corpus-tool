//! 押韵统计的结果按 JSON 打出来，给外部脚本（按 `phonology_dump` 的发音独立重算）对账。
//!
//! `cargo run -p jp-corpus --example rhyme_dump -- <corpus.db 副本>`

use jp_corpus::Corpus;
use jp_corpus::analysis::{CountUnit, Scope};
use jp_corpus::phonology::{RhymePosition, RhymeQuery, RhymeStatsQuery};

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("corpus.db copy path");
    let corpus = Corpus::open(&path)?;
    let mut out = serde_json::Map::new();
    for (name, position, strict, unit) in [
        ("end", RhymePosition::End, false, CountUnit::Tokens),
        ("end_strict", RhymePosition::End, true, CountUnit::Tokens),
        ("end_distinct", RhymePosition::End, false, CountUnit::DistinctLines),
        ("start", RhymePosition::Start, false, CountUnit::Tokens),
    ] {
        let scope = Scope { unit, ..Scope::default() };
        let r = corpus.rhyme_stats(&RhymeStatsQuery { scope, position, strict, limit: 20, ..RhymeStatsQuery::default() })?;
        out.insert(name.into(), serde_json::to_value(r)?);
    }
    for (name, pattern, position, strict) in [
        ("q_ai_end", "ai", RhymePosition::End, false),
        ("q_oia_end", "oia", RhymePosition::End, false),
        ("q_an_end_strict", "an", RhymePosition::End, true),
        ("q_kimi_start", "kimi", RhymePosition::Start, false),
        ("q_aiai_any", "aiai", RhymePosition::Any, false),
    ] {
        let r = corpus.rhyme(&RhymeQuery { pattern: pattern.into(), position, strict, limit: 100000, ..RhymeQuery::default() })?;
        out.insert(name.into(), serde_json::json!({ "lines": r.lines, "songs": r.songs, "ids": r.hits.iter().map(|h| h.utterance_id).collect::<Vec<_>>() }));
    }
    println!("{}", serde_json::Value::Object(out));
    Ok(())
}
