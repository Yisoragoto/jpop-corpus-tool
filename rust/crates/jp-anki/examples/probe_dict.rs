//! 拿真库查几个词，看词典层读出来的东西对不对。
//!
//!     cargo run -p jp-anki --example probe_dict [词...]
//!
//! 单测用的是我自己造的几行数据，验的是「我以为表长这样」。
//! 真库里有 250 万条词条、25 部词典、各种历史格式，只有真查才知道。

fn main() -> anyhow::Result<()> {
    let words: Vec<String> = {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.is_empty() {
            ["夜", "駆ける", "群青", "きれい", "する", "ずっと"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            args
        }
    };

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();
    let conn = rusqlite::Connection::open_with_flags(
        root.join("corpus.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;

    let enabled: i64 = conn.query_row(
        "SELECT COUNT(*) FROM dict_registry WHERE enabled=1 AND dict_type='terms'",
        [],
        |r| r.get(0),
    )?;
    println!("启用的词典：{enabled} 部\n");

    for word in &words {
        let started = std::time::Instant::now();
        let info = jp_anki::dict::lookup(&conn, word)?;
        println!(
            "【{word}】{} · 音高 {:?} · 词频 {:?} · JLPT {:?} · {:?}",
            if info.reading.is_empty() {
                "(无读音)".to_string()
            } else {
                info.reading.clone()
            },
            info.pitch,
            info.freq,
            info.jlpt,
            started.elapsed()
        );
        if info.definitions.is_empty() {
            println!("    (查不到释义)");
        }
        let mut last_source = String::new();
        for d in info.definitions.iter().take(6) {
            if d.source != last_source {
                println!("    ── {} ──", d.source);
                last_source = d.source.clone();
            }
            let text: String = d.text.chars().take(70).collect();
            println!("    {text}");
        }
        if info.definitions.len() > 6 {
            println!("    …还有 {} 条", info.definitions.len() - 6);
        }
        println!();
    }
    Ok(())
}
