//! 音乐文件夹（「曲库 › 来源」）的应用层：加、停用、移除、按文件夹对账、恢复移出的歌。
//!
//! 表和归属在 `jp_corpus::library_sources`，对账的判定在 `jp_import::sync`；
//! 这里负责盘上的事（文件夹在不在、扫目录、读标签）和事务。
//!
//! **两条不能破的线：**
//! - 文件夹整个不在（盘没插、网络盘断了）时只记一条错误，**不对账**：
//!   否则它下面每一首都会被当成「消失」移出语料库。
//! - 对账结果先给人看，确认了才写（[`apply`]）。确认时传回来的是人看过、勾选过的那几项，
//!   不在写的时候重扫一遍——不然人看到的和真正写进去的可能不一样；但每一项落库前都重新核对。

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result, bail};
use jp_corpus::Corpus;
use jp_corpus::library_sources::{self as table, Source, SourceView};
use jp_import::sync::{SongAt, SourceSong, SyncPlan, sync_plan};
use serde::{Deserialize, Serialize};

use crate::admin::{Library, delete_song};

/// 递归的文件夹往下看几层。够深就行：再深多半是误把整个盘加进来了
pub const RECURSIVE_DEPTH: usize = 32;

/// 往下看几层：递归的看 [`RECURSIVE_DEPTH`] 层，不递归的只看直接放在里面的文件
pub fn depth_of(recursive: bool) -> usize {
    if recursive { RECURSIVE_DEPTH } else { 1 }
}

fn depth(source: &Source) -> usize {
    depth_of(source.recursive)
}

/// 来源表还是空的时，从库里的歌所在的位置推出来的建议（和以前「语料库文件夹」同一套推算）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub path: String,
    pub recursive: bool,
    /// 库里有几首在它下面
    pub songs: usize,
}

/// 建议加哪些文件夹，和全部加上之后还有几首不在任何文件夹里（手动导入的单曲）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestions {
    pub folders: Vec<Suggestion>,
    pub loose_songs: usize,
}

/// 推算的建议。来源表里已经有东西时返回空——那时候以用户亲手配的为准。
pub fn suggestions(corpus: &Corpus, library_root: &Path) -> Result<Suggestions> {
    let conn = corpus.connection();
    if !table::all(conn)?.is_empty() {
        return Ok(Suggestions { folders: Vec::new(), loose_songs: 0 });
    }
    let audio_paths: Vec<String> = conn
        .prepare("SELECT audio_path FROM live_songs WHERE audio_path IS NOT NULL AND audio_path <> ''")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let folders: Vec<Suggestion> = jp_import::watched_folders(library_root, &audio_paths, &[])
        .into_iter()
        .filter(|f| f.songs > 0)
        .map(|f| Suggestion {
            path: f.path.to_string_lossy().into_owned(),
            recursive: f.reason == jp_import::WatchReason::Parent || f.depth > 1,
            songs: f.songs,
        })
        .collect();
    let as_sources: Vec<Source> = folders
        .iter()
        .enumerate()
        .map(|(i, f)| Source {
            id: i as i64,
            root_path: f.path.clone(),
            label: String::new(),
            recursive: f.recursive,
            enabled: true,
            created_at: String::new(),
            last_scanned_at: None,
            last_scan_error: None,
        })
        .collect();
    let loose_songs = audio_paths.iter().filter(|p| table::owner(&as_sources, p).is_none()).count();
    Ok(Suggestions { folders, loose_songs })
}

/// 加一个或几个文件夹（确认建议时一次加好几个），然后按路径给歌填归属。一个事务。
pub fn add(corpus: &Corpus, folders: &[(String, bool)]) -> Result<Vec<SourceView>> {
    let tx = corpus.connection().unchecked_transaction()?;
    for (path, recursive) in folders {
        table::add(&tx, path, "", *recursive)?;
    }
    table::assign(&tx)?;
    tx.commit()?;
    table::list(corpus.connection())
}

