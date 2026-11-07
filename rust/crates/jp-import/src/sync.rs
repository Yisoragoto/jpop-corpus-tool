//! 按来源对账：把一个音乐文件夹扫一遍，和库里属于它的歌逐首对上。
//!
//! 以前重扫只找新歌；文件删了、挪了，库里那首就一直挂着一个找不到的路径。
//! 现在一个文件夹扫完分成这几类（[`SyncPlan`]），**确认之后才写库**：
//!
//! | 类 | 怎么判定 | 确认后做什么 |
//! |---|---|---|
//! | 新文件 | 路径不在库里，也不像库里哪首 | 交给现有的导入复核（[`crate::review`]） |
//! | 文件变了 | 同路径，大小或修改时间变了 | 刷新来自文件的字段 |
//! | 回来了 | 已移出的歌，原路径又扫到了 | 清掉移出标记 |
//! | 挪了位置 | 不见了的歌，和某个新文件是同一个录音 | 只改路径（移出的顺便恢复），不新建 |
//! | 消失 | 属于这个文件夹、这次没扫到 | 打移出标记，可恢复 |
//!
//! 「同一个录音」沿用导入计划的判据（[`crate::plan`]）：同一个 ISRC、同一个 MusicBrainz 录音，
//! 或者曲名歌手相同、版本标记相同、时长相近。
//!
//! 纯函数，不碰数据库也不碰盘：扫描结果、索引、这个文件夹的歌都由调用方给。
//! 「文件夹整个不在」不归这里判——那时调用方根本不该调它（不然整个文件夹都会算成消失）。

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::plan::{Action, LibraryIndex, PlannedTrack, path_key, plan};
use crate::scan::ScannedTrack;

/// 库里属于这个文件夹的一首（含已移出的）。
#[derive(Debug, Clone, Default)]
pub struct SourceSong {
    pub id: String,
    pub audio_path: String,
    pub title: String,
    pub artist: String,
    /// 已经移出了（`deleted_at` 不空）
    pub removed: bool,
}

/// 库里的一首歌和它的路径。界面列出来给人看。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongAt {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub path: String,
}

/// 挪了位置的一首。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Moved {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub from: String,
    pub to: String,
    /// 凭什么认定是同一首（「同一个 ISRC」…）
    pub reason: String,
    /// 它原来已经移出了：改路径的同时恢复
    pub was_removed: bool,
}

/// 一个文件夹的对账结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPlan {
    /// 新文件，交给导入复核。疑似重复、同批重复、导不了的也在这里：它们要人看
    pub to_review: Vec<PlannedTrack>,
    pub changed: Vec<PlannedTrack>,
    pub returned: Vec<SongAt>,
    pub moved: Vec<Moved>,
    pub gone: Vec<SongAt>,
    /// 在库、没变的
    pub unchanged: usize,
    /// 没变、但库里还没记过它的大小和修改时间（老库补登记时只记了路径）。
    /// 不用人确认：对账时顺手补记，下次才判断得出「变了」。不发给界面
    #[serde(skip)]
    pub unrecorded: Vec<PlannedTrack>,
    /// 这个文件夹一首都没扫到，而库里有属于它的歌。多半是挂错了盘或者文件夹被清空：界面要格外提醒
    pub nothing_found: bool,
}

impl SyncPlan {
    /// 有没有确认后要直接写库的（新文件不算：那是导入复核的事）
    pub fn has_changes(&self) -> bool {
        !(self.changed.is_empty() && self.returned.is_empty() && self.moved.is_empty() && self.gone.is_empty())
    }
}

fn at(song: &SourceSong) -> SongAt {
    SongAt { song_id: song.id.clone(), title: song.title.clone(), artist: song.artist.clone(), path: song.audio_path.clone() }
}

