//! 给没有封面的歌补封面：用刮削时存下的候选重新取图，不重新搜索。
//!
//! ```text
//! cargo run --release -p jp-app --example backfill_covers -- <corpus.db> <封面目录> [--apply]
//! ```
//!
//! 默认是**试跑**：图会下到给的封面目录里，但不写库。加 `--apply` 才写 `songs.cover_path`。
//! 试跑时用库的副本，别拿真库——图下到哪都行，但库不该被试跑碰。

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let usage = "用法: backfill_covers <corpus.db> <封面目录> [--apply]";
    let db = args.next().context(usage)?;
    let covers = args.next().context(usage)?;
    let apply = args.next().as_deref() == Some("--apply");

    let conn = rusqlite::Connection::open(&db)?;
    let covers_dir = std::path::PathBuf::from(&covers);
    std::fs::create_dir_all(&covers_dir)?;

    let started = std::time::Instant::now();
    let report = jp_app_lib::scrape::backfill_covers(&conn, &covers_dir, !apply)?;
    println!(
        "{} 首没有封面：补上 {}，没有候选 {}，还是下不到 {}（{:.1}s）",
        report.checked,
        report.filled,
        report.no_candidate,
        report.failed,
        started.elapsed().as_secs_f64()
    );
    for line in &report.samples {
        println!("  {line}");
    }
    if apply {
        // 和界面上那颗按钮做的事一样：曲目封面补上了，专辑封面跟着填
        let albums = jp_app_lib::scrape::fill_album_artwork(&conn)?;
        println!("专辑封面填了 {albums} 张");
    } else {
        println!("（试跑：库没有被写）");
    }
    Ok(())
}
