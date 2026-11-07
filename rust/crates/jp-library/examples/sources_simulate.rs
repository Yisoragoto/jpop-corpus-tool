//! 在库副本 + 一份音乐文件夹的复制品上模拟「删、改、挪、放回」，逐步打印对账和写库的结果。
//! **只对副本跑**，原文件夹不动：
//!
//! ```text
//! sources_simulate <库副本.db> <原文件夹> <复制品文件夹>
//! ```
//! 复制品要保留修改时间（`cp -p`），否则每个文件都会算「变了」。
use anyhow::{Result, ensure};
use jp_corpus::Corpus;
use jp_library::sources::{self, ApplyRequest};

fn counts(corpus: &Corpus) -> Result<(i64, i64, i64)> {
    let c = corpus.connection();
    let one = |sql: &str| -> Result<i64> { Ok(c.query_row(sql, [], |r| r.get(0))?) };
    Ok((one("SELECT COUNT(*) FROM live_songs")?, one("SELECT COUNT(*) FROM utterances")?, one("SELECT COUNT(*) FROM tokens")?))
}

fn show(step: &str, p: &jp_library::sources::SourcePlan) {
    println!(
        "{step}: 没变 {}  变了 {}  复核 {}  回来 {}  挪了 {}  消失 {}",
        p.plan.unchanged, p.plan.changed.len(), p.plan.to_review.len(), p.plan.returned.len(), p.plan.moved.len(), p.plan.gone.len()
    );
    for m in &p.plan.moved { println!("    挪了 {}《{}》→ {}（{}）", m.song_id, m.title, m.to, m.reason); }
    for g in &p.plan.gone { println!("    消失 {}《{}》", g.song_id, g.title); }
    for c in &p.plan.changed { println!("    变了 {}", c.track.path); }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (db, original, copy) = (&args[0], &args[1], &args[2]);
    let corpus = Corpus::open_writable(db)?;
    corpus.migrate()?;
    // 让库里指向原文件夹的歌改指复制品（songs 和 audio_files 两处）
    let sep = std::path::MAIN_SEPARATOR;
    let (from, to) = (format!("{original}{sep}"), format!("{copy}{sep}"));
    let n = corpus.connection().execute("UPDATE songs SET audio_path = ?2 || substr(audio_path, length(?1) + 1) WHERE audio_path LIKE ?1 || '%'", [&from, &to])?;
    corpus.connection().execute("UPDATE audio_files SET path = ?2 || substr(path, length(?1) + 1) WHERE path LIKE ?1 || '%'", [&from, &to])?;
    println!("改指复制品的歌 {n} 首");
    let before = counts(&corpus)?;
    let id = sources::add(&corpus, &[(copy.clone(), false)])?.into_iter().find(|v| v.source.root_path == *copy).unwrap().source.id;

    let p = sources::plan(&corpus, id)?;
    show("原样", &p);
    ensure!(p.plan.unchanged == n && !p.plan.has_changes(), "原样不该有任何变化");

    let mut files: Vec<_> = std::fs::read_dir(copy)?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file()).collect();
    files.sort();
    let (deleted, edited, moved) = (files[0].clone(), files[1].clone(), files[2].clone());
    let parked = std::env::temp_dir().join("jp-simulate-parked.mp3");
    std::fs::rename(&deleted, &parked)?;
    { use std::io::Write; std::fs::OpenOptions::new().append(true).open(&edited)?.write_all(&[0u8; 16])?; }
    let renamed = moved.with_file_name("改了名字.mp3");
    std::fs::rename(&moved, &renamed)?;

    let p = sources::plan(&corpus, id)?;
    show("删、改、挪之后", &p);
    ensure!(p.plan.gone.len() == 1 && p.plan.changed.len() == 1 && p.plan.moved.len() == 1, "应当正好一删一改一挪");
    let done = sources::apply(&corpus, id, &ApplyRequest::everything(&p.plan))?;
    println!("写库：{done:?}");
    let path: String = corpus.connection().query_row("SELECT audio_path FROM songs WHERE id = ?1", [&p.plan.moved[0].song_id], |r| r.get(0))?;
    ensure!(path == renamed.to_string_lossy(), "挪了的那首路径没改：{path}");
    let (live, utt, tok) = counts(&corpus)?;
    ensure!(live == before.0 - 1 && utt == before.1 && tok == before.2, "在库少一首、行和词不变：{:?} → {:?}", before, (live, utt, tok));
    let p = sources::plan(&corpus, id)?;
    show("写库之后再对一次", &p);
    ensure!(!p.plan.has_changes(), "写完再对账应当没有变化");

    std::fs::rename(&parked, &deleted)?;
    let p = sources::plan(&corpus, id)?;
    show("放回删掉的那首", &p);
    ensure!(p.plan.returned.len() == 1, "应当认出它回来了");
    sources::apply(&corpus, id, &ApplyRequest::everything(&p.plan))?;
    ensure!(counts(&corpus)? == before, "全部复原后数字和最初一样");
    println!("在库/行/词  最初 {before:?}  最后 {:?}  一致", counts(&corpus)?);
    Ok(())
}