/// 对账。`scanned` 是这个文件夹这次扫到的，`members` 是库里属于它的歌（含已移出的）。
pub fn sync_plan(scanned: &[ScannedTrack], index: &LibraryIndex, members: &[SourceSong]) -> SyncPlan {
    let planned = plan(scanned, index).items;
    let seen: HashSet<String> = scanned.iter().map(|t| path_key(&t.path)).collect();
    let by_id: HashMap<&str, &SourceSong> = members.iter().map(|m| (m.id.as_str(), m)).collect();
    // 属于这个文件夹、原路径这次没扫到的：在库的会算「消失」，除非认出它挪到了哪
    let missing: HashSet<&str> = members
        .iter()
        .filter(|m| !m.audio_path.is_empty() && !seen.contains(&path_key(&m.audio_path)))
        .map(|m| m.id.as_str())
        .collect();

    let mut out = SyncPlan {
        to_review: Vec::new(),
        changed: Vec::new(),
        returned: Vec::new(),
        moved: Vec::new(),
        gone: Vec::new(),
        unchanged: 0,
        unrecorded: Vec::new(),
        nothing_found: scanned.is_empty() && members.iter().any(|m| !m.removed),
    };
    let mut claimed: HashSet<&str> = HashSet::new();
    for item in planned {
        match &item.action {
            Action::AlreadyImported { song_id, file_changed } => {
                let member = by_id.get(song_id.as_str());
                if member.is_some_and(|m| m.removed) {
                    out.returned.push(at(member.unwrap()));
                } else if *file_changed {
                    out.changed.push(item);
                } else {
                    out.unchanged += 1;
                    // 读过标签的（`unchanged` 为假）就是库里没有它的基准
                    if !item.track.unchanged {
                        out.unrecorded.push(item);
                    }
                }
            }
            // 一首不见了的歌只认领一个新位置：两个新文件都像它时，第二个照常交给人看
            Action::PossibleDuplicate { song_id, reason, .. }
                if missing.contains(song_id.as_str()) && !claimed.contains(song_id.as_str()) =>
            {
                let member = by_id[song_id.as_str()];
                claimed.insert(member.id.as_str());
                out.moved.push(Moved {
                    song_id: member.id.clone(),
                    title: member.title.clone(),
                    artist: member.artist.clone(),
                    from: member.audio_path.clone(),
                    to: item.track.path.clone(),
                    reason: reason.clone(),
                    was_removed: member.removed,
                });
            }
            _ => out.to_review.push(item),
        }
    }
    for member in members {
        if !member.removed && missing.contains(member.id.as_str()) && !claimed.contains(member.id.as_str()) {
            out.gone.push(at(member));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::ExistingTrack;

    fn scanned(path: &str, title: &str, isrc: &str) -> ScannedTrack {
        ScannedTrack {
            path: path.into(),
            tag_title: title.into(),
            tag_artist: "YOASOBI".into(),
            isrc: isrc.into(),
            size: Some(100),
            mtime: Some(1.0),
            ..Default::default()
        }
    }
    fn existing(id: &str, path: &str, title: &str, isrc: &str) -> ExistingTrack {
        ExistingTrack { id: id.into(), audio_path: path.into(), title: title.into(), artist: "YOASOBI".into(), isrc: isrc.into(), ..Default::default() }
    }
    fn member(id: &str, path: &str, title: &str, removed: bool) -> SourceSong {
        SourceSong { id: id.into(), audio_path: path.into(), title: title.into(), artist: "YOASOBI".into(), removed }
    }

    #[test]
    fn each_file_lands_in_exactly_one_bucket() {
        let library = [
            existing("001", "D:/m/a.flac", "夜に駆ける", ""),
            existing("002", "D:/m/b.flac", "群青", ""),
            existing("003", "D:/m/c.flac", "ハルジオン", "JPXX01"),
            existing("004", "D:/m/d.flac", "怪物", ""),
            existing("005", "D:/m/e.flac", "アイドル", ""),
        ];
        let mut index = LibraryIndex::from_tracks(&library);
        index.set_stamp("D:/m/a.flac", Some(100), Some(1.0));
        index.set_stamp("D:/m/b.flac", Some(50), Some(1.0));
        index.set_stamp("D:/m/e.flac", Some(100), Some(1.0));
        let members = [
            member("001", "D:/m/a.flac", "夜に駆ける", false),
            member("002", "D:/m/b.flac", "群青", false),
            member("003", "D:/m/c.flac", "ハルジオン", false),
            member("004", "D:/m/d.flac", "怪物", false),
            member("005", "D:/m/e.flac", "アイドル", true),
        ];
        let files = [
            scanned("D:/m/a.flac", "夜に駆ける", ""),          // 没变
            scanned("D:/m/b.flac", "群青", ""),                // 大小变了
            scanned("D:/m/sub/c2.flac", "ハルジオン", "JPXX01"), // 挪了：同一个 ISRC
            scanned("D:/m/e.flac", "アイドル", ""),            // 移出过，又回来了
            scanned("D:/m/new.flac", "勇者", ""),              // 新歌
        ];
        let plan = sync_plan(&files, &index, &members);
        assert_eq!(plan.unchanged, 1);
        assert_eq!(plan.changed.iter().map(|c| c.track.path.as_str()).collect::<Vec<_>>(), ["D:/m/b.flac"]);
        assert_eq!(plan.moved.len(), 1);
        assert_eq!((plan.moved[0].song_id.as_str(), plan.moved[0].to.as_str()), ("003", "D:/m/sub/c2.flac"));
        assert_eq!(plan.moved[0].reason, "同一个 ISRC");
        assert_eq!(plan.returned.iter().map(|r| r.song_id.as_str()).collect::<Vec<_>>(), ["005"]);
        assert_eq!(plan.gone.iter().map(|g| g.song_id.as_str()).collect::<Vec<_>>(), ["004"]);
        assert_eq!(plan.to_review.len(), 1);
        assert!(matches!(plan.to_review[0].action, Action::New { .. }));
        assert!(!plan.nothing_found);
        assert!(plan.has_changes());
    }

    #[test]
    fn a_removed_song_found_elsewhere_is_moved_and_restored() {
        let index = LibraryIndex::from_tracks(&[existing("001", "D:/m/old.flac", "夜に駆ける", "JPXX01")]);
        let plan = sync_plan(&[scanned("D:/m/new.flac", "夜に駆ける", "JPXX01")], &index, &[member("001", "D:/m/old.flac", "夜に駆ける", true)]);
        assert_eq!(plan.moved.len(), 1);
        assert!(plan.moved[0].was_removed);
        assert!(plan.gone.is_empty() && plan.to_review.is_empty());
    }

    #[test]
    fn a_song_from_another_folder_is_not_claimed_as_moved() {
        // 别的文件夹里的歌（不在 members 里）在这里出现同一个录音：照常交给人看，不改它的路径
        let index = LibraryIndex::from_tracks(&[existing("001", "E:/other/x.flac", "夜に駆ける", "JPXX01")]);
        let plan = sync_plan(&[scanned("D:/m/x.flac", "夜に駆ける", "JPXX01")], &index, &[]);
        assert!(plan.moved.is_empty());
        assert!(matches!(plan.to_review[0].action, Action::PossibleDuplicate { .. }));
    }

    #[test]
    fn two_candidates_for_one_missing_song_claim_it_once() {
        let index = LibraryIndex::from_tracks(&[existing("001", "D:/m/old.flac", "夜に駆ける", "JPXX01")]);
        let files = [scanned("D:/m/a.flac", "夜に駆ける", "JPXX01"), scanned("D:/m/b.flac", "夜に駆ける", "JPXX01")];
        let plan = sync_plan(&files, &index, &[member("001", "D:/m/old.flac", "夜に駆ける", false)]);
        assert_eq!(plan.moved.len(), 1);
        assert_eq!(plan.to_review.len(), 1);
        assert!(plan.gone.is_empty());
    }

    #[test]
    fn an_empty_scan_of_a_folder_with_songs_is_flagged() {
        let index = LibraryIndex::from_tracks(&[existing("001", "D:/m/a.flac", "夜に駆ける", "")]);
        let plan = sync_plan(&[], &index, &[member("001", "D:/m/a.flac", "夜に駆ける", false)]);
        assert!(plan.nothing_found);
        assert_eq!(plan.gone.len(), 1);
        // 只剩移出过的歌：一首都没扫到很正常
        let plan = sync_plan(&[], &index, &[member("001", "D:/m/a.flac", "夜に駆ける", true)]);
        assert!(!plan.nothing_found && plan.gone.is_empty() && !plan.has_changes());
    }
}
