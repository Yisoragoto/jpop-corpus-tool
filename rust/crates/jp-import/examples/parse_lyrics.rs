//! 拿真实 LRC 跑一遍解析，和库里现有的 utterances 对账。
//!
//!     cargo run -p jp-import --example parse_lyrics
//!
//! 对账的意义：Python 侧当初解析这批文件写进了 8,443 行 utterances。
//! Rust 侧解析同样的文件，行数应该接近——差太多说明解析逻辑不一致。
//!
//! 预期会**多出**一些行：旧实现用子串判断把含「作曲」二字的正文
//! 也丢掉了，新实现只丢结构化的信用行。

use std::collections::HashMap;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();
    let lrc_dir = root.join("raw/lyrics_lrc");
    anyhow::ensure!(lrc_dir.is_dir(), "找不到 {}", lrc_dir.display());

    let mut files = 0usize;
    let mut total_lines = 0usize;
    let mut total_credits = 0usize;
    let mut files_with_credits = 0usize;
    let mut untimed_files = 0usize;
    let mut per_song: HashMap<String, usize> = HashMap::new();

    let mut paths: Vec<_> = std::fs::read_dir(&lrc_dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("lrc"))
        .collect();
    paths.sort();

    for path in &paths {
        let parsed = jp_import::parse_lrc_file(path)?;
        files += 1;
        total_lines += parsed.lines.len();
        total_credits += parsed.credits.len();
        if !parsed.credits.is_empty() {
            files_with_credits += 1;
        }
        if parsed.lines.iter().all(|l| l.time_sec.is_none()) && !parsed.lines.is_empty() {
            untimed_files += 1;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            per_song.insert(stem.to_string(), parsed.lines.len());
        }
    }

    println!("解析 {files} 个 LRC");
    println!("  歌词行合计   : {total_lines}");
    println!("  信用条目     : {total_credits}（{files_with_credits} 个文件带信用）");
    println!("  完全无时间轴 : {untimed_files} 个文件");

    // 和库里的 utterances 对账
    let db = root.join("corpus.db");
    if db.is_file() {
        let corpus = jp_corpus::Corpus::open(&db)?;
        let mut matched = 0usize;
        let mut diffs: Vec<(String, usize, usize)> = Vec::new();
        for track in corpus.tracks(1000)? {
            let Some(&parsed_count) = per_song.get(&track.id) else { continue };
            let db_count = corpus.lyrics(&track.id)?.len();
            matched += 1;
            if parsed_count != db_count {
                diffs.push((track.id.clone(), parsed_count, db_count));
            }
        }
        println!("\n和库里对账（{matched} 首）：");
        println!("  行数一致 : {}", matched - diffs.len());
        println!("  行数不同 : {}", diffs.len());
        let more: Vec<_> = diffs.iter().filter(|(_, p, d)| p > d).collect();
        let fewer: Vec<_> = diffs.iter().filter(|(_, p, d)| p < d).collect();
        println!("    Rust 多出 : {} 首", more.len());
        println!("    Rust 少了 : {} 首", fewer.len());
        for (id, p, d) in diffs.iter().take(6) {
            println!("      [{id}] Rust={p} 库={d}");
        }
    }
    Ok(())
}
