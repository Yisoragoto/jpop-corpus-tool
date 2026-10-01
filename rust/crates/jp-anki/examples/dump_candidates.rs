//! 查词候选和 Python 逐项对账。
//!
//!     cargo run --release -p jp-anki --example dump_candidates -- <基准.json> <输出.json>
//!
//! 基准是 Python 侧导出的（lemma / pos / surface / candidates / lookupTerm）。
//! 对每个词条用 Rust 重算同样几项，按同样的顺序写出来。**只读**打开 corpus.db。

use std::path::Path;

use serde::Deserialize;

#[derive(Deserialize)]
struct Entry {
    lemma: String,
    pos: String,
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        eprintln!("用法: dump_candidates <基准.json> <输出.json>");
        std::process::exit(2);
    };

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();
    let conn = rusqlite::Connection::open_with_flags(
        root.join("corpus.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;

    let entries: Vec<Entry> = serde_json::from_str(&std::fs::read_to_string(&input)?)?;
    let started = std::time::Instant::now();
    let mut out = Vec::with_capacity(entries.len());
    for entry in &entries {
        let surface = jp_anki::dict::common_surface(&conn, &entry.lemma, &entry.pos)?;
        let candidates = jp_anki::dict::lookup_candidates(&entry.lemma, &entry.pos, &surface);
        let found = jp_anki::dict::lookup_with_candidates(&conn, &entry.lemma, &entry.pos, &surface)?;
        // Python 基准里查不到时 lookupTerm 记的是空串
        let hit = !found.definitions.is_empty() || !found.reading.is_empty();
        out.push(serde_json::json!({
            "lemma": entry.lemma,
            "pos": entry.pos,
            "surface": surface,
            "candidates": candidates,
            "lookupTerm": if hit { found.term } else { String::new() },
        }));
    }
    std::fs::write(&output, serde_json::to_string(&out)?)?;
    println!("{} 个词条，用时 {:?} → {output}", out.len(), started.elapsed());
    Ok(())
}
