//! 扫描结果 → 一份「打算做什么」的计划。
//!
//! **计划和执行分开**，理由有三条：
//!
//! - 计划是纯函数，不碰数据库写，能离线测试；
//! - 用户可以先看计划再决定导不导——导入必须可复核，不能「点一下就
//!   悄悄改了 209 条记录」；
//! - 同一批文件重跑扫描会产出同样的计划，所以重新导入是幂等的。
//!
//! 这里**不做识别**（查 iTunes / MusicBrainz 补 metadata 是 scraper 的事），
//! 只回答「这个文件库里有没有」。

use std::collections::HashMap;

use serde::Serialize;

use crate::filename::matching_key;
use crate::scan::ScannedTrack;

/// 打算对一个扫描条目做什么。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    /// 库里没有，新建。`songId` 是预分配好的，执行时直接用。
    // 结构体变体的字段名要单独改：枚举上的 rename_all 只管变体名。
    #[serde(rename_all = "camelCase")]
    New { song_id: String },
    /// 这个文件路径已经在库里。重跑扫描时绝大多数条目都落这里。
    #[serde(rename_all = "camelCase")]
    AlreadyImported { song_id: String },
    /// 曲名 + 歌手对得上，但指向另一个文件。
    ///
    /// 可能是换了音源（FLAC 换成 MP3），也可能真有两个版本
    /// （原版 / Live）。**不自动决定**——猜错的代价是把用户的库弄脏，
    /// 交给人看。
    #[serde(rename_all = "camelCase")]
    PossibleDuplicate {
        song_id: String,
        existing_path: String,
    },
    /// 同一批扫描里出现了重复，只导第一个。
    #[serde(rename_all = "camelCase")]
    DuplicateInBatch { first_path: String },
    /// 导不了。
    Skipped { reason: String },
}

impl Action {
    /// 会不会真的往库里写。
    pub fn writes(&self) -> bool {
        matches!(self, Action::New { .. })
    }
}

/// 一个扫描条目 + 对它的处置。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedTrack {
    pub track: ScannedTrack,
    pub action: Action,
}

/// 库里已有的一条。计划阶段只需要这几个字段——
/// 刻意不用 `jp_corpus::Track`，这样计划层能脱离数据库测试。
#[derive(Debug, Clone)]
pub struct ExistingTrack {
    pub id: String,
    pub audio_path: String,
    pub title: String,
    pub artist: String,
}

/// 现有曲库的索引：路径 → id，(歌手, 曲名) → id。
pub struct LibraryIndex {
    by_path: HashMap<String, String>,
    by_identity: HashMap<(String, String), (String, String)>,
    /// 下一个可用的数字 id
    next_num: u32,
    /// id 零填充宽度。库里是 `001` 这种三位，超过 999 自然变四位。
    width: usize,
}

impl LibraryIndex {
    pub fn from_tracks(tracks: &[ExistingTrack]) -> Self {
        let mut by_path = HashMap::new();
        let mut by_identity = HashMap::new();
        let mut max_num = 0u32;
        let mut width = 3usize;

        for track in tracks {
            by_path.insert(path_key(&track.audio_path), track.id.clone());
            if let Some(key) = identity_key(&track.artist, &track.title) {
                // 先到先得：库里若已有同名同歌手的两条，报第一条就够了
                by_identity
                    .entry(key)
                    .or_insert_with(|| (track.id.clone(), track.audio_path.clone()));
            }
            // 非数字 id（比如手工加的）不参与序号分配，但也不能撞上
            if let Ok(num) = track.id.trim().parse::<u32>() {
                max_num = max_num.max(num);
                width = width.max(track.id.trim().len());
            }
        }
        Self {
            by_path,
            by_identity,
            next_num: max_num + 1,
            width,
        }
    }

    /// 空库。第一首会是 `001`。
    pub fn empty() -> Self {
        Self::from_tracks(&[])
    }

    /// 从数据库读一份索引。
    pub fn from_corpus(corpus: &jp_corpus::Corpus) -> anyhow::Result<Self> {
        let mut stmt = corpus
            .connection()
            .prepare("SELECT id, audio_path, title, artist FROM songs")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ExistingTrack {
                    id: row.get(0)?,
                    audio_path: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    title: row.get(2)?,
                    artist: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::from_tracks(&rows))
    }

    fn allocate(&self, num: u32) -> String {
        format!("{:0width$}", num, width = self.width)
    }

    /// 这个音频路径已经在库里了吗（迁移去重用，判据和导入的那条一样）
    pub fn song_for_path(&self, audio_path: &str) -> Option<&str> {
        self.by_path.get(&path_key(audio_path)).map(String::as_str)
    }

