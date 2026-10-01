//! 把全部歌词的振假名导出成 JSON，和 Python 版 `_furigana_tokens` 对账（只读打开 corpus.db）。
//!
//! ```text
//! cargo run --release -p jp-app --example dump_furigana -- <输出.json>
//! ```
//! 输出 `{ "<utterance_id>": { "text": ..., "kanji": [[原文, 读音], ...], "word": [...] } }`

use std::collections::BTreeMap;

use anyhow::Context;
use jp_tokenizer::furigana::{FuriganaMode, furigana_segments};

fn main() -> anyhow::Result<()> {
    let output = std::env::args()
        .nth(1)
        .context("用法: dump_furigana <输出.json>")?;
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("manifest 路径异常")
        .to_path_buf();
    let (resources, dict) = jp_tokenizer::locate_sudachipy(&root).context("找不到 Sudachi 词典")?;
    let analyzer = jp_tokenizer::Analyzer::from_sudachipy(&resources, &dict)?;
    let corpus = jp_corpus::Corpus::open(root.join("corpus.db"))?;

    let started = std::time::Instant::now();
    let mut out = BTreeMap::new();
    for track in corpus.tracks(100_000)? {
        for line in corpus.lyrics(&track.id)? {
            let tokens = analyzer.analyze(&line.text)?;
            let mut entry = serde_json::Map::new();
            entry.insert("text".into(), line.text.clone().into());
            entry.insert(
                "kanji".into(),
                serde_json::to_value(furigana_segments(&line.text, &tokens, FuriganaMode::Kanji))?,
            );
            entry.insert(
                "word".into(),
                serde_json::to_value(furigana_segments(&line.text, &tokens, FuriganaMode::Word))?,
            );
            out.insert(
                line.utterance_id.to_string(),
                serde_json::Value::Object(entry),
            );
        }
    }
    let elapsed = started.elapsed();
    std::fs::write(&output, serde_json::to_vec(&out)?)?;
    println!(
        "{} 行，{:.1} 秒（{:.2} ms/行，含两种模式）",
        out.len(),
        elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1000.0 / out.len() as f64
    );
    Ok(())
}