pub fn list(corpus: &Corpus) -> Result<Vec<SourceView>> {
    table::list(corpus.connection())
}

pub fn set_enabled(corpus: &Corpus, id: i64, enabled: bool) -> Result<Vec<SourceView>> {
    table::set_enabled(corpus.connection(), id, enabled)?;
    table::list(corpus.connection())
}

/// 移除一个文件夹时它的歌怎么办。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoveMode {
    /// 移出语料库，可以在「已移出的歌」里恢复
    MoveOut,
    /// 彻底删除（和曲库维护里的删除一样）
    Delete,
}

/// 移除的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Removed {
    pub moved_out: usize,
    pub deleted: usize,
    pub sources: Vec<SourceView>,
}

/// 移除一个文件夹。歌先按 `mode` 处理，再删掉来源这一行。
pub fn remove(library: &mut Library<'_>, id: i64, mode: RemoveMode) -> Result<Removed> {
    let members = table::members(library.corpus.connection(), id)?;
    if table::get(library.corpus.connection(), id)?.is_none() {
        bail!("没有这个文件夹：{id}");
    }
    let (mut moved_out, mut deleted) = (0, 0);
    match mode {
        RemoveMode::MoveOut => {
            let tx = library.corpus.connection_mut().transaction()?;
            let ids: Vec<String> = members.iter().map(|m| m.id.clone()).collect();
            moved_out = table::mark_removed(&tx, &ids)?;
            table::forget(&tx, id)?;
            tx.commit()?;
        }
        RemoveMode::Delete => {
            // 一首一个事务（和曲库维护里删一首一样）：删到一半出错时，已经删掉的就是删掉了，
            // 来源这一行还留着，再点一次会接着删剩下的
            for member in &members {
                delete_song(library, &member.id).with_context(|| format!("删不掉 {}《{}》", member.id, member.title))?;
                deleted += 1;
            }
            table::forget(library.corpus.connection(), id)?;
        }
    }
    Ok(Removed { moved_out, deleted, sources: table::list(library.corpus.connection())? })
}

/// 一个文件夹的对账，带上是哪个文件夹（界面要显示）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePlan {
    pub source: Source,
    #[serde(flatten)]
    pub plan: SyncPlan,
    /// 音频没变、旁边的歌词文件新放进来或改过的歌（`path` 是那个歌词文件）。
    /// 确认后由客户端逐首同步歌词（`jp_lyrics::sources::sync_local`：要分词器，不在这一层）
    pub lyrics_changed: Vec<SongAt>,
}

