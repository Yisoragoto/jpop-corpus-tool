//! 命令行跑数据迁移。先在副本上跑一遍看数字，再决定要不要在真库上点那个按钮。
//!
//!     cargo run -p jp-app --example migrate_library -- <源目录> <目标目录> [--run] [--dicts] [--history]
//!
//! 不带 `--run` 就是只预览（只读）。

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let paths: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    anyhow::ensure!(paths.len() >= 2, "用法：migrate_library <源目录> <目标目录> [--run] [--dicts] [--history]");
    let source = PathBuf::from(paths[0]);
    let target_root = PathBuf::from(paths[1]);
    let options = jp_import::migrate::MigrateOptions {
        dictionaries: args.iter().any(|a| a == "--dicts"),
        history: args.iter().any(|a| a == "--history"),
    };

    let target_db = target_root.join("corpus.db");
    anyhow::ensure!(target_db.is_file(), "找不到 {}", target_db.display());
    let mut conn = rusqlite::Connection::open(&target_db)?;

    let plan = jp_app_lib::migrate::plan(&conn, &source)?;
    println!("源库 {} 首歌：要搬 {}，已经有了 {}", plan.source_songs, plan.new_songs, plan.duplicate_songs);
    println!(
        "歌词 {} 行、分词 {}、人 {}、专辑 {}、署名 {}、收听 {}、收藏 {}、校正 {}",
        plan.lyric_lines, plan.tokens, plan.people, plan.albums, plan.credits, plan.plays,
        plan.favorites, plan.corrections
    );
    println!(
        "0.1.x 词典词条 {}，JLPT 缓存 {}，当前库{}词典表",
        plan.dict_terms,
        plan.jlpt_rows,
        if plan.target_has_dictionaries { "已有" } else { "没有" }
    );
    for sample in &plan.samples {
        println!("  · {sample}");
    }

    if !args.iter().any(|a| a == "--run") {
        println!("\n（只预览，没动任何东西。加 --run 才真搬）");
        return Ok(());
    }

    let started = std::time::Instant::now();
    let outcome = jp_app_lib::migrate::run(&mut conn, &source, &target_root, options, |step| {
        println!("… {step}");
    })?;
    println!(
        "\n搬完：歌 {}（跳过 {}）、歌词 {} 行、分词 {}、署名 {}、收听 {}、校正 {}",
        outcome.db.songs, outcome.db.skipped, outcome.db.lyric_lines, outcome.db.tokens,
        outcome.db.credits, outcome.db.plays, outcome.db.corrections
    );
    println!(
        "文件：封面 {}、歌词 {}、歌手照片 {}；词典库{}",
        outcome.covers_copied,
        outcome.lyrics_copied,
        outcome.artist_photos_copied,
        if outcome.dictionaries_copied {
            format!("复制了 {:.0} MB", outcome.dictionaries_bytes as f64 / 1024.0 / 1024.0)
        } else {
            "没复制".to_string()
        }
    );
    println!("词典词条 {}，JLPT {}，用时 {:.1}s", outcome.db.dict_terms, outcome.db.jlpt_rows, started.elapsed().as_secs_f64());
    for warning in &outcome.warnings {
        println!("[warn] {warning}");
    }
    Ok(())
}
