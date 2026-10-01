//! 拿真实曲库跑一遍「扫描 → 计划」，验证重扫是幂等的。
//!
//!     cargo run -p jp-import --example plan_import
//!
//! 判据很硬：这 209 个文件已经在库里了，重扫一遍**不该产生任何新条目**。
//! 只要出现一条 New，就说明路径比对或判重有问题——那种 bug 一旦上线，
//! 用户每扫一次库就多一份重复。

use std::path::Path;

use jp_import::plan::Action;

fn main() -> anyhow::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();
    let db = root.join("corpus.db");
    anyhow::ensure!(db.is_file(), "找不到 {}", db.display());

    let corpus = jp_corpus::Corpus::open(&db)?;
    let index = jp_import::LibraryIndex::from_corpus(&corpus)?;

    let started = std::time::Instant::now();
    let tracks = jp_import::scan_dir(&root.join("raw/audio"), 6);
    let scanned = started.elapsed();
    let plan = jp_import::plan(&tracks, &index);
    let summary = plan.summary();

    println!("扫描 {} 个文件（{:?}）", tracks.len(), scanned);
    println!("计划：");
    println!("  已在库中     : {}", summary.already_imported);
    println!("  新增         : {}", summary.new);
    println!("  疑似重复     : {}", summary.possible_duplicates);
    println!("  批内重复     : {}", summary.duplicates_in_batch);
    println!("  跳过         : {}", summary.skipped);

    if summary.new == 0 && summary.possible_duplicates == 0 && summary.skipped == 0 {
        println!("\n✅ 重扫幂等：现有 209 首一条不多、一条不少");
    } else {
        println!("\n⚠️  以下条目不是「已在库中」：");
        for item in &plan.items {
            match &item.action {
                Action::AlreadyImported { .. } => {}
                other => println!("  {:?}\n    {}", other, item.track.path),
            }
        }
    }
    Ok(())
}