    /// 同歌手同曲名的已经在库里了吗
    pub fn song_for_identity(&self, artist: &str, title: &str) -> Option<&str> {
        identity_key(artist, title)
            .and_then(|key| self.by_identity.get(&key))
            .map(|(id, _)| id.as_str())
    }

    /// 取下一个 id 并占住它。**连着分很多个时用这个**——
    /// `plan()` 自己在批内累加，迁移那边是一首一首要，不能每次都从库里重算。
    pub fn take_next_id(&mut self) -> String {
        let id = self.allocate(self.next_num);
        self.next_num += 1;
        id
    }

    /// 把一首歌记进索引。迁移时边分边记，免得源库里本来就有的重复歌被搬两份。
    pub fn remember(&mut self, id: &str, audio_path: &str, artist: &str, title: &str) {
        self.by_path.insert(path_key(audio_path), id.to_string());
        if let Some(key) = identity_key(artist, title) {
            self.by_identity
                .entry(key)
                .or_insert_with(|| (id.to_string(), audio_path.to_string()));
        }
    }
}

/// 一份计划。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlan {
    pub items: Vec<PlannedTrack>,
}

/// 计划里各类处置的条数。给 UI 一眼看清「这次会动多少东西」。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub total: usize,
    pub new: usize,
    pub already_imported: usize,
    pub possible_duplicates: usize,
    pub duplicates_in_batch: usize,
    pub skipped: usize,
}

impl ImportPlan {
    pub fn summary(&self) -> PlanSummary {
        let mut s = PlanSummary {
            total: self.items.len(),
            ..Default::default()
        };
        for item in &self.items {
            match item.action {
                Action::New { .. } => s.new += 1,
                Action::AlreadyImported { .. } => s.already_imported += 1,
                Action::PossibleDuplicate { .. } => s.possible_duplicates += 1,
                Action::DuplicateInBatch { .. } => s.duplicates_in_batch += 1,
                Action::Skipped { .. } => s.skipped += 1,
            }
        }
        s
    }

    /// 真正会写库的条目。
    pub fn to_import(&self) -> impl Iterator<Item = &PlannedTrack> {
        self.items.iter().filter(|i| i.action.writes())
    }
}

/// 比对扫描结果和现有曲库。
pub fn plan(tracks: &[ScannedTrack], index: &LibraryIndex) -> ImportPlan {
    let mut items = Vec::with_capacity(tracks.len());
    let mut next_num = index.next_num;
    // 同一批里已经见过的，防止一次扫描把同一首导两遍
    let mut batch_paths: HashMap<String, String> = HashMap::new();
    let mut batch_identities: HashMap<(String, String), String> = HashMap::new();

    for track in tracks {
        let action = if !track.has_identity() {
            Action::Skipped {
                reason: "没有曲名，tag 和文件名都定不出".into(),
            }
        } else {
            let pkey = path_key(&track.path);
            // 路径相同是最硬的证据，优先于曲名歌手
            if let Some(id) = index.by_path.get(&pkey) {
                Action::AlreadyImported {
                    song_id: id.clone(),
                }
            } else if let Some(first) = batch_paths.get(&pkey) {
                Action::DuplicateInBatch {
                    first_path: first.clone(),
                }
            } else {
                let ikey = identity_key(track.artist(), track.title());
                match ikey.as_ref().and_then(|k| index.by_identity.get(k)) {
                    Some((id, path)) => Action::PossibleDuplicate {
                        song_id: id.clone(),
                        existing_path: path.clone(),
                    },
                    None => match ikey.as_ref().and_then(|k| batch_identities.get(k)) {
                        Some(first) => Action::DuplicateInBatch {
                            first_path: first.clone(),
                        },
                        None => {
                            let id = index.allocate(next_num);
                            next_num += 1;
                            batch_paths.insert(pkey, track.path.clone());
                            if let Some(k) = ikey {
                                batch_identities.insert(k, track.path.clone());
                            }
                            Action::New { song_id: id }
                        }
                    },
                }
            }
        };
        items.push(PlannedTrack {
            track: track.clone(),
            action,
        });
    }
    ImportPlan { items }
}

/// 路径比较键。分隔符统一，Windows 上还要忽略大小写——
/// 那边 `A.flac` 和 `a.flac` 是同一个文件。
pub(crate) fn path_key(path: &str) -> String {
    let unified = path.replace('\\', "/");
    if cfg!(windows) {
        unified.to_lowercase()
    } else {
        unified
    }
}

