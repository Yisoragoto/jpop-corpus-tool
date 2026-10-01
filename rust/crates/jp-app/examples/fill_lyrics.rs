//! 命令行补齐歌词。修数据不该非得开着 GUI，而且联网的东西要能在真库的
//! **副本**上先跑一遍看结果。
//!
//!     cargo run -p jp-app --example fill_lyrics -- <语料库目录> [--dry-run] [--limit N]
//!
//! `--dry-run` 只搜不写：打印每首歌会被挂上哪一条，用来核对匹配挑得对不对。
//! 不带参数时用 `JPOP_CORPUS_HOME` 或仓库根目录。

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let limit = args
        .iter()
        .position(|a| a == "--limit")
        .and_then(|i| args.get(i + 1))
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(usize::MAX);
    let root = args
        .iter()
        .find(|a| !a.starts_with("--") && a.parse::<usize>().is_err())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("JPOP_CORPUS_HOME").map(PathBuf::from))
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(3)
                .expect("manifest 路径异常")
                .to_path_buf()
        });

    let db = root.join("corpus.db");
    anyhow::ensure!(db.is_file(), "找不到 {}", db.display());
    let lyrics_dir = root.join("raw").join("lyrics_lrc");

    let mut conn = rusqlite::Connection::open(&db)?;
    let targets = jp_app_lib::lyrics::missing(&conn)?;
    println!(
        "{}：{} 首缺歌词（其中 {} 首旁边就有 .lrc）",
        db.display(),
        targets.len(),
        targets.iter().filter(|t| t.sibling_lrc.is_some()).count()
    );

    let analyzer = jp_tokenizer::locate_sudachipy(&root)
        .and_then(|(res, dict)| jp_tokenizer::Analyzer::from_sudachipy(&res, &dict).ok());
    if analyzer.is_none() {
        println!("[warn] 没有分词器，歌词会入库但不分词");
    }
    let provider = jp_app_lib::lyrics::provider();

    let (mut filled, mut not_found, mut failed) = (0, 0, 0);
    for target in targets.iter().take(limit) {
        print!("[{}] {} — {} … ", target.song_id, target.artist, target.title);
        use std::io::Write;
        std::io::stdout().flush().ok();

        if dry_run {
            match provider.best_lyrics(&target.title, &target.artist, target.duration_sec) {
                Ok(Some((hit, text))) => {
                    filled += 1;
                    println!(
                        "命中 {} — {}（{:?}s，{} 行）",
                        hit.title,
                        hit.artist,
                        hit.duration_sec.unwrap_or(0.0).round(),
                        text.lines().count()
                    );
                }
                Ok(None) => {
                    not_found += 1;
                    println!("挑不出来");
                }
                Err(err) => {
                    failed += 1;
                    println!("失败：{err}");
                }
            }
            continue;
        }

        match jp_app_lib::lyrics::fill_one(
            &mut conn,
            &lyrics_dir,
            &provider,
            target,
            analyzer.as_ref(),
        ) {
            Ok(Some(attached)) => {
                filled += 1;
                println!(
                    "{} ← {}（{} 行，{} 词，{} 署名）",
                    attached.source.as_str(),
                    attached.matched,
                    attached.lyric_lines,
                    attached.tokens,
                    attached.credits
                );
            }
            Ok(None) => {
                not_found += 1;
                println!("挑不出来");
            }
            Err(err) => {
                failed += 1;
                println!("失败：{err:#}");
            }
        }
    }

    println!("---\n补上 {filled}，挑不出来 {not_found}，失败 {failed}");
    Ok(())
}
