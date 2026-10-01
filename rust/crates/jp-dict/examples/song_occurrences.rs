//! 在真实歌词里找一个词出现在哪些行（只读打开 corpus.db）。制卡时「同一首歌里含这个词的句子」就是这么找的。
//!
//! ```text
//! cargo run -p jp-dict --release --example song_occurrences -- <corpus.db> <歌 id> <词头> [词性规则，空格分隔]
//! ```
//! 例：`... D:/jp_corpus/corpus.db 004 夜`、`... 004 甘える v1`

use std::time::Instant;

use anyhow::Context;
use jp_dict::occurrence::find_occurrences;
use jp_dict::transformer::LanguageTransformer;

fn main() -> anyhow::Result<()> {
    const USAGE: &str = "用法: song_occurrences <corpus.db> <歌 id> <词头> [词性规则]";
    let mut args = std::env::args().skip(1);
    let db = args.next().context(USAGE)?;
    let song = args.next().context(USAGE)?;
    let term = args.next().context(USAGE)?;
    let rules: Vec<String> = args.next().map(|r| r.split_whitespace().map(str::to_owned).collect()).unwrap_or_default();

    let corpus = jp_corpus::Corpus::open(&db)?;
    let lyrics = corpus.lyrics(&song)?;
    let transformer = LanguageTransformer::japanese();
    let conditions = transformer.condition_flags_from_parts_of_speech(&rules);

    let started = Instant::now();
    let mut seen: Vec<String> = Vec::new();
    let mut hits = 0;
    for line in &lyrics {
        let surfaces: Vec<&str> = line.tokens.iter().map(|t| t.surface.as_str()).collect();
        let found = find_occurrences(&transformer, &surfaces, &[term.as_str()], conditions);
        if found.is_empty() {
            continue;
        }
        hits += 1;
        let key: String = line.text.chars().filter(|c| !c.is_whitespace()).collect();
        let duplicate = seen.contains(&key);
        if !duplicate {
            seen.push(key);
        }
        let marked: Vec<String> = found.iter().map(|o| surfaces[o.start..o.end].concat()).collect();
        println!(
            "{:>7} {}  [{}]{}",
            line.time_sec.map(|t| format!("{t:.1}s")).unwrap_or_default(),
            line.text,
            marked.join("、"),
            if duplicate { "  （重复，只加一次）" } else { "" }
        );
    }
    println!("{} 行里 {hits} 行含「{term}」，去重后 {} 句；用时 {:.1?}", lyrics.len(), seen.len(), started.elapsed());
    Ok(())
}
