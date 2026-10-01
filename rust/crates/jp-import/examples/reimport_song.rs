//! 把库里一首被删掉的歌重新导入一遍。给「删歌再导入时分词校正能不能恢复」
//! 做跨工具验证用。
//!
//!     cargo run --release -p jp-import --example reimport_song -- <库> <音频文件>
//!
//! 扫描音频所在目录，按库里现有曲目做计划，只执行判为「新增」的条目——
//! 库里还在的歌判成「已在库中」，一行不动。
//!
//! **拒绝往真库里写**：这是给临时副本用的。输出一行 JSON，方便外部脚本解析。

use std::path::Path;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(db), Some(audio)) = (args.next(), args.next()) else {
        eprintln!("用法: reimport_song <库路径> <音频文件>");
        std::process::exit(2);
    };
    let db = Path::new(&db);
    let audio = Path::new(&audio);

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();
    let real = root.join("corpus.db");
    if real.exists() && std::fs::canonicalize(db)? == std::fs::canonicalize(&real)? {
        anyhow::bail!("{} 是真库，不往里写", real.display());
    }

    let (resource_dir, system_dict) = jp_tokenizer::locate_sudachipy(&root)
        .ok_or_else(|| anyhow::anyhow!("找不到 SudachiPy 词典"))?;
    let analyzer = jp_tokenizer::Analyzer::from_sudachipy(&resource_dir, &system_dict)?;

    let index = {
        let corpus = jp_corpus::Corpus::open(db)?;
        jp_import::LibraryIndex::from_corpus(&corpus)?
    };

    let folder = audio
        .parent()
        .ok_or_else(|| anyhow::anyhow!("音频文件没有上级目录"))?;
    let wanted = std::fs::canonicalize(audio)?;
    let tracks: Vec<_> = jp_import::scan_dir(folder, 1)
        .into_iter()
        .filter(|t| std::fs::canonicalize(&t.path).ok().as_deref() == Some(wanted.as_path()))
        .collect();
    anyhow::ensure!(!tracks.is_empty(), "扫描不到 {}", audio.display());

    let plan = jp_import::plan(&tracks, &index);
    let mut conn = rusqlite::Connection::open(db)?;
    let report = jp_import::execute_with_progress(&mut conn, &plan, Some(&analyzer), |_, _, _| {})?;

    println!(
        "{}",
        serde_json::json!({
            "plannedNew": plan.summary().new,
            "imported": report.imported(),
            "failed": report.failed(),
            "correctionsRestored": report.corrections_restored(),
            "tracks": report.tracks,
        })
    );
    Ok(())
}