/// 扫一个文件夹，和库里属于它的歌对账。**不改歌**：只记扫描时间、错误，
/// 以及还没记过的文件大小和修改时间（见 [`SyncPlan::unrecorded`]）。
pub fn plan(corpus: &Corpus, id: i64) -> Result<SourcePlan> {
    let conn = corpus.connection();
    let source = table::get(conn, id)?.with_context(|| format!("没有这个文件夹：{id}"))?;
    let root = Path::new(&source.root_path);
    if !root.is_dir() {
        let reason = format!("找不到文件夹 {}：盘没插上，或者文件夹被挪走了。没有对账，库里的歌都没动", source.root_path);
        table::record_scan(conn, id, Some(&reason))?;
        bail!(reason);
    }
    let index = jp_import::LibraryIndex::from_corpus(corpus)?;
    let scanned = jp_import::scan_dir_known(root, depth(&source), &|path, size, mtime| index.unchanged(path, size, mtime));
    let members: Vec<SourceSong> = table::members(conn, id)?
        .into_iter()
        .map(|m| SourceSong { id: m.id, audio_path: m.audio_path, title: m.title, artist: m.artist, removed: m.removed })
        .collect();
    let plan = sync_plan(&scanned, &index, &members);
    // 只是记账（文件的大小和修改时间），不改任何歌曲字段，所以不等确认
    let tx = conn.unchecked_transaction()?;
    for item in &plan.unrecorded {
        if let jp_import::Action::AlreadyImported { song_id, .. } = &item.action {
            jp_import::import::refresh_known(&tx, song_id, &item.track, false)?;
        }
    }
    // 歌词文件：音频变了、挪了、不见了的歌另有归处（音频变了确认时也会同步歌词），只看其余在库的
    let elsewhere: HashSet<&str> = plan
        .changed
        .iter()
        .filter_map(|c| match &c.action {
            jp_import::Action::AlreadyImported { song_id, .. } => Some(song_id.as_str()),
            _ => None,
        })
        .chain(plan.moved.iter().map(|m| m.song_id.as_str()))
        .chain(plan.gone.iter().map(|g| g.song_id.as_str()))
        .collect();
    let mut lyrics_changed = Vec::new();
    for m in members.iter().filter(|m| !m.removed && !elsewhere.contains(m.id.as_str())) {
        let audio = Path::new(&m.audio_path);
        if !audio.is_file() {
            continue;
        }
        if let Some(path) = jp_lyrics::sources::local_changed(&tx, &m.id, audio)? {
            lyrics_changed.push(SongAt {
                song_id: m.id.clone(),
                title: m.title.clone(),
                artist: m.artist.clone(),
                path: path.display().to_string(),
            });
        }
    }
    table::record_scan(&tx, id, None)?;
    tx.commit()?;
    let source = table::get(conn, id)?.context("文件夹在扫描时被移除了")?;
    Ok(SourcePlan { source, plan, lyrics_changed })
}

/// 确认之后要写的事。由界面按它显示的那份对账、减去用户勾掉的组成后传回来。
///
/// **不信任它**：[`apply`] 落库前每一项都重新核对（属于这个文件夹、文件现在确实在或不在），
/// 对不上的跳过不写。所以后端不用记住上一次的对账，两个客户端也能共用这条命令。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ApplyRequest {
    /// 文件变了、要刷新的那几个文件的路径
    pub refresh: Vec<String>,
    /// 回来了、要恢复的歌
    pub restore: Vec<String>,
    /// 挪了位置的歌：(歌, 新路径)
    pub relink: Vec<Relink>,
    /// 消失了、要移出的歌
    pub move_out: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relink {
    pub song_id: String,
    pub to: String,
}

impl ApplyRequest {
    /// 整份对账照单全收（测试、对账脚本用）
    pub fn everything(plan: &SyncPlan) -> Self {
        Self {
            refresh: plan.changed.iter().map(|c| c.track.path.clone()).collect(),
            restore: plan.returned.iter().map(|r| r.song_id.clone()).collect(),
            relink: plan.moved.iter().map(|m| Relink { song_id: m.song_id.clone(), to: m.to.clone() }).collect(),
            move_out: plan.gone.iter().map(|g| g.song_id.clone()).collect(),
        }
    }
}

/// 写了多少，和因为核对不上跳过了几项。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub refreshed: usize,
    pub restored: usize,
    pub relinked: usize,
    pub moved_out: usize,
    /// 从对账到确认之间情况变了（文件回来了、又不见了……）而没写的
    pub skipped: usize,
}

