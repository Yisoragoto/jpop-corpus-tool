//! 几组 KWIC 检索的结果导出成 JSON，和 Python 版 `SearchWorker` 对账（只读打开库）。
//!
//! ```text
//! cargo run --release -p jp-corpus --example kwic_dump -- <corpus.db> <输出.json>
//! ```

use anyhow::Context;
use jp_corpus::{KwicQuery, MatchField};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let db = args.next().context("用法: kwic_dump <corpus.db> <输出.json>")?;
    let output = args.next().context("用法: kwic_dump <corpus.db> <输出.json>")?;
    let corpus = jp_corpus::Corpus::open(&db)?;
    let words = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let mixed = words(&["I", "love", "you", "Love", "愛", "君", "yeah", "Yeah", "oh", "Oh", "1", "2"]);
    let cases = [
        ("surface_jp", KwicQuery { keywords: mixed.clone(), jp_only: true, ..Default::default() }),
        ("surface_all", KwicQuery { keywords: mixed.clone(), jp_only: false, ..Default::default() }),
        ("surface_jp_nodedup", KwicQuery { keywords: mixed.clone(), jp_only: true, dedup: false, ..Default::default() }),
        ("lemma_jp", KwicQuery { keywords: mixed, field: MatchField::Lemma, jp_only: true, ..Default::default() }),
    ];
    let mut out = serde_json::Map::new();
    for (name, query) in cases {
        let hits = corpus.kwic(&query)?;
        out.insert(
            name.to_owned(),
            hits.iter().map(|h| serde_json::json!([h.utterance_id, h.keyword, h.time_sec])).collect(),
        );
    }
    std::fs::write(&output, serde_json::to_vec_pretty(&out)?)?;
    Ok(())
}
