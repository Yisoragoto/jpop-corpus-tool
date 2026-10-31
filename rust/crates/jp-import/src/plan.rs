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
use crate::version::{self, VersionFacts};

/// 打算对一个扫描条目做什么。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    /// 库里没有，新建。`songId` 是预分配好的，执行时直接用。
    ///
    /// `versionOf`：库里有同名同歌手的歌，但这是**另一个版本**（Live、翻唱、重制、时长不同……），
    /// 照常作为一首新歌导入，界面提一句「是 012 的另一个版本」。
    // 结构体变体的字段名要单独改：枚举上的 rename_all 只管变体名。
    #[serde(rename_all = "camelCase")]
    New {
        song_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version_of: Option<VersionOf>,
    },
    /// 这个文件路径已经在库里。重跑扫描时绝大多数条目都落这里。
    ///
    /// `fileChanged`：同一路径，但大小或修改时间变了（换了压制、改了标签）。
    /// 导入时刷新文件信息；来自文件的字段跟着更新，你改过的、刮削来的不动。
    #[serde(rename_all = "camelCase")]
    AlreadyImported {
        song_id: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        file_changed: bool,
    },
    /// 同一个录音换了个文件：同一个 ISRC / MusicBrainz 录音，或者曲名歌手相同、版本标记相同、时长相近。
    ///
    /// 可能是换了音源（FLAC 换成 MP3），也可能是挪了位置。**不自动决定**——
    /// 猜错的代价是把用户的库弄脏，交给人看。`reason` 说是凭什么判的。
    #[serde(rename_all = "camelCase")]
    PossibleDuplicate {
        song_id: String,
        existing_path: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reason: String,
    },
    /// 同一批扫描里出现了重复，只导第一个。
    #[serde(rename_all = "camelCase")]
    DuplicateInBatch { first_path: String },
    /// 导不了。
    Skipped { reason: String },
}

/// 「这是库里某首的另一个版本」。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionOf {
    pub song_id: String,
    pub existing_path: String,
    /// 为什么算另一个版本（「版本不同：库里的是原版，这个是 Live」「时长差 21 秒」）
    pub reason: String,
}

impl Action {
    /// 会不会真的往库里写。
    pub fn writes(&self) -> bool {
        matches!(self, Action::New { .. })
    }

    /// 新建（没有同名的其他版本）
    pub fn new_song(song_id: impl Into<String>) -> Self {
        Action::New { song_id: song_id.into(), version_of: None }
    }

    /// 已在库中、文件没变
    pub fn already(song_id: impl Into<String>) -> Self {
        Action::AlreadyImported { song_id: song_id.into(), file_changed: false }
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
#[derive(Debug, Clone, Default)]
pub struct ExistingTrack {
    pub id: String,
    pub audio_path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_sec: Option<f64>,
    pub isrc: String,
    pub recording_mbid: String,
}

impl ExistingTrack {
    fn facts(&self) -> VersionFacts<'_> {
        VersionFacts {
            title: &self.title,
            album: &self.album,
            duration_sec: self.duration_sec,
            isrc: &self.isrc,
            recording_mbid: &self.recording_mbid,
        }
    }
}

fn scanned_facts(track: &ScannedTrack) -> VersionFacts<'_> {
    VersionFacts {
        title: track.title(),
        album: track.album(),
        duration_sec: track.duration_sec,
        isrc: &track.isrc,
        recording_mbid: &track.recording_mbid,
    }
}

/// 现有曲库的索引：路径 → id，(歌手, 曲名) → 同名的那几首，ISRC / 录音编号 → id，
/// 外加每个在用文件的大小和修改时间（重扫时判断变没变）。
pub struct LibraryIndex {
    by_path: HashMap<String, String>,
    by_identity: HashMap<(String, String), Vec<ExistingTrack>>,
    by_isrc: HashMap<String, ExistingTrack>,
    by_mbid: HashMap<String, ExistingTrack>,
    stamps: HashMap<String, (Option<u64>, Option<f64>)>,
    /// 下一个可用的数字 id
    next_num: u32,
    /// id 零填充宽度。库里是 `001` 这种三位，超过 999 自然变四位。
    width: usize,
}

fn code_key(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase()
}

