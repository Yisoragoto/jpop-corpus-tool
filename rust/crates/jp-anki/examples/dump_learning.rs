//! 把学习状态导成 JSON，用来和 Python 的 `anki_learning.py` 逐词对账。
//!
//!     cargo run -p jp-anki --example dump_learning -- <out.json>

fn main() -> anyhow::Result<()> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "learning.json".into());
    let state = jp_anki::learning::load(None, "")?;
    let mut map = std::collections::BTreeMap::new();
    for (word, s) in &state.words {
        let mut decks: Vec<&str> = s.decks.iter().map(String::as_str).collect();
        decks.sort_unstable();
        map.insert(
            word.clone(),
            serde_json::json!([s.note_count, s.card_count, s.max_reps, s.studied, decks]),
        );
    }
    std::fs::write(&out, serde_json::to_string(&map)?)?;
    println!("{} 词 → {out}", map.len());
    Ok(())
}
