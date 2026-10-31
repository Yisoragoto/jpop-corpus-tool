//! 拿真实语料库的**副本**验证音频文件记录：迁移、回填、第一次和第二次重扫各花多久、
//! 判出来的处置分布。会改库（迁移 + 回填 + 记文件信息），所以只能给副本用。
//!
//!     cargo run --release -p jp-import --example rescan_check -- <副本目录>

use std::path::PathBuf;
use std::time::Instant;

use jp_import::plan::Action;

fn main() -> anyhow::Result<()> {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("用法：rescan_check <语料库副本目录>"))?;
    let db = root.join("corpus.db");
    anyhow::ensure!(db.is_file(), "找不到 {}", db.display());

    let mut corpus = jp_corpus::Corpus::open_writable(&db)?;
    let started = Instant::now();
    corpus.ensure_schema()?;
    let lineage = corpus.backfill_lineage()?;
    let conn = corpus.connection();
    let count = |sql: &str| -> anyhow::Result<i64> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };
    println!("迁移 + 回填 {:.2?}；补登记 {lineage} 首", started.elapsed());
    println!(
        "歌 {} 首（有音频 {}）；audio_files {} 行（present {}，带大小 {}，带指纹 {}）",
        count("SELECT COUNT(*) FROM songs")?,
        count("SELECT COUNT(*) FROM songs WHERE audio_path IS NOT NULL AND audio_path <> ''")?,
        count("SELECT COUNT(*) FROM audio_files")?,
        count("SELECT COUNT(*) FROM audio_files WHERE status='present'")?,
        count("SELECT COUNT(*) FROM audio_files WHERE size IS NOT NULL")?,
        count("SELECT COUNT(*) FROM audio_files WHERE fingerprint IS NOT NULL")?,
    );

    // 库里每首歌所在的文件夹，重扫两遍
    let mut folders: Vec<String> = conn
        .prepare("SELECT audio_path FROM songs WHERE audio_path IS NOT NULL AND audio_path <> ''")?
        .query_map([], |r| r.get::<_, String>(0))?
        .filter_map(Result::ok)
        .filter_map(|p| PathBuf::from(p).parent().map(|d| d.display().to_string()))
        .collect();
    folders.sort();
    folders.dedup();

    for round in 1..=3 {
        let index = jp_import::LibraryIndex::from_corpus(&corpus)?;
        let started = Instant::now();
        let (tracks, _) = jp_import::scan_targets_known(&folders, 1, Some(&index));
        let scanned = started.elapsed();
        let plan = jp_import::plan(&tracks, &index);
        let s = plan.summary();
        let unchanged = tracks.iter().filter(|t| t.unchanged).count();
        let versions = plan.items.iter().filter(|i| matches!(&i.action, Action::New { version_of: Some(_), .. })).count();
        println!(
            "第 {round} 次：{} 个文件，扫描 {scanned:.2?}；没变（不读标签）{unchanged}；已在库中 {}（文件变了 {}）；新 {}（其中同名其他版本 {versions}）；同一录音换了文件 {}；批内重复 {}；跳过 {}",
            s.total, s.already_imported, s.files_changed, s.new, s.possible_duplicates, s.duplicates_in_batch, s.skipped
        );
        for item in plan.items.iter().filter(|i| matches!(i.action, Action::PossibleDuplicate { .. } | Action::New { version_of: Some(_), .. })).take(8) {
            println!("    {:?} ← {}", item.action, item.track.path);
        }
        if round == 1 {
            // 只刷新文件信息（不导新歌）：让第二次能按大小、修改时间跳过
            let refresh = jp_import::ImportPlan { items: plan.items.into_iter().filter(|i| matches!(i.action, Action::AlreadyImported { .. })).collect() };
            let started = Instant::now();
            let report = jp_import::execute(corpus.connection_mut(), &refresh, None)?;
            println!("    记文件信息 {:.2?}，刷新 {} 首", started.elapsed(), report.files_refreshed);
        }
    }
    Ok(())
}
