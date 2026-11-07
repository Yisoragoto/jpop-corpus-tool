//! 在库副本上试一遍音乐文件夹：升级 → 按推算的建议加文件夹 → 每个文件夹对账（不写对账结果）。
//! 打印歌曲、行、词、校正数在前后是否一致。**只对副本跑**：
//!
//! ```text
//! sources_dry_run <库副本.db> <语料库目录>
//! ```
use anyhow::Result;
use jp_corpus::Corpus;
use jp_library::sources;

fn counts(corpus: &Corpus) -> Result<[i64; 5]> {
    let c = corpus.connection();
    let one = |sql: &str| -> Result<i64> { Ok(c.query_row(sql, [], |r| r.get(0))?) };
    Ok([
        one("SELECT COUNT(*) FROM songs")?,
        one("SELECT COUNT(*) FROM live_songs")?,
        one("SELECT COUNT(*) FROM utterances")?,
        one("SELECT COUNT(*) FROM tokens")?,
        one("SELECT COUNT(*) FROM token_corrections")?,
    ])
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let corpus = Corpus::open_writable(&args[0])?;
    let applied = corpus.migrate()?;
    println!("迁移：{}", applied.summary());
    let before = counts(&corpus)?;
    let t = std::time::Instant::now();
    let suggested = sources::suggestions(&corpus, std::path::Path::new(&args[1]))?;
    println!("建议 {} 个文件夹（{:.0} ms），全加上后不在任何文件夹里的 {} 首：", suggested.folders.len(), t.elapsed().as_secs_f64() * 1e3, suggested.loose_songs);
    for f in &suggested.folders {
        println!("  {}  递归={}  {} 首", f.path, f.recursive, f.songs);
    }
    let root = std::path::Path::new(&args[1]);
    let paths = |corpus: &Corpus| -> Result<Vec<String>> {
        let found = jp_library::discovery::new_audio(corpus, root, &[], &[], false)?;
        let mut v: Vec<String> = found.scan.items.iter().map(|i| i.path.clone()).collect();
        v.sort();
        Ok(v)
    };
    let inferred = paths(&corpus)?;
    let folders: Vec<(String, bool)> = suggested.folders.iter().map(|f| (f.path.clone(), f.recursive)).collect();
    let views = sources::add(&corpus, &folders)?;
    let assigned: i64 = corpus.connection().query_row("SELECT COUNT(*) FROM songs WHERE source_id IS NOT NULL", [], |r| r.get(0))?;
    println!("加好之后 {} 个来源，{} 首有了所属文件夹", views.len(), assigned);
    for view in &views {
        let t = std::time::Instant::now();
        match sources::plan(&corpus, view.source.id) {
            Ok(p) => println!(
                "  [{}] {}  在库 {}  → 没变 {}  变了 {}  新文件等复核 {}  回来 {}  挪了 {}  消失 {}{}（{:.0} ms）",
                view.source.id, view.source.root_path, view.songs, p.plan.unchanged, p.plan.changed.len(), p.plan.to_review.len(),
                p.plan.returned.len(), p.plan.moved.len(), p.plan.gone.len(),
                if p.plan.nothing_found { "  ⚠一首都没扫到" } else { "" }, t.elapsed().as_secs_f64() * 1e3
            ),
            Err(err) => println!("  [{}] {}  对账失败：{err:#}", view.source.id, view.source.root_path),
        }
        if let Ok(p) = sources::plan(&corpus, view.source.id) {
            for g in p.plan.gone.iter().take(5) { println!("      消失 {} 《{}》 {}", g.song_id, g.title, g.path); }
            for m in p.plan.moved.iter().take(5) { println!("      挪了 {} 《{}》 {} → {}（{}）", m.song_id, m.title, m.from, m.to, m.reason); }
            for r in p.plan.to_review.iter().take(5) { println!("      复核 {} {:?}", r.track.path, std::mem::discriminant(&r.action)); }
        }
    }
    let from_sources = paths(&corpus)?;
    println!("启动检查报出的文件：推算 {} 个，来源 {} 个，{}", inferred.len(), from_sources.len(), if inferred == from_sources { "完全相同" } else { "不同！" });
    for p in from_sources.iter().filter(|p| !inferred.contains(p)) { println!("    只有来源报：{p}"); }
    for p in inferred.iter().filter(|p| !from_sources.contains(p)) { println!("    只有推算报：{p}"); }
    let after = counts(&corpus)?;
    println!("歌曲/在库/行/词/校正  前 {before:?}  后 {after:?}  {}", if before == after { "一致" } else { "不一致！" });
    Ok(())
}
