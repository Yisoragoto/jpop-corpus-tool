//! 拿真实的 Anki collection 验一遍学习状态读取。
//!
//!     cargo run -p jp-anki --example probe_learning
//!
//! **只读**：`learning::load` 会先把 collection 连同 -wal/-shm 复制到临时目录，
//! 再从副本读；原文件全程不打开写。

fn main() -> anyhow::Result<()> {
    match jp_anki::learning::find_collection() {
        Some(path) => println!("找到 collection: {}", path.display()),
        None => {
            println!("没找到 collection（没装 Anki 或装在别处）——这不是错误。");
            return Ok(());
        }
    }

    let t0 = std::time::Instant::now();
    let state = jp_anki::learning::load(None, "")?;
    let elapsed = t0.elapsed();

    println!("读取耗时 {elapsed:.2?}");
    println!("路径      {}", state.collection_path);
    println!("已加入    {} 词", state.words.len());
    println!("已复习    {} 词", state.studied_count());

    let mut sample: Vec<_> = state.words.iter().collect();
    sample.sort_by_key(|(word, _)| word.as_str());
    println!("\n抽样：");
    for (word, status) in sample.iter().take(12) {
        println!(
            "  {word:<12} 笔记={} 卡={} 最多复习={:<4} 已学={} 牌组={}",
            status.note_count, status.card_count, status.max_reps, status.studied,
            status.decks.join(" / ")
        );
    }
    Ok(())
}