/// (歌手, 曲名) 的比较键。任一为空就返回 None——
/// 用空歌手当键会把一堆不相干的歌并成一首。
pub(crate) fn identity_key(artist: &str, title: &str) -> Option<(String, String)> {
    let a = matching_key(artist);
    let t = matching_key(title);
    (!a.is_empty() && !t.is_empty()).then_some((a, t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn existing(id: &str, path: &str, title: &str, artist: &str) -> ExistingTrack {
        ExistingTrack {
            id: id.into(),
            audio_path: path.into(),
            title: title.into(),
            artist: artist.into(),
        }
    }

    fn scanned(path: &str, title: &str, artist: &str) -> ScannedTrack {
        ScannedTrack {
            path: path.into(),
            tag_title: title.into(),
            tag_artist: artist.into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_new_file_continues_the_id_sequence() {
        let index = LibraryIndex::from_tracks(&[
            existing("001", "D:/a/001.flac", "夜行", "ヨルシカ"),
            existing("209", "D:/a/209.flac", "再会", "Vaundy"),
        ]);
        let plan = plan(&[scanned("D:/a/新曲.flac", "新曲", "米津玄師")], &index);
        assert_eq!(
            plan.items[0].action,
            Action::New {
                song_id: "210".into()
            }
        );
    }

    #[test]
    fn an_empty_library_starts_at_001() {
        let plan = plan(
            &[scanned("D:/a/x.flac", "x", "y")],
            &LibraryIndex::empty(),
        );
        assert_eq!(
            plan.items[0].action,
            Action::New {
                song_id: "001".into()
            }
        );
    }

    #[test]
    fn the_same_path_is_already_imported() {
        let index = LibraryIndex::from_tracks(&[existing("001", "D:/a/001.flac", "夜行", "ヨルシカ")]);
        let plan = plan(&[scanned("D:/a/001.flac", "夜行", "ヨルシカ")], &index);
        assert_eq!(
            plan.items[0].action,
            Action::AlreadyImported {
                song_id: "001".into()
            }
        );
    }

    #[test]
    fn path_separators_and_case_do_not_create_duplicates() {
        // 库里存的是反斜杠绝对路径，扫描出来可能是正斜杠
        let index =
            LibraryIndex::from_tracks(&[existing("001", r"D:\a\001.flac", "夜行", "ヨルシカ")]);
        let forward = plan(&[scanned("D:/a/001.flac", "夜行", "ヨルシカ")], &index);
        assert!(matches!(
            forward.items[0].action,
            Action::AlreadyImported { .. }
        ));

        if cfg!(windows) {
            let upper = plan(&[scanned(r"d:\A\001.FLAC", "夜行", "ヨルシカ")], &index);
            assert!(matches!(
                upper.items[0].action,
                Action::AlreadyImported { .. }
            ));
        }
    }

    #[test]
    fn the_same_song_at_a_different_path_is_flagged_not_decided() {
        let index = LibraryIndex::from_tracks(&[existing("001", "D:/a/001.flac", "夜行", "ヨルシカ")]);
        let plan = plan(&[scanned("D:/b/夜行.mp3", "夜行", "ヨルシカ")], &index);
        assert_eq!(
            plan.items[0].action,
            Action::PossibleDuplicate {
                song_id: "001".into(),
                existing_path: "D:/a/001.flac".into(),
            }
        );
        // 关键：不写库
        assert_eq!(plan.summary().new, 0);
    }

    #[test]
    fn identity_matching_normalises_width_and_case() {
        // ＹＯＡＳＯＢＩ（全角）和 yoasobi 是同一个歌手
        let index = LibraryIndex::from_tracks(&[existing("001", "D:/a/1.flac", "Idol", "YOASOBI")]);
        let plan = plan(&[scanned("D:/b/2.flac", "ｉｄｏｌ", "ＹＯＡＳＯＢＩ")], &index);
        assert!(matches!(
            plan.items[0].action,
            Action::PossibleDuplicate { .. }
        ));
    }

    #[test]
    fn duplicates_inside_one_batch_import_only_once() {
        let batch = [
            scanned("D:/a/1.flac", "夜行", "ヨルシカ"),
            scanned("D:/b/1.mp3", "夜行", "ヨルシカ"),
        ];
        let plan = plan(&batch, &LibraryIndex::empty());
        assert!(matches!(plan.items[0].action, Action::New { .. }));
        assert_eq!(
            plan.items[1].action,
            Action::DuplicateInBatch {
                first_path: "D:/a/1.flac".into()
            }
        );
        assert_eq!(plan.summary().new, 1);
    }

    #[test]
    fn the_same_file_twice_in_one_batch_is_caught_by_path() {
        let batch = [
            scanned("D:/a/1.flac", "夜行", "ヨルシカ"),
            scanned("D:/a/1.flac", "夜行", "ヨルシカ"),
        ];
        let plan = plan(&batch, &LibraryIndex::empty());
        assert!(matches!(
            plan.items[1].action,
            Action::DuplicateInBatch { .. }
        ));
    }

    #[test]
    fn a_track_without_a_title_is_skipped() {
        let plan = plan(&[scanned("D:/a/1.flac", "", "")], &LibraryIndex::empty());
        assert!(matches!(plan.items[0].action, Action::Skipped { .. }));
        assert_eq!(plan.summary().skipped, 1);
    }

    #[test]
    fn an_empty_artist_does_not_merge_unrelated_songs() {
        // 两首都没歌手，曲名不同——不该因为歌手键都是空就撞上
        let index = LibraryIndex::from_tracks(&[existing("001", "D:/a/1.flac", "曲A", "")]);
        let batch = [
            ScannedTrack {
                path: "D:/b/2.flac".into(),
                guess_title: "曲B".into(),
                ..Default::default()
            },
            ScannedTrack {
                path: "D:/b/3.flac".into(),
                guess_title: "曲C".into(),
                ..Default::default()
            },
        ];
        let plan = plan(&batch, &index);
        assert_eq!(plan.summary().new, 2, "没有歌手的歌不该互相判重");
    }

    #[test]
    fn planning_twice_gives_the_same_result() {
        // 重跑幂等：同一批文件、同一个库，计划必须一模一样
        let index = LibraryIndex::from_tracks(&[existing("001", "D:/a/1.flac", "夜行", "ヨルシカ")]);
        let batch = [
            scanned("D:/a/1.flac", "夜行", "ヨルシカ"),
            scanned("D:/a/2.flac", "花に亡霊", "ヨルシカ"),
        ];
        let first = plan(&batch, &index);
        let second = plan(&batch, &index);
        let ids: Vec<_> = first.items.iter().map(|i| i.action.clone()).collect();
        let ids2: Vec<_> = second.items.iter().map(|i| i.action.clone()).collect();
        assert_eq!(ids, ids2);
    }

    #[test]
    fn id_width_grows_past_999() {
        let index = LibraryIndex::from_tracks(&[existing("0999", "D:/a/1.flac", "a", "b")]);
        let plan = plan(&[scanned("D:/a/2.flac", "c", "d")], &index);
        assert_eq!(
            plan.items[0].action,
            Action::New {
                song_id: "1000".into()
            }
        );
    }

    #[test]
    fn non_numeric_ids_do_not_break_allocation() {
        let index = LibraryIndex::from_tracks(&[
            existing("手动加的", "D:/a/1.flac", "a", "b"),
            existing("007", "D:/a/2.flac", "c", "d"),
        ]);
        let plan = plan(&[scanned("D:/a/3.flac", "e", "f")], &index);
        assert_eq!(
            plan.items[0].action,
            Action::New {
                song_id: "008".into()
            }
        );
    }

    #[test]
    fn summary_counts_every_category() {
        let index = LibraryIndex::from_tracks(&[existing("001", "D:/a/1.flac", "夜行", "ヨルシカ")]);
        let batch = [
            scanned("D:/a/1.flac", "夜行", "ヨルシカ"),  // AlreadyImported
            scanned("D:/b/1.mp3", "夜行", "ヨルシカ"),   // PossibleDuplicate
            scanned("D:/c/新.flac", "新曲", "米津玄師"), // New
            scanned("D:/d/新.mp3", "新曲", "米津玄師"),  // DuplicateInBatch
            scanned("D:/e/x.flac", "", ""),              // Skipped
        ];
        let plan = plan(&batch, &index);
        assert_eq!(
            plan.summary(),
            PlanSummary {
                total: 5,
                new: 1,
                already_imported: 1,
                possible_duplicates: 1,
                duplicates_in_batch: 1,
                skipped: 1,
            }
        );
        assert_eq!(plan.to_import().count(), 1);
    }
}

#[cfg(test)]
mod wire_format {
    use super::*;

    /// ImportPage.tsx 读的是 action.songId / action.existingPath / action.firstPath。
    /// 枚举上的 rename_all 只改变体名（kind 的取值），结构体变体里的字段名要单独改。
    #[test]
    fn actions_use_the_field_names_the_frontend_reads() {
        let dup = serde_json::to_value(Action::PossibleDuplicate {
            song_id: "001".into(),
            existing_path: "D:/a.flac".into(),
        })
        .unwrap();
        assert_eq!(dup["kind"], "possibleDuplicate");
        assert_eq!(dup["songId"], "001", "实际序列化出来的是：{dup}");
        assert_eq!(dup["existingPath"], "D:/a.flac", "实际序列化出来的是：{dup}");

        let batch = serde_json::to_value(Action::DuplicateInBatch {
            first_path: "D:/b.flac".into(),
        })
        .unwrap();
        assert_eq!(batch["firstPath"], "D:/b.flac", "实际序列化出来的是：{batch}");

        let new = serde_json::to_value(Action::New { song_id: "002".into() }).unwrap();
        assert_eq!(new["songId"], "002", "实际序列化出来的是：{new}");
    }
}
