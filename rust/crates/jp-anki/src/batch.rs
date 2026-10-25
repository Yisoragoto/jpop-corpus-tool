//! 两端共用的批量循环与事件字段。
use crate::{DupScope, ExportOptions, ExportOutcome, RefreshOutcome, RefreshScope};
use anyhow::Result;
use serde::Serialize;
/// 一次导出的进度。
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgress {
    /// 哪个作业：export / update / refresh
    pub job: String,
    pub done: usize,
    pub total: usize,
    pub lemma: String,
    /// added / updated / alreadyComplete / noExamples / failed
    pub outcome: String,
    /// 追加了几段例句（只有 updated 时有意义）
    pub new_sentences: usize,
    pub message: String,
    pub finished: bool,
    pub cancelled: bool,
    /// 作业**整体**中断的原因（比如 Anki 中途关了）。为空表示正常结束。
    ///
    /// 和上面的 `message` 分开：那个是**单个词**的结果，这个是整批黄了。
    /// 四个后台作业统一用 `error` 这个名字（刮削、词典、补齐歌词同）。
    pub error: String,
}

/// 要导的一个词。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportItem {
    pub lemma: String,
    #[serde(default)]
    pub pos: String,
}

pub fn run_export(
    db_path: &std::path::Path,
    client: &crate::AnkiConnect,
    items: &[ExportItem],
    options: &ExportOptions,
    is_cancelled: impl Fn() -> bool,
    mut emit: impl FnMut(ExportProgress),
) -> ExportProgress {
    if is_cancelled() {
        return ExportProgress {
            job: "export".into(),
            total: items.len(),
            done: items.len(),
            finished: true,
            cancelled: true,
            ..Default::default()
        };
    }
    let total = items.len();
    let mut error = String::new();
    let mut cancelled = false;

    let outcome = (|| -> Result<()> {
        // 自己开一条连接，不占 UI 那条
        let conn = rusqlite::Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let anki = client;
        // note type 先就位，否则第一张卡就会失败
        crate::export::prepare(anki)?;
        // 牌组不存在就别开始：否则几百个词各报一次同样的错。和 Python 导出前的检查一致
        if let Some(message) = crate::missing_deck_message(anki, &options.deck)? {
            error = message;
            return Ok(());
        }

        for (index, item) in items.iter().enumerate() {
            if is_cancelled() {
                cancelled = true;
                return Ok(());
            }
            let result = crate::export_word(anki, &conn, &item.lemma, &item.pos, options);
            let (outcome, new_sentences, message) = match result {
                Ok(ExportOutcome::Added) => ("added", 0, String::new()),
                Ok(ExportOutcome::Updated { new_sentences }) => {
                    ("updated", new_sentences, String::new())
                }
                Ok(ExportOutcome::AlreadyComplete) => ("alreadyComplete", 0, String::new()),
                Ok(ExportOutcome::Skipped) => ("skipped", 0, "Anki 里已有，按设置跳过".to_string()),
                Ok(ExportOutcome::NoExamples) => {
                    ("noExamples", 0, "语料里没有这个词的例句".to_string())
                }
                Ok(ExportOutcome::Failed { error }) => ("failed", 0, error),
                // **Anki 挂了要停整批**，而不是让 500 个词各报一次
                Err(err) => {
                    error = err.advice();
                    return Ok(());
                }
            };
            emit(ExportProgress {
                done: index + 1,
                total,
                lemma: item.lemma.clone(),
                outcome: outcome.to_string(),
                new_sentences,
                message,
                job: "export".into(),
                ..Default::default()
            });
        }
        Ok(())
    })();

    if let Err(err) = outcome {
        error = format!("{err:#}");
    }

    ExportProgress {
        done: total,
        total,
        finished: true,
        cancelled,
        error,
        job: "export".into(),
        ..Default::default()
    }
}

pub fn run_update(
    db_path: &std::path::Path,
    client: &crate::AnkiConnect,
    items: &[ExportItem],
    deck: &str,
    scope: DupScope,
    is_cancelled: impl Fn() -> bool,
    mut emit: impl FnMut(ExportProgress),
) -> ExportProgress {
    if is_cancelled() {
        return ExportProgress {
            job: "update".into(),
            total: items.len(),
            done: items.len(),
            finished: true,
            cancelled: true,
            ..Default::default()
        };
    }
    let total = items.len();
    let mut error = String::new();
    let mut cancelled = false;

    let outcome = (|| -> Result<()> {
        let conn = rusqlite::Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let anki = client;
        // 顺带把模板和 CSS 更新到最新，和 Python 一致
        crate::export::prepare(anki)?;
        if let Some(message) = crate::missing_deck_message(anki, deck)? {
            error = message;
            return Ok(());
        }
        for (index, item) in items.iter().enumerate() {
            if is_cancelled() {
                cancelled = true;
                return Ok(());
            }
            let (outcome, message) =
                match crate::update_word(anki, &conn, &item.lemma, &item.pos, deck, scope) {
                    Ok(result) => describe_refresh(result),
                    // Anki 挂了要停整批
                    Err(err) => {
                        error = err.advice();
                        return Ok(());
                    }
                };
            emit(ExportProgress {
                job: "update".into(),
                done: index + 1,
                total,
                lemma: item.lemma.clone(),
                outcome,
                message,
                ..Default::default()
            });
        }
        Ok(())
    })();

    if let Err(err) = outcome {
        error = format!("{err:#}");
    }

    ExportProgress {
        job: "update".into(),
        done: total,
        total,
        finished: true,
        cancelled,
        error,
        ..Default::default()
    }
}