/// 把确认过的对账写进库。新文件不在这里：它们走导入复核。一个事务。
pub fn apply(corpus: &Corpus, id: i64, request: &ApplyRequest) -> Result<Applied> {
    let conn = corpus.connection();
    let source = table::get(conn, id)?.with_context(|| format!("没有这个文件夹：{id}"))?;
    if !Path::new(&source.root_path).is_dir() {
        bail!("找不到文件夹 {}：盘没插上，或者文件夹被挪走了。没有对账，库里的歌都没动", source.root_path);
    }
    let members: std::collections::HashMap<String, table::Member> =
        table::members(conn, id)?.into_iter().map(|m| (m.id.clone(), m)).collect();
    let inside = |path: &str| table::covers(&source.root_path, source.recursive, path) && Path::new(path).is_file();
    let index = jp_import::LibraryIndex::from_corpus(corpus)?;
    let mut done = Applied::default();
    let tx = conn.unchecked_transaction()?;

    for path in &request.refresh {
        let song = index.song_for_path(path).filter(|song| members.contains_key(*song));
        match song {
            Some(song) if inside(path) => {
                let track = jp_import::scan_file(Path::new(path));
                jp_import::import::refresh_known(&tx, song, &track, true)?;
                done.refreshed += 1;
            }
            _ => done.skipped += 1,
        }
    }
    for song in &request.restore {
        // 回来了：原路径上确实有文件
        match members.get(song) {
            Some(m) if m.removed && Path::new(&m.audio_path).is_file() => {
                done.restored += table::restore(&tx, std::slice::from_ref(song))?;
            }
            _ => done.skipped += 1,
        }
    }
    for Relink { song_id, to } in &request.relink {
        // 挪了：原路径上没有了，新路径在这个文件夹里、而且不是别的歌的文件
        let taken = index.song_for_path(to).is_some_and(|other| other != song_id);
        match members.get(song_id) {
            Some(m) if !Path::new(&m.audio_path).is_file() && inside(to) && !taken => {
                jp_import::maintain::relink_audio(&tx, song_id, to)
                    .with_context(|| format!("改不了 {}《{}》的路径", song_id, m.title))?;
                done.relinked += 1;
                if m.removed {
                    done.restored += table::restore(&tx, std::slice::from_ref(song_id))?;
                }
            }
            _ => done.skipped += 1,
        }
    }
    let moved: HashSet<&str> = request.relink.iter().map(|r| r.song_id.as_str()).collect();
    for song in &request.move_out {
        // 消失：在库、原路径上确实没有文件、没有同时被改路径
        match members.get(song) {
            Some(m) if !m.removed && !Path::new(&m.audio_path).is_file() && !moved.contains(song.as_str()) => {
                done.moved_out += table::mark_removed(&tx, std::slice::from_ref(song))?;
            }
            _ => done.skipped += 1,
        }
    }
    table::assign(&tx)?;
    tx.commit()?;
    Ok(done)
}

pub fn removed_songs(corpus: &Corpus) -> Result<Vec<table::RemovedSong>> {
    table::removed(corpus.connection())
}

