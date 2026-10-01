//! 拿真语料组几张卡，看内容对不对。**不需要 Anki**。
//!
//!     cargo run -p jp-anki --example probe_card [词...]
//!
//! 单测用的是我造的四行歌词。真库有 209 首歌、5.8 万个 token、
//! 250 万条词条，只有真跑才知道选词、例句、释义拼出来是什么样。

fn main() -> anyhow::Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();
    let conn = rusqlite::Connection::open_with_flags(
        root.join("corpus.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;

    // ── 选词 ──
    let started = std::time::Instant::now();
    let candidates = jp_anki::pick_words(
        &conn,
        &jp_anki::PickOptions {
            limit: 20,
            ..Default::default()
        },
        None,
    )?;
    println!("按频次选出的前 20 个词（{:?}）：", started.elapsed());
    for w in candidates.iter().take(10) {
        println!(
            "  {:<10} {:<6} {:>4} 次 · {:>3} 首歌",
            w.lemma, w.pos, w.count, w.song_count
        );
    }

    // ── 组卡 ──
    let words: Vec<(String, String)> = {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.is_empty() {
            candidates
                .iter()
                .take(3)
                .map(|w| (w.lemma.clone(), w.pos.clone()))
                .collect()
        } else {
            args.into_iter().map(|a| (a, "NOUN".to_string())).collect()
        }
    };

    for (lemma, pos) in &words {
        let started = std::time::Instant::now();
        let card = jp_anki::card::build(
            &conn,
            lemma,
            pos,
            &[],
            jp_anki::CardOptions { max_examples: 2, ..Default::default() },
        )?;
        println!("\n════ {lemma} ════ {:?}", started.elapsed());
        println!("  读音   {}", card.reading);
        println!("  音高   {}", card.pitch);
        println!("  词频   {}", card.freq);
        println!("  JLPT   {}", card.jlpt);
        println!("  词性   {}", card.part_of_speech);
        println!("  出处   {}", card.source);
        for example in &card.examples {
            println!("  例句   {} —— {}", example.text, example.source_label());
        }
        println!(
            "  释义   {} 字符{}",
            card.meaning.chars().count(),
            if card.no_definitions { "（查不到）" } else { "" }
        );
        // 卡片模板要求的字段一个都不能少
        let fields = card.fields();
        let missing: Vec<&str> = jp_anki::FIELDS
            .iter()
            .filter(|f| !fields.contains_key(**f))
            .copied()
            .collect();
        if missing.is_empty() {
            println!("  字段   {} 个，齐全", fields.len());
        } else {
            println!("  ⚠️ 缺字段 {missing:?}");
        }
    }
    Ok(())
}
