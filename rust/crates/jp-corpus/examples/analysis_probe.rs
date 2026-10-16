//! 在真实库上跑一遍新的分析接口，打印结果和耗时。只读。
//!
//! ```text
//! cargo run -p jp-corpus --release --example analysis_probe -- <corpus.db>
//! ```

use std::time::Instant;

use jp_corpus::analysis::{CollocationQuery, CountUnit, FrequencyQuery, KeynessQuery, Scope};
use jp_corpus::subcorpus::Subcorpus;
use jp_corpus::{Corpus, KwicQuery, MatchField, NearFilter};

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "corpus.db".into());
    let corpus = Corpus::open(&path)?;

    let t = Instant::now();
    let snap = corpus.snapshot()?;
    println!("snapshot {:?}  ({:?})", snap, t.elapsed());

    for unit in [CountUnit::Tokens, CountUnit::DistinctLines] {
        let t = Instant::now();
        let f = corpus.frequency(&FrequencyQuery {
            scope: Scope { unit, jp_only: true, ..Default::default() },
            pos: Some("NOUN".into()),
            limit: Some(10),
        })?;
        println!("\nfrequency NOUN {:?}: {} 词次, {} 首  ({:?})", unit, f.provenance.tokens, f.provenance.songs, t.elapsed());
        for i in &f.items {
            println!(
                "  {:<8} freq {:>4}  raw {:>4}  distinct {:>4}  repeat {:>4.0}%  songs {:>3}  per10k {:.1}",
                i.lemma, i.freq, i.raw_freq, i.distinct_freq, i.repeat_share * 100.0, i.songs, i.per_10k
            );
        }
    }

    // 副歌堆出来的词：全部词次里重复占比最高的
    let all = corpus.frequency(&FrequencyQuery {
        scope: Scope { jp_only: true, ..Default::default() },
        pos: None,
        limit: Some(100_000),
    })?;
    let mut inflated: Vec<_> = all.items.iter().filter(|i| i.raw_freq >= 20).collect();
    inflated.sort_by(|a, b| b.repeat_share.partial_cmp(&a.repeat_share).unwrap());
    println!("\n重复占比最高的常见词（raw ≥ 20）：");
    for i in inflated.iter().take(8) {
        println!("  {} ({}) raw {} distinct {} → {:.0}% 来自重复行", i.lemma, i.pos, i.raw_freq, i.distinct_freq, i.repeat_share * 100.0);
    }

    let t = Instant::now();
    let c = corpus.collocations(&CollocationQuery {
        node: "君".into(),
        scope: Scope { unit: CountUnit::DistinctLines, jp_only: true, ..Default::default() },
        min_freq: 3,
        limit: Some(12),
        ..Default::default()
    })?;
    println!("\ncollocations 君 (node {}) ({:?})", c.node_freq, t.elapsed());
    for x in &c.collocates {
        println!(
            "  {:<6} {:<5} O {:>3} (L{} R{})  fc {:>4}  logDice {:.2}  MI {:.2}  LL {:.1}  songs {}",
            x.lemma, x.pos, x.freq, x.freq_left, x.freq_right, x.collocate_freq, x.log_dice, x.mi, x.log_likelihood, x.songs
        );
    }

    // 年代对比：2015 年起 vs 之前
    let t = Instant::now();
    let k = corpus.keyness(&KeynessQuery {
        target: Subcorpus { year_from: Some(2015), ..Default::default() },
        reference: Some(Subcorpus { year_to: Some(2014), ..Default::default() }),
        unit: CountUnit::DistinctLines,
        jp_only: true,
        min_freq: 5,
        limit: Some(8),
        ..Default::default()
    })?;
    println!(
        "\nkeyness 2015– ({} 首 {} 词) vs –2014 ({} 首 {} 词) ({:?})",
        k.target.songs, k.target.tokens, k.reference.songs, k.reference.tokens, t.elapsed()
    );
    for x in &k.over {
        println!("  + {:<6} {:<5} {:>4} vs {:>4}  LL {:.1}  LR {:.2}", x.lemma, x.pos, x.target_freq, x.reference_freq, x.log_likelihood, x.log_ratio);
    }
    for x in &k.under {
        println!("  - {:<6} {:<5} {:>4} vs {:>4}  LL {:.1}  LR {:.2}", x.lemma, x.pos, x.target_freq, x.reference_freq, x.log_likelihood, x.log_ratio);
    }

    for (label, query) in [
        ("て+しまう (lemma)", KwicQuery { keywords: vec!["て+しまう".into()], field: MatchField::Lemma, ..Default::default() }),
        (
            "君 near 好き",
            KwicQuery {
                keywords: vec!["君".into()],
                field: MatchField::Lemma,
                near: Some(NearFilter { word: "好き".into(), field: MatchField::Lemma, window: 4 }),
                ..Default::default()
            },
        ),
        ("夜 (lemma)", KwicQuery { keywords: vec!["夜".into()], field: MatchField::Lemma, ..Default::default() }),
    ] {
        let t = Instant::now();
        let hits = corpus.kwic(&query)?;
        let timed = hits.iter().filter(|h| h.end_sec.is_some()).count();
        println!("\nkwic {label}: {} 条, {} 条有区间 ({:?})", hits.len(), timed, t.elapsed());
        for h in hits.iter().take(4) {
            println!(
                "  [{}]{}[{}]  {} · {}  {:.2}–{:.2} {:?}",
                h.left, h.keyword, h.right, h.artist, h.title,
                h.time_sec.unwrap_or(-1.0), h.end_sec.unwrap_or(-1.0), h.end_kind
            );
        }
    }

    let song = corpus.tracks(1)?.into_iter().next().map(|t| t.id);
    if let Some(song) = song {
        let t = Instant::now();
        let p = corpus.song_profile(&song, 8)?;
        println!(
            "\nsong_profile {song}: {} 行 ({} 重复), {} 词 ({} 去重) ({:?})",
            p.lines, p.repeated_lines, p.tokens, p.distinct_tokens, t.elapsed()
        );
        for x in &p.keywords {
            println!("  {} {} LL {:.1}", x.lemma, x.target_freq, x.log_likelihood);
        }
    }
    Ok(())
}
