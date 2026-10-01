//! 曲库维护的应用层：编辑、删除、找回丢了的音频。库里的逻辑在 `jp_import::maintain`，这里负责
//! 事务、`metadata/songs.csv` 同步、正在播的歌、封面和变调缓存这些盘上的东西。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use jp_import::maintain::{
    self, DeleteReport, EditReport, MissingAudio, RelinkSuggestion, SongEdit,
};
use jp_import::metadata_csv;
use serde::{Deserialize, Serialize};

use crate::state::AppState;

/// 维护操作的结果，外加 `metadata/songs.csv` 同步得怎么样
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Maintained<T: Serialize> {
    #[serde(flatten)]
    pub report: T,
    /// updated：改了清单；noFile：没有清单（不建）；unchanged：没有要改的；其余是出错原因（库已经改好了）
    pub csv: String,
}

fn csv_path(state: &AppState) -> PathBuf {
    state
        .db_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("metadata")
        .join("songs.csv")
}

fn describe_csv(result: Result<bool>) -> String {
    match result {
        Ok(true) => "updated".into(),
        Ok(false) => "noFile".into(),
        Err(err) => format!("songs.csv 没同步上：{err:#}"),
    }
}

pub fn edit_song(
    state: &AppState,
    song_id: &str,
    edit: &SongEdit,
) -> Result<Maintained<EditReport>> {
    let (report, row) = {
        let mut corpus = state.corpus();
        let tx = corpus.connection_mut().transaction()?;
        let report = maintain::edit_song(&tx, song_id, edit)?;
        let row = metadata_csv::from_db(&tx, song_id)?;
        tx.commit()?;
        (report, row)
    };
    let csv = match row {
        Some(row) if !report.changed.is_empty() => {
            describe_csv(metadata_csv::upsert(&csv_path(state), &[row]))
        }
        _ => "unchanged".into(),
    };
    Ok(Maintained { report, csv })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    #[serde(flatten)]
    pub report: DeleteReport,
    /// 刮削时下载到 raw/covers 的封面删掉了没有
    pub cover_removed: bool,
    /// 删掉的变调缓存文件数
    pub pitch_cache_removed: usize,
}

pub fn delete_song(state: &AppState, song_id: &str) -> Result<Maintained<DeleteResult>> {
    // 正在放这首：先停，把这一段收听写进历史，再删（历史跟着一起删掉），免得之后又写一条挂在空 id 上
    if let Some(engine) = state.audio()
        && engine.state().song_id == song_id
    {
        state.pitch().stop();
        engine.stop();
    }
    state.flush_session();

    let report = {
        let mut corpus = state.corpus();
        let tx = corpus.connection_mut().transaction()?;
        let report = maintain::delete_song(&tx, song_id)?;
        tx.commit()?;
        report
    };

    // 下一首新歌会复用这个 id，刮削下载封面时「已有图就不重下」会沿用这张旧图，所以删掉。
    // 只删 raw/covers 里、文件名就是这个 id 的——别的路径可能是用户自己的图
    let cover_removed = !report.cover_path.is_empty()
        && remove_app_cover(&state.covers_dir, Path::new(&report.cover_path), song_id);
    let pitch_cache_removed = remove_pitch_cache(state, Path::new(&report.audio_path));
    let csv = describe_csv(metadata_csv::remove(&csv_path(state), &[song_id]));
    Ok(Maintained {
        report: DeleteResult {
            report,
            cover_removed,
            pitch_cache_removed,
        },
        csv,
    })
}

fn remove_app_cover(covers_dir: &Path, cover: &Path, song_id: &str) -> bool {
    let (Ok(dir), Ok(file)) = (
        std::fs::canonicalize(covers_dir),
        std::fs::canonicalize(cover),
    ) else {
        return false;
    };
    let named_by_id = file.file_stem().and_then(|s| s.to_str()) == Some(song_id);
    named_by_id && file.starts_with(&dir) && std::fs::remove_file(&file).is_ok()
}