impl LibraryIndex {
    pub fn from_tracks(tracks: &[ExistingTrack]) -> Self {
        let mut index = Self {
            by_path: HashMap::new(),
            by_identity: HashMap::new(),
            by_isrc: HashMap::new(),
            by_mbid: HashMap::new(),
            stamps: HashMap::new(),
            next_num: 1,
            width: 3,
        };
        let mut max_num = 0u32;
        for track in tracks {
            index.insert(track);
            // 非数字 id（比如手工加的）不参与序号分配，但也不能撞上
            if let Ok(num) = track.id.trim().parse::<u32>() {
                max_num = max_num.max(num);
                index.width = index.width.max(track.id.trim().len());
            }
        }
        index.next_num = max_num + 1;
        index
    }

    fn insert(&mut self, track: &ExistingTrack) {
        self.by_path.insert(path_key(&track.audio_path), track.id.clone());
        if let Some(key) = identity_key(&track.artist, &track.title) {
            self.by_identity.entry(key).or_default().push(track.clone());
        }
        let isrc = code_key(&track.isrc);
        if !isrc.is_empty() {
            self.by_isrc.entry(isrc).or_insert_with(|| track.clone());
        }
        let mbid = track.recording_mbid.trim().to_ascii_lowercase();
        if !mbid.is_empty() {
            self.by_mbid.entry(mbid).or_insert_with(|| track.clone());
        }
    }

    /// 空库。第一首会是 `001`。
    pub fn empty() -> Self {
        Self::from_tracks(&[])
    }

    /// 从数据库读一份索引。
    pub fn from_corpus(corpus: &jp_corpus::Corpus) -> anyhow::Result<Self> {
        let mut stmt = corpus.connection().prepare(
            "SELECT id, audio_path, title, artist, COALESCE(album,''), duration_sec, COALESCE(isrc,''), COALESCE(recording_mbid,'') FROM songs",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ExistingTrack {
                    id: row.get(0)?,
                    audio_path: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    title: row.get(2)?,
                    artist: row.get(3)?,
                    album: row.get(4)?,
                    duration_sec: row.get(5)?,
                    isrc: row.get(6)?,
                    recording_mbid: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut index = Self::from_tracks(&rows);
        index.stamps = jp_corpus::audio_files::stamps(corpus.connection())?;
        Ok(index)
    }

    /// 记下一个在用文件的大小和修改时间（测试、迁移用；`from_corpus` 自己会读）。
    pub fn set_stamp(&mut self, audio_path: &str, size: Option<u64>, mtime: Option<f64>) {
        self.stamps.insert(path_key(audio_path), (size, mtime));
    }

    /// 这个文件库里有、大小和修改时间都和上次一样：重扫时不用再读标签。
    pub fn unchanged(&self, audio_path: &str, size: Option<u64>, mtime: Option<f64>) -> bool {
        let key = path_key(audio_path);
        self.by_path.contains_key(&key)
            && self.stamps.get(&key).is_some_and(|stamp| jp_corpus::audio_files::same_stamp(*stamp, (size, mtime)))
    }

    /// 库里有这个路径、上次记过它的大小和修改时间，而这次不一样了。
    /// 没记过（升级前导入的）不算变了：那次只是补记一下。
    pub fn file_changed(&self, audio_path: &str, size: Option<u64>, mtime: Option<f64>) -> bool {
        match self.stamps.get(&path_key(audio_path)) {
            Some(&(Some(old_size), old_mtime)) => {
                !jp_corpus::audio_files::same_stamp((Some(old_size), old_mtime), (size, mtime)) && size.is_some()
            }
            _ => false,
        }
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
            .and_then(|tracks| tracks.first())
            .map(|track| track.id.as_str())
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
        self.insert(&ExistingTrack {
            id: id.to_string(),
            audio_path: audio_path.to_string(),
            title: title.to_string(),
            artist: artist.to_string(),
            ..Default::default()
        });
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
    /// 已在库中、但文件变了的（导入时只刷新文件信息）
    #[serde(default)]
    pub files_changed: usize,
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
                Action::AlreadyImported { file_changed, .. } => {
                    s.already_imported += 1;
                    s.files_changed += usize::from(file_changed);
                }
                Action::PossibleDuplicate { .. } => s.possible_duplicates += 1,
                Action::DuplicateInBatch { .. } => s.duplicates_in_batch += 1,
                Action::Skipped { .. } => s.skipped += 1,
            }
        }
        s
    }

