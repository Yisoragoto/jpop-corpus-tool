//! 语料库的音频都放在哪些文件夹——启动时去这些地方看有没有还没导入的文件。
//!
//! 导入是「就地引用」：歌留在用户自己的文件夹里，库里只记路径。所以「语料库文件夹」
//! 不是一个写死的目录，而是**库里的歌现在所在的那些文件夹**，外加语料库目录自己。
//! 往这些地方放了新文件，下次启动就该看得见，不用再去导入页重选一遍目录。
//!
//! **只回答「去哪儿找」和「哪些文件库里没有」，不判断是不是新歌**——
//! 那是 [`crate::plan`] 的事（路径不同但曲名歌手相同的，是疑似重复而不是新歌）。
//!
//! 找的范围是推出来的，推错的代价是每次启动都报一堆不相干的文件，所以两条都从严：
//!
//! * 一个文件夹里有库里的歌，只看**它自己这一层**，不往下钻；
//! * 往上并一级（`raw/audio/{歌手}/` → `raw/audio/`）只在那一级**过半的子文件夹**
//!   都已经是库里的文件夹时才做。从下载目录里单挑过两首歌，不该变成盯着整个用户目录。
//!
//! 仍然推错的，用户可以把那个文件夹排除掉（`excluded`），排除的清单由调用方记着。

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::plan::{LibraryIndex, path_key};
use crate::scan::list_audio_where;

/// 为什么盯着这个文件夹。界面要能说清楚，用户才知道一个文件为什么被（没被）发现。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WatchReason {
    /// 语料库目录自己（根上的散文件，以及 `raw/` 下面）
    Library,
    /// 库里有歌直接放在这个文件夹里
    LibraryAudio,
    /// 它的子文件夹过半是库里的文件夹（`raw/audio/` 这种「一个歌手一个文件夹」的上一级）
    Parent,
}

/// 一个要去看的文件夹。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolder {
    pub path: PathBuf,
    /// 往下看几层。1 是只看这一层的文件。
    pub depth: usize,
    pub reason: WatchReason,
    /// 库里有多少首歌在它下面。语料库目录自己不数，是 0。
    pub songs: usize,
}

/// 直接装着库里的歌的文件夹：只看这一层。
const DEPTH_OWN: usize = 1;
/// 并上去的那一级：它自己、歌手文件夹、歌手下面的专辑文件夹。
const DEPTH_PARENT: usize = 3;
/// 语料库目录的 `raw/`：`raw/audio/{歌手}/{专辑}/`。
const DEPTH_LIBRARY_RAW: usize = 4;
/// 并上去至少要有几个库里的子文件夹。只有一个时「上一级」说明不了任何事。
const MIN_SIBLINGS: usize = 2;