/// 恢复移出的歌（「已移出的歌」里点恢复）。音频找不到的也照样恢复：它会出现在「找不到音频的歌」里
pub fn restore(corpus: &Corpus, ids: &[String]) -> Result<usize> {
    let tx = corpus.connection().unchecked_transaction()?;
    let n = table::restore(&tx, ids)?;
    tx.commit()?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("jp-library-sources-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn corpus(dir: &Path) -> Corpus {
        let corpus = Corpus::open_writable(dir.join("corpus.db")).unwrap();
        corpus.migrate().unwrap();
        corpus
    }
    fn song(corpus: &Corpus, id: &str, path: &Path) {
        corpus
            .connection()
            .execute(
                "INSERT INTO songs (id, title, artist, audio_path) VALUES (?1, ?1, 'a', ?2)",
                rusqlite::params![id, path.to_string_lossy()],
            )
            .unwrap();
    }

    #[test]
    fn a_missing_folder_is_reported_and_nothing_is_moved_out() {
        let scratch = Scratch::new("missing");
        let music = scratch.0.join("music");
        std::fs::create_dir_all(&music).unwrap();
        let corpus = corpus(&scratch.0);
        song(&corpus, "001", &music.join("a.flac"));
        let id = add(&corpus, &[(music.to_string_lossy().into_owned(), true)]).unwrap()[0].source.id;
        std::fs::remove_dir_all(&music).unwrap();
        let err = plan(&corpus, id).unwrap_err().to_string();
        assert!(err.contains("没有对账"), "{err}");
        let view = &list(&corpus).unwrap()[0];
        assert!(!view.present);
        assert!(view.source.last_scan_error.as_deref().unwrap().contains("找不到文件夹"));
        assert_eq!(view.songs, 1, "歌还在库里");
    }

    #[test]
    fn a_deleted_file_is_moved_out_and_can_be_restored() {
        let scratch = Scratch::new("gone");
        let music = scratch.0.join("music");
        std::fs::create_dir_all(&music).unwrap();
        // 一个真文件（扫得到但读不出标签：没关系，路径相同就是「已在库中」）和一个已经没了的
        std::fs::write(music.join("a.flac"), b"not really flac").unwrap();
        let corpus = corpus(&scratch.0);
        song(&corpus, "001", &music.join("a.flac"));
        song(&corpus, "002", &music.join("b.flac"));
        let id = add(&corpus, &[(music.to_string_lossy().into_owned(), true)]).unwrap()[0].source.id;

        let found = plan(&corpus, id).unwrap();
        assert_eq!(found.plan.gone.iter().map(|g| g.song_id.as_str()).collect::<Vec<_>>(), ["002"]);
        assert!(found.source.last_scanned_at.is_some());
        let done = apply(&corpus, id, &ApplyRequest::everything(&found.plan)).unwrap();
        assert_eq!(done.moved_out, 1);
        assert_eq!(removed_songs(&corpus).unwrap().len(), 1);
        let view = &list(&corpus).unwrap()[0];
        assert_eq!((view.songs, view.removed), (1, 1));

        // 文件放回来：再对一次账就是「回来了」
        std::fs::write(music.join("b.flac"), b"back").unwrap();
        let again = plan(&corpus, id).unwrap();
        assert_eq!(again.plan.returned.len(), 1);
        assert!(again.plan.gone.is_empty());
        apply(&corpus, id, &ApplyRequest::everything(&again.plan)).unwrap();
        assert!(removed_songs(&corpus).unwrap().is_empty());
    }

    #[test]
    fn suggestions_come_from_where_the_songs_are_and_stop_once_sources_exist() {
        let scratch = Scratch::new("suggest");
        let a = scratch.0.join("music").join("A");
        std::fs::create_dir_all(&a).unwrap();
        let corpus = corpus(&scratch.0);
        song(&corpus, "001", &a.join("x.flac"));
        song(&corpus, "002", &a.join("y.flac"));
        let library_root = scratch.0.join("library");
        let s = suggestions(&corpus, &library_root).unwrap();
        assert_eq!(s.folders.len(), 1);
        assert_eq!(s.folders[0].songs, 2);
        assert_eq!(s.loose_songs, 0);
        add(&corpus, &[(s.folders[0].path.clone(), s.folders[0].recursive)]).unwrap();
        assert!(suggestions(&corpus, &library_root).unwrap().folders.is_empty());
    }

    #[test]
    fn removing_a_folder_moves_its_songs_out() {
        let scratch = Scratch::new("remove");
        let music = scratch.0.join("music");
        std::fs::create_dir_all(&music).unwrap();
        let mut corpus = corpus(&scratch.0);
        song(&corpus, "001", &music.join("a.flac"));
        let id = add(&corpus, &[(music.to_string_lossy().into_owned(), true)]).unwrap()[0].source.id;
        let covers = scratch.0.join("covers");
        let pitch = scratch.0.join("pitch");
        let db_path = scratch.0.join("corpus.db");
        let mut library = Library { corpus: &mut corpus, db_path: &db_path, covers_dir: &covers, pitch_cache: &pitch };
        let removed = remove(&mut library, id, RemoveMode::MoveOut).unwrap();
        assert_eq!((removed.moved_out, removed.deleted, removed.sources.len()), (1, 0, 0));
        assert_eq!(removed_songs(&corpus).unwrap()[0].source, "", "来源已经移除了");
    }
}
