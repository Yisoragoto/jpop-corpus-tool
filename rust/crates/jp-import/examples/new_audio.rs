//! 拿一个真实的语料库目录跑一遍「启动时查新歌」，看它会去哪些文件夹、会报出什么。**只读。**
//!
//!     cargo run -p jp-import --example new_audio -- "C:\Users\me\AppData\Local\JPOP Corpus Tool"
//!
//! 看的范围是从库里的歌所在的文件夹推出来的（见 `jp_import::watch`），推得对不对只有
//! 拿真实的盘面才看得出来：改那几条规则之前、之后各跑一遍，对一对报出来的东西。

use std::path::PathBuf;

use jp_import::plan::Action;

fn main() -> anyhow::Result<()> {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("用法：new_audio <语料库目录>"))?;
    let db = root.join("corpus.db");
    anyhow::ensure!(db.is_file(), "找不到 {}", db.display());

    // 只读打开：这是用户真在用的库
    let corpus = jp_corpus::Corpus::open(&db)?;
    let index = jp_import::LibraryIndex::from_corpus(&corpus)?;
    let audio_paths: Vec<String> = corpus
        .connection()
        .prepare("SELECT audio_path FROM songs WHERE audio_path IS NOT NULL AND audio_path <> ''")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    let started = std::time::Instant::now();
    let folders = jp_import::watched_folders(&root, &audio_paths, &[]);
    let unknown = jp_import::unknown_audio(&folders, &index, &[], &[]);
    let listed = started.elapsed();
    let tracks: Vec<_> = unknown.iter().map(|file| jp_import::scan_file(&file.path)).collect();
    let plan = jp_import::plan(&tracks, &index);
    let summary = plan.summary();

    println!("库里 {} 首歌，看 {} 个文件夹：", audio_paths.len(), folders.len());
    for (at, folder) in folders.iter().enumerate() {
        let here = unknown.iter().filter(|file| file.folder == at).count();
        println!(
            "  {:?}  往下 {} 层  库里 {} 首  库外 {} 个  {}",
            folder.reason,
            folder.depth,
            folder.songs,
            here,
            folder.path.display()
        );
    }
    println!("列目录 {listed:?}，加上读 tag 共 {:?}", started.elapsed());
    println!(
        "库外的 {} 个文件：新歌 {}，疑似重复 {}，本批重复 {}，跳过 {}",
        summary.total, summary.new, summary.possible_duplicates, summary.duplicates_in_batch, summary.skipped
    );
    for item in &plan.items {
        let what = match &item.action {
            Action::New { .. } => "新歌".to_string(),
            Action::PossibleDuplicate { song_id, .. } => format!("疑似重复（库里的 {song_id}）"),
            Action::DuplicateInBatch { .. } => "本批重复".to_string(),
            Action::Skipped { reason } => format!("跳过：{reason}"),
            Action::AlreadyImported { song_id } => format!("已在库中 {song_id}（不该出现在这里）"),
        };
        println!(
            "  {what}  {} — {}（歌手来自 {:?}）  {}",
            item.track.title(),
            item.track.artist(),
            item.track.artist_source(),
            item.track.path
        );
    }
    Ok(())
}
