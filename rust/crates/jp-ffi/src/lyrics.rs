//! 歌词入口。联网操作和长作业使用独立连接，不持有会话数据库锁。
use super::*;
use jp_lyrics::{lyrics, lyric_upgrade, timed_lyrics as timed};

pub(super) fn dispatch(session: &Session, method: &str, args: &Value) -> Result<Option<Value>> {
    let dir = session.library_root().join("raw/lyrics_lrc");
    let v = match method {
        "lyricsSourcesGet" => serde_json::to_value(session.lyric_sources.lock().unwrap_or_else(|e| e.into_inner()).view())?,
        "lyricsSourcesSet" => serde_json::to_value(session.lyric_sources.lock().unwrap_or_else(|e| e.into_inner()).configure(arg(args, "config")?)?)?,
        "lyricsMissing" | "lyricsTimedTargets" => {
            let corpus = session.network_connection()?;
            if method == "lyricsMissing" {
                serde_json::to_value(lyrics::missing(corpus.connection())?)?
            } else {
                serde_json::to_value(timed::targets(corpus.connection())?)?
            }
        }
        "lyricsSaveLines" | "lyricsSaveTiming" | "lyricsRestoreVersion" | "lyricsImportFile" | "lyricsTimedApply" => {
            let id: String = arg(args, "songId")?;
            let note: String = arg_or(args, "note", String::new())?;
            let analyzer = session.analyzer();
            let mut corpus = session.network_connection()?;
            let conn = corpus.connection_mut();
            let attached = match method {
                "lyricsSaveLines" => lyrics::save_edited_lines(conn, &dir, &id, &arg::<Vec<lyrics::EditedLine>>(args, "lines")?, &note, analyzer.as_deref())?,
                "lyricsSaveTiming" => lyrics::save_timed_lines(conn, &dir, &id, &arg::<Vec<lyrics::TimedEditLine>>(args, "lines")?, &note, analyzer.as_deref())?,
                "lyricsRestoreVersion" => lyrics::restore_version(conn, &dir, &id, arg(args, "versionId")?, analyzer.as_deref())?,
                "lyricsImportFile" => lyrics::attach_from_file(conn, &dir, &id, Path::new(&arg::<String>(args, "path")?), analyzer.as_deref())?,
                _ => {
                    let pending = session.lyric_sources.lock().unwrap_or_else(|e| e.into_inner())
                        .candidate(&id, &arg::<String>(args, "candidateId")?)?;
                    timed::apply(conn, &dir, &pending, analyzer.as_deref())?
                }
            };
            session.lyric_sources.lock().unwrap_or_else(|e| e.into_inner()).forget(&id);
            serde_json::to_value(attached)?
        }
        "lyricsRevalidate" => {
            let mut corpus = session.network_connection()?;
            json!(jp_lyrics::management::revalidate(corpus.connection_mut(), arg_or(args, "songId", None)?)?)
        }
        "lyricsTimedSearch" => {
            let id: String = arg(args, "songId")?;
            let before = timed::snapshot(session.network_connection()?.connection(), &id)?;
            let (options, ticket) = session.lyric_sources.lock().unwrap_or_else(|e| e.into_inner()).begin_search(&id);
            serde_json::to_value(jp_lyrics::management::search(&session.lyric_sources, before, options, ticket)?)?
        }
        "lyricsFillOne" => {
            let id: String = arg(args, "songId")?;
            let mut corpus = session.network_connection()?;
            ensure!(lyrics::missing(corpus.connection())?.iter().any(|t| t.song_id == id), "{id} 已经有歌词了，先清掉再补，或者直接导入一份文件覆盖");
            let options = session.lyric_sources.lock().unwrap_or_else(|e| e.into_inner()).options();
            serde_json::to_value(lyric_upgrade::fetch_one(corpus.connection_mut(), &dir, &id, &options, session.analyzer().as_deref())?.map(|(attached, _)| attached))?
        }
        "lyricsFillStart" | "lyricsUpgradeStart" => {
            let corpus = session.network_connection()?;
            let ids: Vec<String> = if method == "lyricsFillStart" {
                let wanted: Option<Vec<String>> = arg_or(args, "songIds", None)?;
                lyrics::missing(corpus.connection())?.into_iter()
                    .filter(|t| wanted.as_ref().is_none_or(|ids| ids.contains(&t.song_id)))
                    .map(|t| t.song_id).collect()
            } else {
                timed::targets(corpus.connection())?.into_iter().map(|t| t.song_id).collect()
            };
            let options = session.lyric_sources.lock().unwrap_or_else(|e| e.into_inner()).options();
            let (db, batch) = (session.db_path.clone(), Arc::clone(&session.lyric_batch));
            let reporter = Arc::clone(&session.lyrics_job);
            session.lyrics_job.spawn("歌词任务", move |job| {
                let progress = lyric_upgrade::run_batch(batch, db, dir, ids, options, job.cancel_flag(), move |p| {
                    reporter.report(p.done as u64, p.total as u64, &p.title);
                })?;
                ensure!(progress.error.is_empty(), "{}", progress.error);
                Ok((serde_json::to_value(&progress)?, progress.cancelled))
            })?;
            Value::Null
        }
        "lyricsFillProgress" | "lyricsUpgradeProgress" => serde_json::to_value(session.lyrics_job.view())?,
        "lyricsUpgradeStatus" => serde_json::to_value(session.lyric_batch.status())?,
        "lyricsFillRunning" => json!(session.lyrics_job.view().running),
        "lyricsFillCancel" | "lyricsUpgradeCancel" => {
            session.lyrics_job.request_cancel();
            Value::Null
        }
        "wordTimings" => {
            let corpus = session.network_connection()?;
            serde_json::to_value(corpus.word_timings(&arg::<Vec<i64>>(args, "utteranceIds")?)?)?
        }
        "wordTimeSet" => {
            let corpus = session.network_connection()?;
            corpus.set_word_time(arg(args, "utteranceId")?, arg(args, "charStart")?, arg(args, "charEnd")?, arg(args, "startSec")?)?;
            Value::Null
        }
        "wordTimeClear" => {
            let corpus = session.network_connection()?;
            corpus.clear_word_time(arg(args, "utteranceId")?, arg(args, "charStart")?)?;
            Value::Null
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}
