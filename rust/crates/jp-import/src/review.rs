//! 扫描 → 给人复核的那份计划。
//!
//! 两个客户端（Tauri 的导入页、macOS 的导入视图）看到的是同一份东西：扫哪些路径、
//! 同一个文件选了两次怎么算、选中的不是音频怎么报、计划里每一条显示什么。
//! 原来写在 `jp-app/src/commands.rs` 里，和 Tauri 无关，挪到这里共用。
//!
//! **只读，什么都不写。** 写库是 [`crate::execute_cancellable`]。

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;

use crate::{Action, ArtistSource, LibraryIndex, PlanSummary, ScannedTrack};

/// 计划里的一条，给界面显示。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItemView {
    pub path: String,
    pub file_name: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_sec: Option<f64>,
    pub has_lyrics: bool,
    /// 读 tag 时出的问题。为空表示一切正常。
    pub warning: Option<String>,
    /// 歌手是从 tag、文件名还是文件夹名定出来的。从文件夹名来的只是猜，界面不让它不经复核就导
    pub artist_source: ArtistSource,
    pub action: Action,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub summary: PlanSummary,
    pub items: Vec<PlanItemView>,
    /// 分词器可不可用。不可用时导入仍能进行，只是不写 tokens——
    /// UI 要在按下「导入」之前就把这件事说清楚。
    pub tokenizer_ready: bool,
    /// 选中但不是音频、因此没进计划的文件名。
    ///
    /// 单曲导入时用户可能顺手选中了 .lrc 或封面图。默不作声地丢掉
    /// 会让人以为「选了 5 个却只导了 3 个」是 bug，所以原样报出来。
    #[serde(default)]
    pub ignored: Vec<String>,
}

/// 扫一批路径（目录或具体的文件都行）。
///
/// 同一个文件被选中两次（选了文件又选了它所在的目录）只算一次：
/// 按路径去重在这里做，不然计划里会出现两条一模一样的、还互相判成「本批重复」。
///
/// 返回（按路径排好的扫描结果，选中但不是音频的文件名）。
pub fn scan_targets(paths: &[String], max_depth: usize) -> (Vec<ScannedTrack>, Vec<String>) {
    let mut tracks: Vec<ScannedTrack> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut ignored: Vec<String> = Vec::new();

    for path in paths {
        let target = Path::new(path);
        let found = crate::scan_dir(target, max_depth);
        if found.is_empty() && target.is_file() {
            // 不是音频（.lrc、封面图、歌单文件……）。扫目录时这种文件本来就不计数，
            // 但用户明确选中它的时候要说一声。
            ignored.push(
                target
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.clone()),
            );
            continue;
        }
        for track in found {
            if seen.insert(track.path.clone()) {
                tracks.push(track);
            }
        }
    }
    tracks.sort_by(|a, b| a.path.cmp(&b.path));
    (tracks, ignored)
}

/// 扫描结果 → 给界面复核的那份计划。**只算，不记**：记成待导入的一批是调用方的事。
pub fn review(
    tracks: &[ScannedTrack],
    index: &LibraryIndex,
    tokenizer_ready: bool,
    ignored: Vec<String>,
) -> ScanResult {
    let plan = crate::plan(tracks, index);
    let items = plan
        .items
        .iter()
        .map(|item| PlanItemView {
            path: item.track.path.clone(),
            file_name: item.track.file_name.clone(),
            title: item.track.title().to_string(),
            artist: item.track.artist().to_string(),
            album: item.track.album().to_string(),
            duration_sec: item.track.duration_sec,
            has_lyrics: item.track.lyrics_pick().is_some(),
            warning: item.track.warning.clone(),
            artist_source: item.track.artist_source(),
            action: item.action.clone(),
        })
        .collect();
    ScanResult {
        summary: plan.summary(),
        items,
        tokenizer_ready,
        ignored,
    }
}
