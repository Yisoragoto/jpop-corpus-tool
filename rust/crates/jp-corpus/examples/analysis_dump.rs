//! 把第一阶段的分析指标在一个库上全算一遍，输出 JSON，给 `scripts/analysis_reconcile.py` 逐项对账。只读。
//!
//! ```text
//! cargo run -p jp-corpus --release --example analysis_dump -- <corpus.db> <out.json>
//! ```
//!
//! 对账脚本按论文公式独立实现同样的指标（不复用这里的任何代码），从同一个库读数据，
//! 数字要逐项对上：离散度、搭配、对比的显著性、N-gram、词汇丰富度、文字种。

use std::time::Instant;

use jp_corpus::analysis::{
    CollocationMeasure, CollocationQuery, CountUnit, FrequencyQuery, KeynessQuery, NgramQuery, Scope,
};
use jp_corpus::lexical::{LexicalGroup, LexicalQuery};
use jp_corpus::subcorpus::Subcorpus;
use jp_corpus::{Corpus, MatchField};
use serde_json::json;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| "corpus.db".into());
    let out = args.next().unwrap_or_else(|| "analysis_dump.json".into());
    let corpus = Corpus::open(&path)?;
    let t = Instant::now();

    let mut frequency = Vec::new();
    for unit in [CountUnit::Tokens, CountUnit::DistinctLines] {
        for jp_only in [false, true] {
            let scope = Scope { unit, jp_only, ..Default::default() };
            let f = corpus.frequency(&FrequencyQuery { scope: scope.clone(), pos: None, limit: Some(1_000_000) })?;
            frequency.push(json!({ "unit": unit, "jpOnly": jp_only, "result": f }));
        }
    }

    let mut collocations = Vec::new();
    for node in ["君", "夢", "心", "見る"] {
        let q = CollocationQuery {
            node: node.into(),
            field: MatchField::Lemma,
            min_freq: 1,
            measure: CollocationMeasure::LogDice,
            limit: Some(1_000_000),
            ..Default::default()
        };
        collocations.push(json!({ "node": node, "result": corpus.collocations(&q)? }));
    }

    // 目标：2020 年起的歌，参照：其余
    let recent: Vec<String> = {
        let mut stmt = corpus
            .connection()
            .prepare("SELECT id FROM songs WHERE CAST(substr(year,1,4) AS INTEGER) >= 2020 ORDER BY id")?;
        stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    let mut keyness = Vec::new();
    for (max_p, bonferroni) in [(None, false), (Some(0.05), false), (Some(0.05), true)] {
        let q = KeynessQuery {
            target: Subcorpus { song_ids: recent.clone(), ..Default::default() },
            reference: None,
            unit: CountUnit::DistinctLines,
            jp_only: false,
            pos: None,
            min_freq: 1,
            limit: Some(1_000_000),
            max_p,
            bonferroni,
        };
        keyness.push(json!({ "maxP": max_p, "bonferroni": bonferroni, "result": corpus.keyness(&q)? }));
    }

    let mut ngrams = Vec::new();
    for n in 2..=4 {
        for field in [MatchField::Lemma, MatchField::Surface] {
            for jp_only in [false, true] {
                let q = NgramQuery {
                    scope: Scope { unit: CountUnit::Tokens, jp_only, ..Default::default() },
                    n,
                    field,
                    min_freq: 1,
                    min_songs: 1,
                    contains: None,
                    limit: Some(10_000_000),
                };
                ngrams.push(json!({ "n": n, "field": field, "jpOnly": jp_only, "result": corpus.ngrams(&q)? }));
            }
        }
    }

    let mut lexical = Vec::new();
    for group in [LexicalGroup::Whole, LexicalGroup::Performer, LexicalGroup::Song] {
        for unit in [CountUnit::Tokens, CountUnit::DistinctLines] {
            let q = LexicalQuery {
                scope: Scope { unit, ..Default::default() },
                group,
                window: 50,
                min_tokens: 1,
                limit: Some(100_000),
            };
            lexical.push(json!({ "group": group, "unit": unit, "result": corpus.lexical_profile(&q)? }));
        }
    }

    let dump = json!({
        "recent": recent,
        "frequency": frequency,
        "collocations": collocations,
        "keyness": keyness,
        "ngrams": ngrams,
        "lexical": lexical,
    });
    std::fs::write(&out, serde_json::to_vec(&dump)?)?;
    eprintln!("写出 {out}（{:?}）", t.elapsed());
    Ok(())
}