/// 算出要去看的文件夹。
///
/// * `audio_paths`：库里每首歌的 `audio_path`；
/// * `excluded`：用户说过「别再看这个文件夹」的那些，不会出现在结果里。被排除的是并上去的那一级时，
///   它下面库里的文件夹仍然各看各的——排除的是「整个上一级」这个推断，不是那些歌所在的地方。
///   从更上面往下翻的时候也要绕开它们，所以 [`unknown_audio`] 还要再收一遍这份清单。
///
/// 现在不在的文件夹（盘没插、目录挪走了）不列：没有东西可看，也谈不上有新歌。
pub fn watched_folders(
    library_root: &Path,
    audio_paths: &[String],
    excluded: &[String],
) -> Vec<WatchedFolder> {
    let excluded: HashSet<String> = excluded.iter().map(|p| folder_key(Path::new(p))).collect();

    // 库里的歌直接所在的文件夹 → 那里有几首
    let mut own: BTreeMap<String, (PathBuf, usize)> = BTreeMap::new();
    for audio in audio_paths {
        let Some(dir) = Path::new(audio).parent().filter(|d| !d.as_os_str().is_empty()) else {
            continue;
        };
        own.entry(folder_key(dir)).or_insert_with(|| (dir.to_path_buf(), 0)).1 += 1;
    }
    own.retain(|_, (dir, _)| dir.is_dir());

    // 上一级 → 它下面库里的文件夹
    let mut parents: BTreeMap<String, (PathBuf, Vec<String>)> = BTreeMap::new();
    for (key, (dir, _)) in &own {
        // 盘符根不并：`D:\` 下面两个文件夹里有歌，不等于整个 D 盘是曲库
        let Some(parent) = dir.parent().filter(|p| p.parent().is_some()) else {
            continue;
        };
        parents
            .entry(folder_key(parent))
            .or_insert_with(|| (parent.to_path_buf(), Vec::new()))
            .1
            .push(key.clone());
    }

    let mut folders = Vec::new();
    let mut merged: HashSet<String> = HashSet::new();
    for (key, (parent, children)) in &parents {
        if excluded.contains(key) || children.len() < MIN_SIBLINGS {
            continue;
        }
        // 过半才并。数的是子文件夹的个数，不用读里面的文件
        if children.len() * 2 < subfolder_count(parent) {
            continue;
        }
        folders.push(WatchedFolder {
            path: parent.clone(),
            depth: DEPTH_PARENT,
            reason: WatchReason::Parent,
            // 它自己这一层也可能直接放着库里的歌
            songs: children.iter().map(|c| own[c].1).sum::<usize>() + own.get(key).map_or(0, |o| o.1),
        });
        merged.extend(children.iter().cloned());
    }
    for (key, (dir, songs)) in &own {
        if merged.contains(key) || excluded.contains(key) {
            continue;
        }
        folders.push(WatchedFolder {
            path: dir.clone(),
            depth: DEPTH_OWN,
            reason: WatchReason::LibraryAudio,
            songs: *songs,
        });
    }

    // 语料库目录自己：根上只看散文件（`output/` 里有变调缓存的 WAV，开发时根下面还有整个源码树），
    // `raw/` 是 0.1.x 就定下的放音频的地方，往下看
    for (dir, depth) in [(library_root.to_path_buf(), DEPTH_OWN), (library_root.join("raw"), DEPTH_LIBRARY_RAW)] {
        if dir.is_dir() && !excluded.contains(&folder_key(&dir)) {
            folders.push(WatchedFolder { path: dir, depth, reason: WatchReason::Library, songs: 0 });
        }
    }

    drop_covered(folders)
}

/// 已经被另一个文件夹整个盖住的不用再列一遍（开发时 `raw/audio/` 就在语料库目录的 `raw/` 下面）。
fn drop_covered(folders: Vec<WatchedFolder>) -> Vec<WatchedFolder> {
    let keys: Vec<String> = folders.iter().map(|f| folder_key(&f.path)).collect();
    let covered = |inner: usize| {
        (0..folders.len()).any(|outer| {
            if outer == inner {
                return false;
            }
            if keys[outer] == keys[inner] {
                // 同一个文件夹推出来两次：留看得深的那个，一样深留前面的
                let (a, b) = (folders[outer].depth, folders[inner].depth);
                return a > b || (a == b && outer < inner);
            }
            let Some(rest) = keys[inner].strip_prefix(&keys[outer]).and_then(|r| r.strip_prefix('/')) else {
                return false;
            };
            let levels_down = rest.split('/').count();
            folders[outer].depth >= levels_down + folders[inner].depth
        })
    };
    let keep: Vec<bool> = (0..folders.len()).map(|i| !covered(i)).collect();
    let mut kept: Vec<WatchedFolder> =
        folders.into_iter().zip(keep).filter_map(|(folder, keep)| keep.then_some(folder)).collect();
    kept.sort_by(|a, b| a.path.cmp(&b.path));
    kept
}

/// 库里没有的一个文件，和它是在哪个文件夹里找到的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownAudio {
    pub path: PathBuf,
    /// `folders` 里的下标。几个文件夹都看得到它时，算最贴近它的那个（路径最长的）
    pub folder: usize,
}

