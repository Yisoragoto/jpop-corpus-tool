use anyhow::Result;
use jp_import::ScanResult;
use serde::Serialize;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolderView {
    pub path: String,
    pub reason: jp_import::WatchReason,
    /// 往下看几层。1 是只看直接放在这里的文件
    pub depth: usize,
    /// 库里有多少首歌在它下面
    pub songs: usize,
    /// 里面库里没有的音频文件数
    pub unknown: usize,
    /// 其中会被当成新歌导入的（其余是疑似重复、放不了的格式、读不出曲名）
    pub new: usize,
}

/// 语料库文件夹里还没导入的音频（`library_new_audio`）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAudio {
    /// 去看了哪些文件夹。要原样端给界面——「为什么这个文件没被发现」只能靠它回答
    pub folders: Vec<WatchedFolderView>,
    /// 库里没有的那些文件的计划，和导入页扫出来的是同一种东西
    pub scan: ScanResult,
}

pub fn new_audio(
    corpus: &jp_corpus::Corpus,
    root: &std::path::Path,
    ignored_folders: &[String],
    ignored_files: &[String],
    tokenizer_ready: bool,
) -> Result<NewAudio> {
    let index = jp_import::LibraryIndex::from_corpus(corpus)?;
    let mut stmt = corpus.connection().prepare(
        "SELECT audio_path FROM songs WHERE audio_path IS NOT NULL AND audio_path <> ''",
    )?;
    let audio_paths = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;

    let folders = jp_import::watched_folders(root, &audio_paths, ignored_folders);
    let unknown = jp_import::unknown_audio(&folders, &index, ignored_files, ignored_folders);

    let mut tracks: Vec<jp_import::ScannedTrack> = unknown
        .iter()
        .map(|file| jp_import::scan_file(&file.path))
        .collect();
    tracks.sort_by(|a, b| a.path.cmp(&b.path));
    let scan = jp_import::review(&tracks, &index, tokenizer_ready, Vec::new());

    // 每个文件夹里有几个、其中几个是新歌。计划里的路径是 `scan_file` 统一过分隔符的，
    // 所以这边也拿同一个函数算键，不直接比 `file.path`
    let folder_of: std::collections::HashMap<String, usize> = unknown
        .iter()
        .map(|file| (jp_import::scan_file_path(&file.path), file.folder))
        .collect();
    let mut views: Vec<WatchedFolderView> = folders
        .iter()
        .map(|folder| WatchedFolderView {
            // 分隔符统一成一种：界面拿它和计划里的路径比前缀，也原样显示给用户
            path: jp_import::scan_file_path(&folder.path),
            reason: folder.reason,
            depth: folder.depth,
            songs: folder.songs,
            unknown: 0,
            new: 0,
        })
        .collect();
    for item in &scan.items {
        if let Some(view) = folder_of.get(&item.path).and_then(|at| views.get_mut(*at)) {
            view.unknown += 1;
            if item.action.writes() {
                view.new += 1;
            }
        }
    }

    Ok(NewAudio {
        folders: views,
        scan,
    })
}