/// 变调缓存按「路径 + 修改时间 + 大小」命名，原曲还在才算得出来；不在就留着（没有别的歌能用到它）
fn remove_pitch_cache(state: &AppState, audio: &Path) -> usize {
    let cache_dir = state
        .db_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("output")
        .join("pitch_cache");
    (jp_audio::pitch::MIN_SEMITONES..=jp_audio::pitch::MAX_SEMITONES)
        .filter(|&n| n != 0)
        .filter_map(|n| jp_audio::pitch::cache_path(&cache_dir, audio, n).ok())
        .filter(|path| std::fs::remove_file(path).is_ok())
        .count()
}

pub fn missing_audio(state: &AppState) -> Result<Vec<MissingAudio>> {
    maintain::missing_audio(state.corpus().connection())
}

/// 扫一个目录，给丢了音频的歌找新文件。只读，不改库
pub fn suggest_relinks(state: &AppState, folder: &Path) -> Result<Vec<RelinkSuggestion>> {
    if !folder.is_dir() {
        bail!("不是目录：{}", folder.display());
    }
    let (missing, in_use) = {
        let corpus = state.corpus();
        let missing = maintain::missing_audio(corpus.connection())?;
        let mut stmt = corpus
            .connection()
            .prepare("SELECT COALESCE(audio_path,'') FROM songs")?;
        let in_use: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        (missing, in_use)
    };
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(8);
    let scanned = jp_import::scan_dir(folder, threads);
    Ok(maintain::suggest_relinks(&missing, &scanned, &in_use))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkRequest {
    pub song_id: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkResult {
    pub song_id: String,
    /// 为空表示成功
    pub error: String,
    pub duration_sec: Option<f64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkBatch {
    pub results: Vec<RelinkResult>,
    /// 同 `Maintained::csv`
    pub csv: String,
}

/// 逐首重新链接，一首一个事务：一首失败不影响别的
pub fn relink_audio(state: &AppState, links: &[RelinkRequest]) -> Result<RelinkBatch> {
    let mut results = Vec::new();
    let mut rows = Vec::new();
    for link in links {
        let outcome = (|| -> Result<Option<f64>> {
            let path = Path::new(link.path.trim());
            if !path.is_file() {
                bail!("文件不存在：{}", path.display());
            }
            // 读得出来才算能用；时长顺手更新（换了音源时长可能不一样）
            let duration = jp_audio::probe_duration(path)
                .with_context(|| format!("这个文件放不了：{}", path.display()))?;
            let mut corpus = state.corpus();
            let tx = corpus.connection_mut().transaction()?;
            maintain::relink_audio(&tx, &link.song_id, &path.display().to_string())?;
            if let Some(seconds) = duration {
                tx.execute(
                    "UPDATE songs SET duration_sec=?1 WHERE id=?2",
                    rusqlite::params![seconds, link.song_id],
                )?;
            }
            let row = metadata_csv::from_db(&tx, &link.song_id)?;
            tx.commit()?;
            rows.extend(row);
            Ok(duration)
        })();
        results.push(match outcome {
            Ok(duration_sec) => RelinkResult {
                song_id: link.song_id.clone(),
                error: String::new(),
                duration_sec,
            },
            Err(err) => RelinkResult {
                song_id: link.song_id.clone(),
                error: format!("{err:#}"),
                duration_sec: None,
            },
        });
    }
    let csv = if rows.is_empty() {
        "unchanged".into()
    } else {
        describe_csv(metadata_csv::upsert(&csv_path(state), &rows))
    };
    Ok(RelinkBatch { results, csv })
}

/// 导入完把新歌追加进 songs.csv（Python 版加歌时也追加）
pub fn sync_imported(state: &AppState, report: &jp_import::ImportReport) -> String {
    let rows: Vec<_> = {
        let corpus = state.corpus();
        report
            .tracks
            .iter()
            .filter(|t| matches!(t.outcome, jp_import::Outcome::Imported { .. }))
            .filter_map(|t| {
                metadata_csv::from_db(corpus.connection(), &t.song_id)
                    .ok()
                    .flatten()
            })
            .collect()
    };
    if rows.is_empty() {
        return "unchanged".into();
    }
    describe_csv(metadata_csv::upsert(&csv_path(state), &rows))
}
