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
use jp_corpus::morphology::{InflectionQuery, OrthographyQuery};
use jp_corpus::cql::CqlQuery;
use jp_corpus::explore::{BreakdownQuery, GroupBy, NetworkQuery};
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

    // 形态信息（第二阶段）：库要先补过（jp-import 的 annotate_copy 例子），否则两张表都是空的
    let tokens_scope = Scope { unit: CountUnit::Tokens, ..Default::default() };
    let inflection = corpus.inflection(&InflectionQuery { scope: tokens_scope.clone(), pos: None, examples: Some(6) })?;
    let orthography = corpus.orthography(&OrthographyQuery {
        scope: tokens_scope,
        min_freq: 1,
        min_form_freq: 1,
        content_only: true,
        limit: Some(1_000_000),
    })?;

    // 第三阶段：CQL、频率分解、搭配网络
    let cql_queries = [
        r#"[lemma="君"] [pos="ADP"]"#,
        r#"[pos="VERB" & cform="命令形.*"]"#,
        r#"[lemma="て"] []{0,2} [lemma="しまう"]"#,
        r#"[pos!="PUNCT" & pos!="SYM"]{6,}"#,
        r#"[reading="きみ"]"#,
        r#"[norm="会う"] [lemma="たい"]"#,
        r#""夢" []? [pos="NOUN"]"#,
        r#"[(pos="VERB" | pos="ADJ") & !cform="連用形.*"] [lemma="よ|ね"]"#,
    ];
    let mut cql = Vec::new();
    for q in cql_queries {
        let hits = corpus.cql(&CqlQuery { query: q.into(), dedup: false, ..Default::default() })?;
        let rows: Vec<_> = hits.iter().map(|h| json!([h.utterance_id, h.match_len, h.keyword])).collect();
        cql.push(json!({ "query": q, "hits": rows }));
    }
    let kwic_dream = jp_corpus::KwicQuery { keywords: vec!["夢".into()], field: jp_corpus::MatchField::Lemma, ..Default::default() };
    let mut breakdowns = Vec::new();
    for by in [GroupBy::Year, GroupBy::Performer] {
        breakdowns.push(json!({ "source": "kwic:夢", "by": by, "result": corpus.breakdown(&BreakdownQuery { by, kwic: Some(kwic_dream.clone()), cql: None })? }));
    }
    breakdowns.push(json!({ "source": "cql:命令形", "by": GroupBy::Year, "result": corpus.breakdown(&BreakdownQuery {
        by: GroupBy::Year,
        kwic: None,
        cql: Some(CqlQuery { query: cql_queries[1].into(), ..Default::default() }),
    })? }));
    let network = corpus.collocation_network(&NetworkQuery {
        collocation: CollocationQuery { node: "君".into(), field: MatchField::Lemma, content_only: true, ..Default::default() },
        size: 20,
        min_pair: 2,
        per_node: 3,
    })?;

    let dump = json!({
        "cql": cql,
        "breakdowns": breakdowns,
        "network": network,
        "inflection": inflection,
        "orthography": orthography,
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
