//! 在真实词典库上量查词：每行歌词从每个字起查一次（和点击查词一样，最多取 16 个字），
//! 统计用时、各阶段占比和命中；再打印几个样例和「夜」在各词频词典里的值。
//!
//! ```text
//! cargo run -p jp-dict --release --example lookup_bench -- <dictionaries.db> <每行一句的文本> [最多行数]
//! ```

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Context;
use jp_dict::store::DictionaryStore;
use jp_dict::translator::{EnabledDictionary, FindTermsMode, FindTermsOptions, LookupTimings, Translator};

/// Yomitan 默认的扫描长度
const SCAN_LENGTH: usize = 16;

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    sorted[((sorted.len() - 1) as f64 * p) as usize]
}

fn run(translator: &Translator, store: &DictionaryStore, options: &FindTermsOptions, lines: &[&str], label: &str) -> anyhow::Result<()> {
    let mut times = Vec::new();
    let mut total = LookupTimings::default();
    let (mut hits, mut empty, mut entries) = (0usize, 0usize, 0usize);
    for line in lines {
        for (i, _) in line.char_indices() {
            let rest: String = line[i..].chars().take(SCAN_LENGTH).collect();
            let started = Instant::now();
            let result = translator.find_terms(store, FindTermsMode::Group, &rest, options)?;
            times.push(started.elapsed());
            total.add(&result.timings);
            if result.dictionary_entries.is_empty() {
                empty += 1;
            } else {
                hits += 1;
                entries += result.dictionary_entries.len();
            }
        }
    }
    if times.is_empty() {
        return Ok(());
    }
    let n = times.len();
    let sum: Duration = times.iter().sum();
    times.sort();
    println!(
        "\n[{label}] 查词 {n} 次：有结果 {hits}（平均 {:.1} 条），无结果 {empty}",
        entries as f64 / hits.max(1) as f64
    );
    println!(
        "  用时 p50 {:.1?}  p90 {:.1?}  p99 {:.1?}  最慢 {:.1?}  平均 {:.1?}",
        percentile(&times, 0.5),
        percentile(&times, 0.9),
        percentile(&times, 0.99),
        times[n - 1],
        sum / n as u32
    );
    let share = |d: Duration| d.as_secs_f64() * 100.0 / sum.as_secs_f64();
    let per = |d: Duration| d / n as u32;
    for (name, d) in [
        ("活用还原", total.deinflect),
        ("查条目", total.find_terms),
        ("建词条", total.build_entries),
        ("分组", total.group),
        ("词频音调", total.term_meta),
        ("标签", total.tags),
        ("排序", total.sort),
        ("读释义", total.glossary),
    ] {
        println!("  {name:<6} 平均 {:>9.2?}  占 {:>5.1}%", per(d), share(d));
    }
    println!(
        "  每次平均：还原候选 {:.0}，查库词 {:.0}，返回行 {:.0}，解压释义 {:.0}",
        total.deinflections as f64 / n as f64,
        total.unique_terms as f64 / n as f64,
        total.rows as f64 / n as f64,
        total.definitions as f64 / n as f64
    );
    Ok(())
}

fn main() -> anyhow::Result<()> {
    const USAGE: &str = "用法: lookup_bench <dictionaries.db> <文本文件> [最多行数]";
    let mut args = std::env::args().skip(1);
    let db = args.next().context(USAGE)?;
    let lines_path = args.next().context(USAGE)?;
    let max_lines: usize = args.next().map(|s| s.parse()).transpose()?.unwrap_or(150);

    let store = DictionaryStore::open(Path::new(&db))?;
    let dictionaries: Vec<EnabledDictionary> = store
        .dictionaries()?
        .into_iter()
        .filter(|d| d.enabled)
        .enumerate()
        .map(|(index, d)| EnabledDictionary {
            alias: d.title.clone(),
            title: d.title,
            index,
            parts_of_speech_filter: true,
            use_deinflections: true,
        })
        .collect();
    println!("启用词典 {} 本", dictionaries.len());
    let mut options = FindTermsOptions {
        dictionaries,
        deinflect: true,
        remove_non_japanese_characters: true,
        primary_reading: String::new(),
        sort_frequency_dictionary: None,
        sort_frequency_ascending: true,
        use_all_frequency_dictionaries: false,
        max_results: None,
    };
    let translator = Translator::new();

    let text = std::fs::read_to_string(&lines_path)?;
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).take(max_lines).collect();
    println!("歌词 {} 行", lines.len());

    run(&translator, &store, &options, &lines, "不截断")?;
    options.max_results = Some(32);
    run(&translator, &store, &options, &lines, "最多 32 条")?;
    options.max_results = None;

    for sample in ["食べちゃった", "夜に駆ける", "打ち込んでいませんでした", "君の名は", "すっっごーーい", "ｶﾞｯｺｳ"] {
        let started = Instant::now();
        let r = translator.find_terms(&store, FindTermsMode::Group, sample, &options)?;
        println!("\n{sample}：命中原文 {} 字，{} 条，{:.1?}", r.original_text_length, r.dictionary_entries.len(), started.elapsed());
        for e in r.dictionary_entries.iter().take(4) {
            let h = &e.headwords[0];
            let inflection = e
                .inflection_rule_chain_candidates
                .first()
                .map(|c| c.inflection_rules.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(" « "))
                .unwrap_or_default();
            println!(
                "  {}【{}】{} 释义 {} 条 词频 {} 音调 {}",
                h.term,
                h.reading,
                if inflection.is_empty() { String::new() } else { format!("〈{inflection}〉") },
                e.definitions.len(),
                e.frequencies.len(),
                e.pronunciations.len()
            );
        }
    }

    let r = translator.find_terms(&store, FindTermsMode::Group, "夜", &options)?;
    println!("\n「夜」各读音的词频：");
    for e in &r.dictionary_entries {
        for f in &e.frequencies {
            let h = &e.headwords[f.headword_index];
            println!(
                "  {}【{}】 {:<16} {}{}",
                h.term,
                h.reading,
                f.dictionary,
                f.display_value.clone().unwrap_or_else(|| f.frequency.to_string()),
                if f.has_reading { "" } else { "（不分读音）" }
            );
        }
    }
    Ok(())
}