/// 这些文件夹里**库里没有**的音频文件（按路径比，和导入判「已在库中」是同一个键）。
///
/// * `ignored_files`：用户说过「不用再提这个文件」的那些；
/// * `excluded`：和 [`watched_folders`] 收的是同一份。翻目录时遇到它们整个绕开——
///   不然排除了 `music/A`，从 `music/` 往下翻照样会把它里面的文件报出来。
///
/// 只列路径，不读 tag。
pub fn unknown_audio(
    folders: &[WatchedFolder],
    index: &LibraryIndex,
    ignored_files: &[String],
    excluded: &[String],
) -> Vec<UnknownAudio> {
    let ignored: HashSet<String> = ignored_files.iter().map(|p| path_key(p)).collect();
    let excluded: HashSet<String> = excluded.iter().map(|p| folder_key(Path::new(p))).collect();
    let enter = |dir: &Path| !excluded.contains(&folder_key(dir));
    // 贴得近的先认领：`music/` 和 `music/歌手/` 都在看的时候，歌手文件夹里的算歌手文件夹的
    let mut order: Vec<usize> = (0..folders.len()).collect();
    order.sort_by_key(|&at| std::cmp::Reverse(folder_key(&folders[at].path).len()));

    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for folder in order {
        let watched = &folders[folder];
        for path in list_audio_where(&watched.path, watched.depth, &enter) {
            let text = path.to_string_lossy();
            let key = path_key(&text);
            if index.song_for_path(&text).is_none() && !ignored.contains(&key) && seen.insert(key) {
                out.push(UnknownAudio { path, folder });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// 文件夹的比较键：和路径同一套（分隔符统一、Windows 上不分大小写），再去掉末尾的分隔符。
fn folder_key(dir: &Path) -> String {
    let key = path_key(&dir.to_string_lossy());
    let trimmed = key.trim_end_matches('/');
    // `D:/` 去掉末尾会变成 `D:`，那是「D 盘的当前目录」，不是根
    if trimmed.is_empty() || trimmed.ends_with(':') { key } else { trimmed.to_string() }
}

fn subfolder_count(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().filter(|e| e.path().is_dir()).count())
        .unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::ExistingTrack;

    /// 一个临时的盘面：`songs` 是库里的歌（会真的建出文件），`loose` 是放进去但没导入的。
    struct Disk {
        root: PathBuf,
        songs: Vec<String>,
    }

    impl Disk {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("jp-import-watch-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("library")).unwrap();
            Self { root, songs: Vec::new() }
        }

        fn touch(&self, relative: &str) -> PathBuf {
            let path = self.root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"not really audio").unwrap();
            path
        }

        /// 放一个文件，并且算它已经在库里
        fn song(&mut self, relative: &str) {
            let path = self.touch(relative);
            self.songs.push(path.display().to_string());
        }

        fn library(&self) -> PathBuf {
            self.root.join("library")
        }

        fn watched(&self, excluded: &[&Path]) -> Vec<WatchedFolder> {
            let excluded: Vec<String> = excluded.iter().map(|p| p.display().to_string()).collect();
            watched_folders(&self.library(), &self.songs, &excluded)
        }

        fn index(&self) -> LibraryIndex {
            let tracks: Vec<ExistingTrack> = self
                .songs
                .iter()
                .enumerate()
                .map(|(n, path)| ExistingTrack {
                    id: format!("{:03}", n + 1),
                    audio_path: path.clone(),
                    title: format!("曲{n}"),
                    artist: "歌手".into(),
                })
                .collect();
            LibraryIndex::from_tracks(&tracks)
        }

        /// 相对盘面根的路径，`/` 分隔，好比较
        fn relative(&self, path: &Path) -> String {
            path.strip_prefix(&self.root).unwrap().to_string_lossy().replace('\\', "/")
        }

        fn watched_names(&self, excluded: &[&Path]) -> Vec<(String, usize, WatchReason)> {
            self.watched(excluded).iter().map(|f| (self.relative(&f.path), f.depth, f.reason)).collect()
        }

        fn unknown(&self, ignored: &[&Path]) -> Vec<String> {
            let ignored: Vec<String> = ignored.iter().map(|p| p.display().to_string()).collect();
            unknown_audio(&self.watched(&[]), &self.index(), &ignored, &[])
                .iter()
                .map(|u| self.relative(&u.path))
                .collect()
        }

        /// 排除了这些文件夹之后还找得到的：(文件, 记在哪个文件夹名下)
        fn unknown_excluding(&self, excluded: &[&Path]) -> Vec<(String, String)> {
            let watched = self.watched(excluded);
            let excluded: Vec<String> = excluded.iter().map(|p| p.display().to_string()).collect();
            unknown_audio(&watched, &self.index(), &[], &excluded)
                .iter()
                .map(|u| (self.relative(&u.path), self.relative(&watched[u.folder].path)))
                .collect()
        }
    }

    impl Drop for Disk {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// 最常见的情形：往一个已经导入过的下载文件夹里又放了一首。
    #[test]
    fn a_file_dropped_next_to_library_songs_is_found() {
        let mut disk = Disk::new("next-to");
        disk.song("downloads/sakana/サカナクション - 怪獣.flac");
        disk.song("downloads/sakana/サカナクション - モス.flac");
        disk.touch("downloads/sakana/サカナクション - 新宝島.flac");
        disk.touch("downloads/sakana/cover.jpg");

        assert_eq!(
            disk.watched_names(&[]),
            [
                ("downloads/sakana".to_string(), 1, WatchReason::LibraryAudio),
                ("library".to_string(), 1, WatchReason::Library),
            ]
        );
        assert_eq!(disk.unknown(&[]), ["downloads/sakana/サカナクション - 新宝島.flac"]);
    }

    /// 有歌的文件夹只看它自己这一层：它下面、它旁边的文件夹都不是「语料库文件夹」。
    /// 从下载目录里单挑过一首歌，不该把整个下载目录的子文件夹都翻一遍。
    #[test]
    fn a_folder_with_library_songs_is_not_searched_any_deeper_or_wider() {
        let mut disk = Disk::new("own-level");
        disk.song("downloads/picked.flac");
        disk.touch("downloads/games/sfx/explosion.ogg");
        disk.touch("other/unrelated.mp3");

        assert_eq!(disk.unknown(&[]), Vec::<String>::new());
    }

    /// `raw/audio/{歌手}/`：上一级的子文件夹大多是库里的，就把上一级整个看了——
    /// 新建一个歌手文件夹、或者把文件直接丢在上一级，都要找得到。
    #[test]
    fn a_parent_made_mostly_of_library_folders_is_watched_as_a_whole() {
        let mut disk = Disk::new("parent");
        disk.song("music/ヨルシカ/014.flac");
        disk.song("music/ヨルシカ/015.flac");
        disk.song("music/Vaundy/101.flac");
        disk.song("music/きのこ帝国/120.flac");
        disk.touch("music/ヨルシカ/晴る.flac");
        disk.touch("music/新しい歌手/新曲.flac");
        disk.touch("music/新しい歌手/アルバム/収録曲.flac");
        disk.touch("music/散らかった曲.mp3");

        let watched = disk.watched(&[]);
        let parent = watched.iter().find(|f| f.reason == WatchReason::Parent).expect("上一级要被并上去");
        assert_eq!(disk.relative(&parent.path), "music");
        assert_eq!(parent.songs, 4, "它下面库里的歌都算它的");

        // 上一级自己也直接放着一首库里的歌：还是只列它一次，那一首也算进去
        disk.song("music/直接放在上一级的.flac");
        let watched = disk.watched(&[]);
        let music: Vec<_> = watched.iter().filter(|f| disk.relative(&f.path) == "music").collect();
        assert_eq!(music.len(), 1, "{watched:?}");
        assert_eq!((music[0].reason, music[0].depth, music[0].songs), (WatchReason::Parent, 3, 5));
        assert!(
            !watched.iter().any(|f| f.reason == WatchReason::LibraryAudio),
            "并上去之后下面的文件夹不用再各列一遍：{watched:?}"
        );
        assert_eq!(
            disk.unknown(&[]),
            [
                "music/ヨルシカ/晴る.flac",
                "music/散らかった曲.mp3",
                "music/新しい歌手/アルバム/収録曲.flac",
                "music/新しい歌手/新曲.flac",
            ],
            "库里已有的 4 首不该出现"
        );
    }

    /// 上一级里库的文件夹只占少数：那是一个大音乐目录（或者用户目录），只导过其中两个。
    /// 并上去的话每次启动都会报出几千个不相干的文件。
    #[test]
    fn a_parent_with_mostly_unrelated_folders_is_not_watched_as_a_whole() {
        let mut disk = Disk::new("minority");
        disk.song("home/Music/a.flac");
        disk.song("home/Downloads/b.flac");
        for other in ["Documents", "Pictures", "Videos", "Desktop", "AppData"] {
            std::fs::create_dir_all(disk.root.join("home").join(other)).unwrap();
        }
        disk.touch("home/Videos/clip.mp3");
        disk.touch("home/Music/new.flac");

        assert!(
            !disk.watched(&[]).iter().any(|f| f.reason == WatchReason::Parent),
            "7 个子文件夹里只有 2 个是库里的，不能并"
        );
        assert_eq!(disk.unknown(&[]), ["home/Music/new.flac"], "各自那一层照看");
    }

    /// 只有一个库里的子文件夹时不并：「上一级」说明不了任何事。
    #[test]
    fn a_single_library_folder_does_not_pull_in_its_parent() {
        let mut disk = Disk::new("single");
        disk.song("wyy/sakana/a.flac");
        disk.touch("wyy/b.flac");

        assert_eq!(disk.unknown(&[]), Vec::<String>::new());
    }

    /// 用户排除了并上去的那一级：推断撤回，库里的文件夹仍然各看各的。
    #[test]
    fn excluding_a_merged_parent_falls_back_to_the_folders_under_it() {
        let mut disk = Disk::new("exclude-parent");
        disk.song("music/A/1.flac");
        disk.song("music/B/2.flac");
        disk.touch("music/A/new.flac");
        disk.touch("music/C/unrelated.flac");
        let parent = disk.root.join("music");

        assert_eq!(disk.unknown(&[]).len(), 2, "没排除时两个都找得到");
        let names = disk.watched_names(&[&parent]);
        assert_eq!(
            names,
            [
                ("library".to_string(), 1, WatchReason::Library),
                ("music/A".to_string(), 1, WatchReason::LibraryAudio),
                ("music/B".to_string(), 1, WatchReason::LibraryAudio),
            ]
        );
        assert_eq!(
            disk.unknown_excluding(&[&parent]),
            [("music/A/new.flac".to_string(), "music/A".to_string())],
            "music/C 不是库里的文件夹，上一级撤回之后就不该再报"
        );
        // 再把其中一个也排除掉
        let a = disk.root.join("music").join("A");
        let names = disk.watched_names(&[&parent, &a]);
        assert!(!names.iter().any(|(name, ..)| name == "music/A"), "{names:?}");
        assert_eq!(disk.unknown_excluding(&[&parent, &a]), []);
    }

    /// 上一级整个在看的时候排除它下面的一个文件夹：从上一级往下翻也要绕开它。
    #[test]
    fn an_excluded_folder_is_skipped_even_when_its_parent_is_watched() {
        let mut disk = Disk::new("exclude-child");
        disk.song("music/A/1.flac");
        disk.song("music/B/2.flac");
        disk.touch("music/A/new.flac");
        disk.touch("music/A/album/deep.flac");
        disk.touch("music/B/new.flac");
        let a = disk.root.join("music").join("A");

        assert_eq!(disk.unknown_excluding(&[]).len(), 3);
        assert_eq!(
            disk.unknown_excluding(&[&a]),
            [("music/B/new.flac".to_string(), "music".to_string())],
            "排除的文件夹和它下面的都不该再报"
        );
    }

    /// 两级都在看（`music/`，和它下面有好几张专辑的歌手文件夹 `music/B/`）：两边都翻得到的文件
    /// 记在贴得最近的那个名下。界面上「这个文件夹里有几个」才对得上，「别再看这个文件夹」也才点得准。
    #[test]
    fn a_file_is_filed_under_the_nearest_watched_folder() {
        let mut disk = Disk::new("nearest");
        disk.song("music/A/1.flac");
        disk.song("music/C/4.flac");
        disk.song("music/B/x/2.flac");
        disk.song("music/B/y/3.flac");
        disk.touch("music/A/new.flac");
        disk.touch("music/B/z/new.flac");
        disk.touch("music/B/x/disc2/too-deep-for-music.flac");

        let roots: Vec<String> = disk
            .watched_names(&[])
            .into_iter()
            .filter(|(_, _, reason)| *reason == WatchReason::Parent)
            .map(|(name, ..)| name)
            .collect();
        assert_eq!(roots, ["music", "music/B"], "前提：两级都并上去了");
        assert_eq!(
            disk.unknown_excluding(&[]),
            [
                ("music/A/new.flac".to_string(), "music".to_string()),
                ("music/B/x/disc2/too-deep-for-music.flac".to_string(), "music/B".to_string()),
                // 从 music/ 往下三层也翻得到它，但 music/B/ 更近
                ("music/B/z/new.flac".to_string(), "music/B".to_string()),
            ]
        );
    }

    /// 排除清单是界面记下来再传回来的，大小写和分隔符可能和盘上的不一样。
    #[cfg(windows)]
    #[test]
    fn an_excluded_folder_matches_regardless_of_case_and_separators() {
        let mut disk = Disk::new("exclude-case");
        disk.song("Downloads/a.flac");
        let spelled = disk.root.join("downloads").display().to_string().replace('\\', "/").to_uppercase() + "/";

        let watched = watched_folders(&disk.library(), &disk.songs, &[spelled]);
        assert!(!watched.iter().any(|f| f.reason == WatchReason::LibraryAudio), "{watched:?}");
    }

    /// 语料库目录自己：根上的散文件和 `raw/` 下面的都找；应用自己写的音频（变调缓存）不算。
    #[test]
    fn the_library_folder_itself_is_watched_but_not_what_the_app_writes_there() {
        let disk = Disk::new("library");
        disk.touch("library/直接放进来的.flac");
        disk.touch("library/raw/audio/歌手/アルバム/曲.flac");
        disk.touch("library/raw/放在 raw 里的.mp3");
        disk.touch("library/output/pitch_cache/abcdef.wav");
        disk.touch("library/backups/old/曲.flac");

        assert_eq!(
            disk.unknown(&[]),
            [
                "library/raw/audio/歌手/アルバム/曲.flac",
                "library/raw/放在 raw 里的.mp3",
                "library/直接放进来的.flac",
            ]
        );
    }

    /// 开发时语料库目录就是仓库根，库里的歌在它的 `raw/audio/{歌手}/` 下面：
    /// 同一片地方不要列三遍，文件也不要报两次。
    #[test]
    fn folders_already_covered_by_the_library_folder_are_listed_once() {
        let mut disk = Disk::new("covered");
        disk.song("library/raw/audio/A/1.flac");
        disk.song("library/raw/audio/B/2.flac");
        disk.touch("library/raw/audio/C/3.flac");

        assert_eq!(
            disk.watched_names(&[]),
            [
                ("library".to_string(), 1, WatchReason::Library),
                ("library/raw".to_string(), 4, WatchReason::Library),
            ]
        );
        assert_eq!(disk.unknown(&[]), ["library/raw/audio/C/3.flac"]);
        // 文件记在找到它的那个文件夹名下：界面要按文件夹说「这里有几个」
        assert_eq!(
            disk.unknown_excluding(&[]),
            [("library/raw/audio/C/3.flac".to_string(), "library/raw".to_string())]
        );
    }

    /// 「不用再提这个文件」的那些不再出现；放不了的格式照样列出来，由计划去说明原因。
    #[test]
    fn ignored_files_are_left_out_and_unsupported_formats_are_not() {
        let mut disk = Disk::new("ignored");
        disk.song("dl/a.flac");
        let skipped = disk.touch("dl/b.flac");
        disk.touch("dl/c.opus");

        assert_eq!(disk.unknown(&[]), ["dl/b.flac", "dl/c.opus"]);
        assert_eq!(disk.unknown(&[&skipped]), ["dl/c.opus"]);
    }

    /// 盘没插、文件夹挪走了：没有东西可看，不列，也不报错。
    #[test]
    fn folders_that_are_gone_are_not_listed() {
        let disk = Disk::new("gone");
        let songs = vec![
            disk.root.join("unplugged").join("a.flac").display().to_string(),
            String::new(),
            "没有目录的文件名.flac".to_string(),
        ];
        let watched = watched_folders(&disk.library(), &songs, &[]);
        assert_eq!(watched.len(), 1, "{watched:?}");
        assert_eq!(watched[0].reason, WatchReason::Library);
    }

    #[test]
    fn a_drive_root_keeps_its_slash() {
        if cfg!(windows) {
            assert_eq!(folder_key(Path::new(r"D:\")), "d:/");
            assert_eq!(folder_key(Path::new(r"D:\Music\")), "d:/music");
        } else {
            assert_eq!(folder_key(Path::new("/")), "/");
            assert_eq!(folder_key(Path::new("/music/")), "/music");
        }
    }
}
