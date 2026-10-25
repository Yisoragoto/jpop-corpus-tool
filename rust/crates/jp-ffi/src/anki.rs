//! Anki 的轮询适配；业务循环与 Tauri 共用。
use super::*;
use jp_anki::{batch, service};

pub(super) fn dispatch(s: &Session, method: &str, args: &Value) -> Result<Option<Value>> {
    if !method.starts_with("anki") && !matches!(method, "exportText" | "exportPng") {
        return Ok(None);
    }
    dispatch_with_client(s, method, args, &jp_anki::AnkiConnect::local())
}

pub(super) fn dispatch_with_client(
    s: &Session,
    method: &str,
    args: &Value,
    client: &jp_anki::AnkiConnect,
) -> Result<Option<Value>> {
    let v = match method {
        "ankiStatus" => json!(service::status(client)),
        "exportText" => {
            jp_corpus::export::text(arg(args, "path")?, arg(args, "content")?)?;
            Value::Null
        }
        "exportPng" => {
            jp_corpus::export::png(arg(args, "path")?, arg(args, "bytes")?)?;
            Value::Null
        }
        "ankiNotesSince" => json!(service::notes_since(arg(args, "sinceMs")?)?),
        "ankiWords" => {
            let defaults = jp_anki::PickOptions::default();
            let options = jp_anki::PickOptions {
                pos: arg_or(args, "pos", defaults.pos)?,
                min_count: arg_or(args, "minCount", defaults.min_count)?,
                song_ids: arg_or(args, "songIds", Vec::new())?,
                limit: arg_or(args, "limit", defaults.limit)?,
            };
            json!(service::words(
                s.network_connection()?.connection(),
                &options,
                arg_or(args, "skipStudied", false)?
            )?)
        }
        "ankiPreview" => json!(service::preview(
            s.network_connection()?.connection(),
            &arg::<String>(args, "lemma")?,
            &arg_or(args, "pos", String::new())?,
            arg_or(args, "maxExamples", 2)?,
            arg_or(args, "maxDicts", None)?
        )?),
        "ankiEnsureNoteType" => json!(jp_anki::export::prepare(client)?),
        "ankiModelNames" => json!(client.model_names().unwrap_or_default()),
        "ankiBrowseNotes" => {
            service::browse_notes(client, &arg::<Vec<i64>>(args, "noteIds")?)?;
            Value::Null
        }
        "ankiRefreshPreview" => json!(
            jp_anki::refresh_targets(
                client,
                &arg::<String>(args, "deck")?,
                arg_or(args, "scope", jp_anki::RefreshScope::default())?
            )?
            .len()
        ),
        "ankiMiningReport" => {
            let collection = jp_anki::learning::find_collection().ok_or_else(|| {
                anyhow!("找不到 Anki 的 collection.anki2：装了 Anki 并且至少打开过一次吗？")
            })?;
            let levels = jp_anki::report::jlpt_levels(s.network_connection()?.connection())?;
            let mut result = jp_anki::report::generate(
                &s.library_root(),
                &collection,
                &levels,
                &arg::<String>(args, "now")?,
                &arg_or(args, "deckContains", String::new())?,
            )?;
            if let Err(err) = jp_anki::report::open_in_browser(Path::new(&result.path)) {
                result.open_error = format!("{err:#}");
            }
            json!(result)
        }
        "ankiCancel" => {
            s.anki_job.request_cancel();
            Value::Null
        }
        "ankiIsRunning" => json!(s.anki_job.view().running),
        "ankiProgress" => json!(s.anki_job.view()),
        "ankiExportStart" | "ankiUpdateStart" | "ankiRefreshStart" => {
            let words: Vec<batch::ExportItem> = arg_or(args, "words", Vec::new())?;
            let deck: String = arg(args, "deck")?;
            if method == "ankiRefreshStart" {
                service::validate_deck(&deck)?;
            } else {
                service::validate_selection(&words, &deck)?;
            }
            let mut options = service::options(
                deck.clone(),
                arg_or(args, "maxExamples", 2)?,
                arg_or(args, "maxDicts", None)?,
            );
            options.dup_mode = arg_or(args, "dupMode", jp_anki::DupMode::default())?;
            options.dup_scope = arg_or(args, "dupScope", jp_anki::DupScope::default())?;
            options.clip_audio = arg_or(args, "clipAudio", true)?;
            let scope = arg_or(args, "scope", jp_anki::RefreshScope::default())?;
            let refresh = method == "ankiRefreshStart";
            let method = method.to_owned();
            let total = words.len();
            let db = s.db_path.clone();
            s.anki_job.spawn("Anki 作业", move |job| {
                let client = jp_anki::AnkiConnect::local();
                let mut log = Vec::new();
                let mut emit = |p: batch::ExportProgress| {
                    if !p.finished && !p.lemma.is_empty() {
                        log.insert(0, p.clone());
                        log.truncate(200);
                    }
                    job.report(p.done as u64, p.total as u64, &p.lemma);
                    job.report_result(json!({"progress": p, "log": log}));
                };
                let p = match method.as_str() {
                    "ankiExportStart" => batch::run_export(
                        &db,
                        &client,
                        &words,
                        &options,
                        || job.cancelled(),
                        &mut emit,
                    ),
                    "ankiUpdateStart" => batch::run_update(
                        &db,
                        &client,
                        &words,
                        &deck,
                        options.dup_scope,
                        || job.cancelled(),
                        &mut emit,
                    ),
                    _ => batch::run_refresh(
                        &db,
                        &client,
                        &deck,
                        scope,
                        || job.cancelled(),
                        &mut emit,
                    ),
                };
                job.report(p.done as u64, p.total as u64, "完成");
                Ok((json!({"progress": p, "log": log}), p.cancelled))
            })?;
            if refresh { Value::Null } else { json!(total) }
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}