pub fn run_refresh(
    db_path: &std::path::Path,
    client: &crate::AnkiConnect,
    deck: &str,
    scope: RefreshScope,
    is_cancelled: impl Fn() -> bool,
    mut emit: impl FnMut(ExportProgress),
) -> ExportProgress {
    if is_cancelled() {
        return ExportProgress {
            job: "refresh".into(),
            finished: true,
            cancelled: true,
            ..Default::default()
        };
    }
    let mut total = 0;
    let mut error = String::new();
    let mut cancelled = false;

    let outcome = (|| -> Result<()> {
        let conn = rusqlite::Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let anki = client;
        crate::export::prepare(anki)?;
        if let Some(message) = crate::missing_deck_message(anki, deck)? {
            error = message;
            return Ok(());
        }
        let targets = match crate::refresh_targets(anki, deck, scope) {
            Ok(targets) => targets,
            Err(err) => {
                error = err.advice();
                return Ok(());
            }
        };
        total = targets.len();
        for (index, target) in targets.iter().enumerate() {
            if is_cancelled() {
                cancelled = true;
                return Ok(());
            }
            let (outcome, message) = match crate::refresh_target(anki, &conn, target) {
                Ok(result) => describe_refresh(result),
                Err(err) => {
                    error = err.advice();
                    return Ok(());
                }
            };
            emit(ExportProgress {
                job: "refresh".into(),
                done: index + 1,
                total,
                lemma: target.expression.clone(),
                outcome,
                message,
                ..Default::default()
            });
        }
        Ok(())
    })();

    if let Err(err) = outcome {
        error = format!("{err:#}");
    }

    ExportProgress {
        job: "refresh".into(),
        done: total,
        total,
        finished: true,
        cancelled,
        error,
        ..Default::default()
    }
}

fn describe_refresh(result: RefreshOutcome) -> (String, String) {
    match result {
        RefreshOutcome::Refreshed => ("refreshed".into(), String::new()),
        RefreshOutcome::NotFound => ("notFound".into(), "Anki 里没有这个词的卡".into()),
        RefreshOutcome::Failed { error } => ("failed".into(), error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::testing::{FakeAnki, client};
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    fn fixture(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("jp-anki-batch-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = jp_corpus::fixture::write_to(&dir).unwrap();
        (db, dir)
    }

    #[test]
    fn export_and_update_cancel_between_words_and_keep_windows_payloads() {
        let (db, dir) = fixture("words");
        for update in [false, true] {
            let fake = Arc::new(FakeAnki::new(&[
                ("modelNames", json!([])),
                ("deckNames", json!(["D"])),
                ("findNotes", json!([])),
            ]));
            let anki = client(fake);
            let items = [
                ExportItem {
                    lemma: "不存在的词".into(),
                    pos: "NOUN".into(),
                },
                ExportItem {
                    lemma: "另一个词".into(),
                    pos: "NOUN".into(),
                },
            ];
            let cancel = AtomicBool::new(false);
            let mut log = Vec::new();
            let emit = |p| {
                log.push(p);
                cancel.store(true, Ordering::SeqCst);
            };
            let final_progress = if update {
                run_update(
                    &db,
                    &anki,
                    &items,
                    "D",
                    DupScope::Deck,
                    || cancel.load(Ordering::SeqCst),
                    emit,
                )
            } else {
                run_export(
                    &db,
                    &anki,
                    &items,
                    &ExportOptions {
                        deck: "D".into(),
                        ..Default::default()
                    },
                    || cancel.load(Ordering::SeqCst),
                    emit,
                )
            };
            assert!(
                final_progress.finished
                    && final_progress.cancelled
                    && final_progress.error.is_empty()
            );
            assert_eq!((final_progress.done, final_progress.total), (2, 2));
            assert_eq!(log.len(), 1);
            assert_eq!(log[0].lemma, "不存在的词");
            assert_eq!(
                log[0].outcome,
                if update { "notFound" } else { "noExamples" }
            );
            assert!(!log[0].finished);
            let payload = serde_json::to_value(&log[0]).unwrap();
            for field in [
                "job",
                "done",
                "total",
                "lemma",
                "outcome",
                "newSentences",
                "message",
                "finished",
                "cancelled",
                "error",
            ] {
                assert!(payload.get(field).is_some(), "{field}");
            }
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn refresh_cancel_before_start_never_uses_transport_or_database() {
        let fake = Arc::new(FakeAnki::offline());
        let p = run_refresh(
            std::path::Path::new("missing.db"),
            &client(fake.clone()),
            "D",
            RefreshScope::Deck,
            || true,
            |_| panic!("不应发逐词进度"),
        );
        assert!(p.finished && p.cancelled && p.error.is_empty());
        assert_eq!(p.job, "refresh");
        assert!(fake.actions().is_empty());
    }

    #[test]
    fn missing_deck_and_offline_transport_finish_the_whole_batch() {
        let (db, dir) = fixture("errors");
        let fake = Arc::new(FakeAnki::new(&[
            ("modelNames", json!([])),
            ("deckNames", json!(["Other"])),
        ]));
        let p = run_export(
            &db,
            &client(fake),
            &[],
            &ExportOptions {
                deck: "D".into(),
                ..Default::default()
            },
            || false,
            |_| panic!("牌组不存在应立即停"),
        );
        assert!(p.finished && !p.cancelled && p.error.contains("牌组"));
        let p = run_update(
            &db,
            &client(Arc::new(FakeAnki::offline())),
            &[],
            "D",
            DupScope::Deck,
            || false,
            |_| panic!("离线应立即停"),
        );
        assert!(p.finished && p.error.contains("连不上 Anki"));
        std::fs::remove_dir_all(dir).ok();
    }
}
