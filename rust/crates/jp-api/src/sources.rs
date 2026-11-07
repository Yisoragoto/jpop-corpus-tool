//! 曲库 › 来源：音乐文件夹、按文件夹对账、移出与恢复。
//!
//! 业务都在 `jp_library::sources`；这里只把参数、返回值整理成两个客户端看的形状。
//! 对账（[`LibrarySourcePlan`]）和确认（[`LibrarySourceApply`]）是两条命令，中间不留状态：
//! 确认时界面把它要做的那几项传回来，后端逐项重新核对后才写。

use anyhow::Result;
use jp_corpus::Corpus;
use jp_corpus::library_sources::{RemovedSong, Source, SourceView};
use jp_import::{Action, PlannedTrack};
use jp_library::sources;
use serde::{Deserialize, Serialize};

use crate::{Command, NoArgs};

/// 所有音乐文件夹，带歌曲数、移出数、盘在不在。
pub struct LibrarySources;

impl Command for LibrarySources {
    const TAURI: &'static str = "library_sources";
    const FFI: &'static str = "librarySources";
    type Args = NoArgs;
    type Output = Vec<SourceView>;
    fn run(corpus: &Corpus, _: NoArgs) -> Result<Vec<SourceView>> {
        sources::list(corpus)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
pub struct FolderArg {
    pub path: String,
    /// 包括子文件夹。不传当作是
    pub recursive: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
pub struct AddArgs {
    pub folders: Vec<FolderArg>,
}

/// 加一个或几个文件夹，再按路径给库里的歌填上归属。返回加完之后的列表。
pub struct LibrarySourcesAdd;

impl Command for LibrarySourcesAdd {
    const TAURI: &'static str = "library_sources_add";
    const FFI: &'static str = "librarySourcesAdd";
    type Args = AddArgs;
    type Output = Vec<SourceView>;
    fn run(corpus: &Corpus, args: AddArgs) -> Result<Vec<SourceView>> {
        let folders: Vec<(String, bool)> =
            args.folders.into_iter().map(|f| (f.path, f.recursive.unwrap_or(true))).collect();
        sources::add(corpus, &folders)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
pub struct SetEnabledArgs {
    pub id: i64,
    pub enabled: bool,
}

/// 停用 / 启用一个文件夹。停用的不参与启动检查，歌照常在库里。
pub struct LibrarySourceSetEnabled;

impl Command for LibrarySourceSetEnabled {
    const TAURI: &'static str = "library_source_set_enabled";
    const FFI: &'static str = "librarySourceSetEnabled";
    type Args = SetEnabledArgs;
    type Output = Vec<SourceView>;
    fn run(corpus: &Corpus, args: SetEnabledArgs) -> Result<Vec<SourceView>> {
        sources::set_enabled(corpus, args.id, args.enabled)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
pub struct SourceIdArgs {
    pub id: i64,
}

/// 一首歌和它的文件。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SongFile {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub path: String,
}

/// 挪了位置的歌。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MovedSong {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub from: String,
    pub to: String,
    /// 凭什么认定是同一首（ISRC、录音 ID、曲名歌手时长……）
    pub reason: String,
    /// 原来已经移出了，改路径的同时恢复
    pub was_removed: bool,
}

/// 一个文件夹的对账。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SourcePlanView {
    pub source: Source,
    /// 库里没有的文件（新歌、疑似重复、导入时会跳过的都在这里）。
    /// 不在这里导入：界面拿这些路径去走导入复核（`scan_files`）
    pub new_files: Vec<String>,
    /// 其中确定是新歌的几首
    pub new_songs: usize,
    /// 文件变了（换了压制、改了标签）：刷新文件信息
    pub changed: Vec<SongFile>,
    /// 移出过、文件又回来了
    pub returned: Vec<SongFile>,
    pub moved: Vec<MovedSong>,
    /// 文件不见了
    pub gone: Vec<SongFile>,
    pub unchanged: usize,
    /// 文件夹里一个音频都没扫到（可能是盘换了内容）
    pub nothing_found: bool,
}

fn song_file(song: &jp_import::sync::SongAt) -> SongFile {
    SongFile { song_id: song.song_id.clone(), title: song.title.clone(), artist: song.artist.clone(), path: song.path.clone() }
}

fn changed_file(item: &PlannedTrack) -> SongFile {
    let song_id = match &item.action {
        Action::AlreadyImported { song_id, .. } => song_id.clone(),
        _ => String::new(),
    };
    let track = &item.track;
    let pick = |a: &str, b: &str| if a.trim().is_empty() { b.to_string() } else { a.to_string() };
    SongFile {
        song_id,
        title: pick(&track.tag_title, &track.guess_title),
        artist: pick(&track.tag_artist, &track.guess_artist),
        path: track.path.clone(),
    }
}

/// 扫一个文件夹、和库里属于它的歌对账。**不改歌**，只记扫描时间和文件的大小、修改时间。
/// 文件夹找不到时报错（并记在文件夹上），不对账。
pub struct LibrarySourcePlan;

impl Command for LibrarySourcePlan {
    const TAURI: &'static str = "library_source_plan";
    const FFI: &'static str = "librarySourcePlan";
    type Args = SourceIdArgs;
    type Output = SourcePlanView;
    fn run(corpus: &Corpus, args: SourceIdArgs) -> Result<SourcePlanView> {
        let found = sources::plan(corpus, args.id)?;
        let plan = &found.plan;
        Ok(SourcePlanView {
            source: found.source.clone(),
            new_files: plan.to_review.iter().map(|t| t.track.path.clone()).collect(),
            new_songs: plan.to_review.iter().filter(|t| matches!(t.action, Action::New { .. })).count(),
            changed: plan.changed.iter().map(changed_file).collect(),
            returned: plan.returned.iter().map(song_file).collect(),
            moved: plan
                .moved
                .iter()
                .map(|m| MovedSong {
                    song_id: m.song_id.clone(),
                    title: m.title.clone(),
                    artist: m.artist.clone(),
                    from: m.from.clone(),
                    to: m.to.clone(),
                    reason: m.reason.clone(),
                    was_removed: m.was_removed,
                })
                .collect(),
            gone: plan.gone.iter().map(song_file).collect(),
            unchanged: plan.unchanged,
            nothing_found: plan.nothing_found,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
pub struct RelinkArg {
    pub song_id: String,
    pub to: String,
}

/// 确认对账：界面显示的那几组、去掉用户勾掉的。每一项都会重新核对，对不上的跳过。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
pub struct ApplyArgs {
    pub id: i64,
    /// 要刷新的文件路径（`changed` 里的 `path`）
    pub refresh: Option<Vec<String>>,
    /// 要恢复的歌（`returned`）
    pub restore: Option<Vec<String>>,
    /// 要改路径的歌（`moved`）
    pub relink: Option<Vec<RelinkArg>>,
    /// 要移出的歌（`gone`）
    pub move_out: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AppliedView {
    pub refreshed: usize,
    pub restored: usize,
    pub relinked: usize,
    pub moved_out: usize,
    /// 对账到确认之间情况变了、没有写的几项
    pub skipped: usize,
}

pub struct LibrarySourceApply;

impl Command for LibrarySourceApply {
    const TAURI: &'static str = "library_source_apply";
    const FFI: &'static str = "librarySourceApply";
    type Args = ApplyArgs;
    type Output = AppliedView;
    fn run(corpus: &Corpus, args: ApplyArgs) -> Result<AppliedView> {
        let request = sources::ApplyRequest {
            refresh: args.refresh.unwrap_or_default(),
            restore: args.restore.unwrap_or_default(),
            relink: args
                .relink
                .unwrap_or_default()
                .into_iter()
                .map(|r| sources::Relink { song_id: r.song_id, to: r.to })
                .collect(),
            move_out: args.move_out.unwrap_or_default(),
        };
        let done = sources::apply(corpus, args.id, &request)?;
        Ok(AppliedView {
            refreshed: done.refreshed,
            restored: done.restored,
            relinked: done.relinked,
            moved_out: done.moved_out,
            skipped: done.skipped,
        })
    }
}

/// 移出语料库的歌（可以恢复）。
pub struct LibraryRemovedSongs;

impl Command for LibraryRemovedSongs {
    const TAURI: &'static str = "library_removed_songs";
    const FFI: &'static str = "libraryRemovedSongs";
    type Args = NoArgs;
    type Output = Vec<RemovedSong>;
    fn run(corpus: &Corpus, _: NoArgs) -> Result<Vec<RemovedSong>> {
        sources::removed_songs(corpus)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields))]
pub struct SongIdsArgs {
    pub ids: Vec<String>,
}

/// 恢复移出的歌。返回恢复了几首
pub struct LibraryRestoreSongs;

impl Command for LibraryRestoreSongs {
    const TAURI: &'static str = "library_restore_songs";
    const FFI: &'static str = "libraryRestoreSongs";
    type Args = SongIdsArgs;
    type Output = usize;
    fn run(corpus: &Corpus, args: SongIdsArgs) -> Result<usize> {
        sources::restore(corpus, &args.ids)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::ffi::dispatch;
    use jp_corpus::Corpus;

    fn call(corpus: &Corpus, method: &str, args: Value) -> Value {
        dispatch(corpus, method, &args).expect("登记了").expect("调用成功")
    }

    #[test]
    fn a_folder_can_be_added_scanned_and_a_stale_request_is_skipped() {
        let dir = std::env::temp_dir().join(format!("jp-api-sources-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = jp_corpus::fixture::write_to(&dir.join("db")).unwrap();
        let music = dir.join("music");
        std::fs::create_dir_all(&music).unwrap();
        let corpus = Corpus::open_writable(&db).unwrap();

        assert_eq!(call(&corpus, "librarySources", Value::Null), json!([]));
        let path = music.to_string_lossy().to_string();
        let added = call(&corpus, "librarySourcesAdd", json!({ "folders": [{ "path": path }] }));
        assert_eq!(added[0]["recursive"], true, "不传 recursive 当作含子文件夹");
        assert_eq!(added[0]["songs"], 0);
        let id = added[0]["id"].clone();

        let plan = call(&corpus, "librarySourcePlan", json!({ "id": id }));
        // 库里没有歌属于它，空文件夹不算「一首都没扫到」
        assert_eq!(plan["nothingFound"], false);
        assert_eq!(plan["newFiles"], json!([]));
        assert!(plan["source"]["lastScannedAt"].is_string());

        // 不属于这个文件夹的歌：核对不上，跳过，不移出
        let done = call(&corpus, "librarySourceApply", json!({ "id": id, "moveOut": ["001"], "restore": ["002"] }));
        assert_eq!(done, json!({ "refreshed": 0, "restored": 0, "relinked": 0, "movedOut": 0, "skipped": 2 }));
        assert_eq!(call(&corpus, "libraryRemovedSongs", Value::Null), json!([]));

        let off = call(&corpus, "librarySourceSetEnabled", json!({ "id": id, "enabled": false }));
        assert_eq!(off[0]["enabled"], false);

        drop(corpus);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