    /// 有没有要做的事：新歌，或者变了的文件要刷新
    pub fn has_work(&self) -> bool {
        self.items.iter().any(|i| i.action.writes() || matches!(i.action, Action::AlreadyImported { file_changed: true, .. }))
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
    // 同一批里已经见过的，防止一次扫描把同一首导两遍（同名的另一个版本照导）
    let mut batch_paths: HashMap<String, String> = HashMap::new();
    let mut batch_identities: HashMap<(String, String), Vec<&ScannedTrack>> = HashMap::new();

    for track in tracks {
        let pkey = path_key(&track.path);
        // 库里有、没变、没读标签的：直接「已在库中」，别拿它的空标签去比
        if track.unchanged
            && let Some(id) = index.by_path.get(&pkey)
        {
            items.push(PlannedTrack { track: track.clone(), action: Action::already(id.clone()) });
            continue;
        }
        // 放不了的格式不入库。但旧版本导进去过的那些照实报「已在库中」——
        // 它确实在库里，说「跳过」反而是错的
        let unsupported = track.unsupported.as_ref().filter(|_| !index.by_path.contains_key(&pkey));
        let action = if let Some(reason) = unsupported {
            Action::Skipped {
                reason: reason.clone(),
            }
        } else if let Some(id) = index.by_path.get(&pkey) {
            // 路径相同是最硬的证据，优先于曲名歌手；没有曲名也照样是「已在库中」
            Action::AlreadyImported {
                song_id: id.clone(),
                file_changed: index.file_changed(&track.path, track.size, track.mtime),
            }
        } else if !track.has_identity() {
            Action::Skipped {
                reason: "没有曲名，tag 和文件名都定不出".into(),
            }
        } else if let Some(first) = batch_paths.get(&pkey) {
            Action::DuplicateInBatch {
                first_path: first.clone(),
            }
        } else {
            let facts = scanned_facts(track);
            let ikey = identity_key(track.artist(), track.title());
            let same_code = Some(code_key(&track.isrc))
                .filter(|c| !c.is_empty())
                .and_then(|c| index.by_isrc.get(&c))
                .or_else(|| {
                    Some(track.recording_mbid.trim().to_ascii_lowercase())
                        .filter(|m| !m.is_empty())
                        .and_then(|m| index.by_mbid.get(&m))
                });
            let candidates: &[ExistingTrack] = ikey.as_ref().and_then(|k| index.by_identity.get(k)).map_or(&[], Vec::as_slice);
            if let Some(existing) = same_code {
                // 外部编号最硬：同一个 ISRC / 录音编号就是同一个录音，曲名写法不同也一样
                Action::PossibleDuplicate {
                    song_id: existing.id.clone(),
                    existing_path: existing.audio_path.clone(),
                    reason: if code_key(&track.isrc).is_empty() { "同一个 MusicBrainz 录音".into() } else { "同一个 ISRC".into() },
                }
            } else if let Some(existing) = candidates.iter().find(|c| version::difference(&c.facts(), &facts).is_none()) {
                Action::PossibleDuplicate {
                    song_id: existing.id.clone(),
                    existing_path: existing.audio_path.clone(),
                    reason: version::sameness(&existing.facts(), &facts),
                }
            } else if let Some(first) = ikey
                .as_ref()
                .and_then(|k| batch_identities.get(k))
                .and_then(|seen| seen.iter().find(|t| version::difference(&scanned_facts(t), &facts).is_none()))
            {
                Action::DuplicateInBatch {
                    first_path: first.path.clone(),
                }
            } else {
                let id = index.allocate(next_num);
                next_num += 1;
                batch_paths.insert(pkey, track.path.clone());
                if let Some(k) = ikey {
                    batch_identities.entry(k).or_default().push(track);
                }
                let version_of = candidates.first().map(|existing| VersionOf {
                    song_id: existing.id.clone(),
                    existing_path: existing.audio_path.clone(),
                    reason: version::difference(&existing.facts(), &facts).unwrap_or_default(),
                });
                Action::New { song_id: id, version_of }
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
            ..Default::default()
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
            Action::new_song("210")
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
            Action::new_song("001")
        );
    }

    #[test]
    fn the_same_path_is_already_imported() {
        let index = LibraryIndex::from_tracks(&[existing("001", "D:/a/001.flac", "夜行", "ヨルシカ")]);
        let plan = plan(&[scanned("D:/a/001.flac", "夜行", "ヨルシカ")], &index);
        assert_eq!(
            plan.items[0].action,
            Action::already("001")
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
                reason: "曲名、歌手相同（没有时长可比）".into(),
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

    /// 放不了的格式（opus、wma）不入库，计划里写明原因；曲名歌手齐全也一样。
    /// 旧版本已经导进去的那些照实报「已在库中」。
    #[test]
    fn an_unsupported_format_is_skipped_with_its_reason() {
        let unsupported = |path: &str| ScannedTrack {
            unsupported: Some("不支持 Opus：播放器没有这种格式的解码器".into()),
            ..scanned(path, "夜行", "ヨルシカ")
        };
        let index = LibraryIndex::from_tracks(&[existing("007", "D:/old/旧.opus", "旧", "某人")]);
        let plan = plan(&[unsupported("D:/a/夜行.opus"), unsupported("D:/old/旧.opus"), scanned("D:/a/夜行.flac", "夜行", "ヨルシカ")], &index);
        assert_eq!(
            plan.items[0].action,
            Action::Skipped { reason: "不支持 Opus：播放器没有这种格式的解码器".into() }
        );
        assert_eq!(plan.items[1].action, Action::already("007"));
        // 被跳过的那个不能占住「夜行 / ヨルシカ」，后面同名的 FLAC 照常导入
        assert_eq!(plan.items[2].action, Action::new_song("008"));
        assert_eq!(plan.to_import().count(), 1);
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
            Action::new_song("1000")
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
            Action::new_song("008")
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
                files_changed: 0,
            }
        );
        assert_eq!(plan.to_import().count(), 1);
    }
}

#[cfg(test)]
mod versions {
    use super::*;

    fn existing(id: &str, path: &str, album: &str, duration: Option<f64>, isrc: &str) -> ExistingTrack {
        ExistingTrack {
            id: id.into(),
            audio_path: path.into(),
            title: "夜に駆ける".into(),
            artist: "YOASOBI".into(),
            album: album.into(),
            duration_sec: duration,
            isrc: isrc.into(),
            ..Default::default()
        }
    }

    fn scanned(path: &str, title: &str, album: &str, duration: Option<f64>, isrc: &str) -> ScannedTrack {
        ScannedTrack {
            path: path.into(),
            tag_title: title.into(),
            tag_artist: "YOASOBI".into(),
            tag_album: album.into(),
            duration_sec: duration,
            isrc: isrc.into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_live_or_shorter_version_is_imported_as_its_own_song() {
        let index = LibraryIndex::from_tracks(&[existing("012", "D:/a/夜に駆ける.flac", "THE BOOK", Some(261.0), "")]);
        let plan = plan(
            &[
                scanned("D:/b/夜に駆ける.flac", "夜に駆ける", "YOASOBI LIVE 2023", Some(275.0), ""),
                scanned("D:/c/夜に駆ける.flac", "夜に駆ける", "THE BOOK", Some(201.0), ""),
            ],
            &index,
        );
        let Action::New { version_of: Some(live), .. } = &plan.items[0].action else { panic!("{:?}", plan.items[0].action) };
        assert_eq!(live.song_id, "012");
        assert!(live.reason.contains("Live"), "{}", live.reason);
        let Action::New { version_of: Some(short), .. } = &plan.items[1].action else { panic!("{:?}", plan.items[1].action) };
        assert!(short.reason.contains("时长差 60 秒"), "{}", short.reason);
        assert_eq!(plan.summary().new, 2);
    }

    #[test]
    fn the_same_recording_in_another_file_is_flagged_not_imported() {
        let index = LibraryIndex::from_tracks(&[existing("012", "D:/a/夜に駆ける.flac", "THE BOOK", Some(261.0), "JPXX12000001")]);
        let plan = plan(
            &[
                // 同一个录音换成 MP3：时长差不到一秒
                scanned("D:/b/夜に駆ける.mp3", "夜に駆ける", "THE BOOK", Some(261.4), ""),
                // 曲名写法不同，但 ISRC 一样
                scanned("D:/b/yorunikakeru.flac", "Racing into the Night", "", Some(261.0), "JP-XX1-20-00001"),
            ],
            &index,
        );
        assert_eq!(
            plan.items[0].action,
            Action::PossibleDuplicate {
                song_id: "012".into(),
                existing_path: "D:/a/夜に駆ける.flac".into(),
                reason: "曲名、歌手相同，时长相差 0.4 秒".into(),
            }
        );
        assert!(matches!(&plan.items[1].action, Action::PossibleDuplicate { reason, .. } if reason == "同一个 ISRC"));
        assert_eq!(plan.summary().new, 0);
    }

    #[test]
    fn different_isrcs_mean_different_recordings() {
        let index = LibraryIndex::from_tracks(&[existing("012", "D:/a/1.flac", "", Some(261.0), "JPXX12000001")]);
        let plan = plan(&[scanned("D:/b/1.flac", "夜に駆ける", "", Some(261.0), "JPXX12100009")], &index);
        assert!(matches!(&plan.items[0].action, Action::New { version_of: Some(v), .. } if v.reason.starts_with("ISRC 不同")));
    }

    #[test]
    fn two_versions_in_one_batch_are_both_imported() {
        let plan = plan(
            &[
                scanned("D:/a/1.flac", "夜に駆ける", "THE BOOK", Some(261.0), ""),
                scanned("D:/a/2.flac", "夜に駆ける", "LIVE", Some(280.0), ""),
                scanned("D:/a/3.mp3", "夜に駆ける", "THE BOOK", Some(261.2), ""),
            ],
            &LibraryIndex::empty(),
        );
        assert!(matches!(plan.items[0].action, Action::New { .. }));
        assert!(matches!(plan.items[1].action, Action::New { .. }));
        assert_eq!(plan.items[2].action, Action::DuplicateInBatch { first_path: "D:/a/1.flac".into() });
    }

    #[test]
    fn unchanged_files_are_already_imported_without_reading_tags_and_changed_ones_are_flagged() {
        let mut index = LibraryIndex::from_tracks(&[
            existing("012", "D:/a/1.flac", "", None, ""),
            existing("013", "D:/a/2.flac", "", None, ""),
            existing("014", "D:/a/3.flac", "", None, ""),
        ]);
        index.set_stamp("D:/a/1.flac", Some(100), Some(1_000.0));
        index.set_stamp("D:/a/2.flac", Some(100), Some(1_000.0));
        assert!(index.unchanged("D:/a/1.flac", Some(100), Some(1_000.5)));
        assert!(!index.unchanged("D:/a/2.flac", Some(120), Some(1_000.0)));
        assert!(!index.unchanged("D:/a/3.flac", Some(1), Some(1.0)), "没记过大小的要读一遍");
        assert!(!index.unchanged("D:/z/new.flac", Some(100), Some(1_000.0)));
        let stub = ScannedTrack { path: "D:/a/1.flac".into(), size: Some(100), mtime: Some(1_000.0), unchanged: true, ..Default::default() };
        let changed = ScannedTrack { size: Some(120), mtime: Some(2_000.0), ..scanned("D:/a/2.flac", "夜に駆ける", "", None, "") };
        let unknown = ScannedTrack { size: Some(1), mtime: Some(1.0), ..scanned("D:/a/3.flac", "夜に駆ける", "", None, "") };
        let plan = plan(&[stub, changed, unknown], &index);
        assert_eq!(plan.items[0].action, Action::already("012"));
        assert_eq!(plan.items[1].action, Action::AlreadyImported { song_id: "013".into(), file_changed: true });
        assert_eq!(plan.items[2].action, Action::already("014"), "升级前导入、没记过的不算变了");
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
            reason: String::new(),
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

        let new = serde_json::to_value(Action::new_song("002")).unwrap();
        assert_eq!(new["songId"], "002", "实际序列化出来的是：{new}");
    }
}
