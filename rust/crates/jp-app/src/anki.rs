//! Anki 导出的后台作业。
//!
//! 和刮削那边一样的形状：主循环独占数据库连接，进度走事件，可中断。
//! 但**不共用作业标志**——刮削排的是外部服务的限流队列，
//! Anki 是本机，两件事同时做没有冲突。

use std::sync::Arc;

use anyhow::Result;
use jp_anki::{
    AnkiConnect, AnkiError, CardOptions, DupScope, ExportOptions, ExportOutcome, RefreshOutcome,
    RefreshScope,
};
use jp_scraper::UreqTransport;
use jp_scraper::http::HttpClient;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

/// 生产用的 AnkiConnect 客户端。
pub fn client() -> AnkiConnect {
    AnkiConnect::new(HttpClient::new(Box::new(UreqTransport)))
}

/// 一次导出的进度。
#[derive(Debug, Clone, Default, Serialize)]
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

/// Anki 作业的运行/取消状态。四个后台作业共用 `crate::job::Job`——
/// 原来这里自己写了一份，带着「第二次启动会抹掉取消」和
/// 「panic 之后界面永久卡住」两个毛病。
pub type ExportJob = crate::job::Job;

/// 要导的一个词。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportItem {
    pub lemma: String,
    #[serde(default)]
    pub pos: String,
}

/// 后台导一批词。
pub fn spawn_export<R: Runtime>(
    app: AppHandle<R>,
    job: Arc<ExportJob>,
    db_path: std::path::PathBuf,
    items: Vec<ExportItem>,
    options: ExportOptions,
) -> Result<()> {
    // 线程崩了也要让界面收一条终态（见 `crate::job`）
    let crashed = app.clone();
    let guard = job
        .start_with(move || {
            let _ = crashed.emit(
                "anki://progress",
                ExportProgress {
                    finished: true,
                    error: "Anki 后台线程崩了，详情见日志".into(),
                    job: "export".into(),
                    ..Default::default()
                },
            );
        })
        .ok_or_else(|| anyhow::anyhow!("已经有一个导出作业在跑"))?;

    std::thread::spawn(move || {
        let total = items.len();
        let mut error = String::new();
        let mut cancelled = false;

        let outcome = (|| -> Result<()> {
            // 自己开一条连接，不占 UI 那条
            let conn = rusqlite::Connection::open_with_flags(
                &db_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let anki = client();
            // note type 先就位，否则第一张卡就会失败
            jp_anki::export::prepare(&anki)?;
            // 牌组不存在就别开始：否则几百个词各报一次同样的错。和 Python 导出前的检查一致
            if let Some(message) = jp_anki::missing_deck_message(&anki, &options.deck)? {
                error = message;
                return Ok(());
            }

            for (index, item) in items.iter().enumerate() {
                if job.cancelled() {
                    cancelled = true;
                    return Ok(());
                }
                let result = jp_anki::export_word(&anki, &conn, &item.lemma, &item.pos, &options);
                let (outcome, new_sentences, message) = match result {
                    Ok(ExportOutcome::Added) => ("added", 0, String::new()),
                    Ok(ExportOutcome::Updated { new_sentences }) => {
                        ("updated", new_sentences, String::new())
                    }
                    Ok(ExportOutcome::AlreadyComplete) => ("alreadyComplete", 0, String::new()),
                    Ok(ExportOutcome::Skipped) => {
                        ("skipped", 0, "Anki 里已有，按设置跳过".to_string())
                    }
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
                let _ = app.emit(
                    "anki://progress",
                    ExportProgress {
                        done: index + 1,
                        total,
                        lemma: item.lemma.clone(),
                        outcome: outcome.to_string(),
                        new_sentences,
                        message,
                        job: "export".into(),
                        ..Default::default()
                    },
                );
            }
            Ok(())
        })();

        if let Err(err) = outcome {
            error = format!("{err:#}");
        }
        // 先放开运行权再发终态，顺序和原来的 `job.finish()` 一致
        drop(guard);
        let _ = app.emit(
            "anki://progress",
            ExportProgress {
                done: total,
                total,
                finished: true,
                cancelled,
                error,
                job: "export".into(),
                ..Default::default()
            },
        );
    });
    Ok(())
}

fn emit<R: Runtime>(app: &AppHandle<R>, progress: ExportProgress) {
    let _ = app.emit("anki://progress", progress);
}

fn describe_refresh(result: RefreshOutcome) -> (String, String) {
    match result {
        RefreshOutcome::Refreshed => ("refreshed".into(), String::new()),
        RefreshOutcome::NotFound => ("notFound".into(), "Anki 里没有这个词的卡".into()),
        RefreshOutcome::Failed { error } => ("failed".into(), error),
    }
}

/// 后台更新选中的词：只重查读音、释义、JLPT、音高、词频、词性，不动例句、音频和出处。
pub fn spawn_update<R: Runtime>(
    app: AppHandle<R>,
    job: Arc<ExportJob>,
    db_path: std::path::PathBuf,
    items: Vec<ExportItem>,
    deck: String,
    scope: DupScope,
) -> Result<()> {
    // 线程崩了也要让界面收一条终态（见 `crate::job`）
    let crashed = app.clone();
    let guard = job
        .start_with(move || {
            let _ = crashed.emit(
                "anki://progress",
                ExportProgress {
                    finished: true,
                    error: "Anki 后台线程崩了，详情见日志".into(),
                    job: "update".into(),
                    ..Default::default()
                },
            );
        })
        .ok_or_else(|| anyhow::anyhow!("已经有一个 Anki 作业在跑"))?;

    std::thread::spawn(move || {
        let total = items.len();
        let mut error = String::new();
        let mut cancelled = false;

        let outcome = (|| -> Result<()> {
            let conn = rusqlite::Connection::open_with_flags(
                &db_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let anki = client();
            // 顺带把模板和 CSS 更新到最新，和 Python 一致
            jp_anki::export::prepare(&anki)?;
            if let Some(message) = jp_anki::missing_deck_message(&anki, &deck)? {
                error = message;
                return Ok(());
            }
            for (index, item) in items.iter().enumerate() {
                if job.cancelled() {
                    cancelled = true;
                    return Ok(());
                }
                let (outcome, message) = match jp_anki::update_word(
                    &anki,
                    &conn,
                    &item.lemma,
                    &item.pos,
                    &deck,
                    scope,
                ) {
                    Ok(result) => describe_refresh(result),
                    // Anki 挂了要停整批
                    Err(err) => {
                        error = err.advice();
                        return Ok(());
                    }
                };
                emit(
                    &app,
                    ExportProgress {
                        job: "update".into(),
                        done: index + 1,
                        total,
                        lemma: item.lemma.clone(),
                        outcome,
                        message,
                        ..Default::default()
                    },
                );
            }
            Ok(())
        })();

        if let Err(err) = outcome {
            error = format!("{err:#}");
        }
        // 先放开运行权再发终态，顺序和原来的 `job.finish()` 一致
        drop(guard);
        emit(
            &app,
            ExportProgress {
                job: "update".into(),
                done: total,
                total,
                finished: true,
                cancelled,
                error,
                ..Default::default()
            },
        );
    });
    Ok(())
}

/// 刷新之前先报个数：范围内有几张 JPOP Corpus 旧卡。只读。
pub fn refresh_preview(deck: &str, scope: RefreshScope) -> Result<usize> {
    let anki = client();
    jp_anki::refresh_targets(&anki, deck, scope)
        .map(|targets| targets.len())
        .map_err(|err| anyhow::anyhow!("{}", err.advice()))
}

/// 后台刷新旧牌组：范围内每张 JPOP Corpus 卡重查词典字段，不动例句、音频和出处。
pub fn spawn_refresh<R: Runtime>(
    app: AppHandle<R>,
    job: Arc<ExportJob>,
    db_path: std::path::PathBuf,
    deck: String,
    scope: RefreshScope,
) -> Result<()> {
    // 线程崩了也要让界面收一条终态（见 `crate::job`）
    let crashed = app.clone();
    let guard = job
        .start_with(move || {
            let _ = crashed.emit(
                "anki://progress",
                ExportProgress {
                    finished: true,
                    error: "Anki 后台线程崩了，详情见日志".into(),
                    job: "refresh".into(),
                    ..Default::default()
                },
            );
        })
        .ok_or_else(|| anyhow::anyhow!("已经有一个 Anki 作业在跑"))?;

    std::thread::spawn(move || {
        let mut total = 0;
        let mut error = String::new();
        let mut cancelled = false;

        let outcome = (|| -> Result<()> {
            let conn = rusqlite::Connection::open_with_flags(
                &db_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let anki = client();
            jp_anki::export::prepare(&anki)?;
            if let Some(message) = jp_anki::missing_deck_message(&anki, &deck)? {
                error = message;
                return Ok(());
            }
            let targets = match jp_anki::refresh_targets(&anki, &deck, scope) {
                Ok(targets) => targets,
                Err(err) => {
                    error = err.advice();
                    return Ok(());
                }
            };
            total = targets.len();
            for (index, target) in targets.iter().enumerate() {
                if job.cancelled() {
                    cancelled = true;
                    return Ok(());
                }
                let (outcome, message) = match jp_anki::refresh_target(&anki, &conn, target) {
                    Ok(result) => describe_refresh(result),
                    Err(err) => {
                        error = err.advice();
                        return Ok(());
                    }
                };
                emit(
                    &app,
                    ExportProgress {
                        job: "refresh".into(),
                        done: index + 1,
                        total,
                        lemma: target.expression.clone(),
                        outcome,
                        message,
                        ..Default::default()
                    },
                );
            }
            Ok(())
        })();

        if let Err(err) = outcome {
            error = format!("{err:#}");
        }
        // 先放开运行权再发终态，顺序和原来的 `job.finish()` 一致
        drop(guard);
        emit(
            &app,
            ExportProgress {
                job: "refresh".into(),
                done: total,
                total,
                finished: true,
                cancelled,
                error,
                ..Default::default()
            },
        );
    });
    Ok(())
}

/// 界面一进来就问的那几件事。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnkiStatus {
    /// Anki 开着而且装了 AnkiConnect
    pub connected: bool,
    /// 连不上时的处置建议
    pub message: String,
    pub version: u32,
    pub decks: Vec<String>,
    /// 「JPOP Corpus」这个笔记类型在不在
    pub note_type_ready: bool,
    /// 读到的 collection 路径。空表示没找到。
    pub collection_path: String,
    /// 已经进过 Anki 的词数
    pub known_words: usize,
    /// 其中复习过的
    pub studied_words: usize,
}

/// 问一遍 Anki 的状态。**连不上不是错误**——用户可能还没开 Anki，
/// 界面该显示指引而不是红色报错。
pub fn status() -> AnkiStatus {
    let anki = client();
    let mut out = AnkiStatus::default();
    match anki.ping() {
        Ok(version) => {
            out.connected = true;
            out.version = version;
        }
        Err(err) => {
            out.message = err.advice();
            // 连不上也把学习状态读出来：那是直接读文件，不需要 Anki 开着
            fill_learning(&mut out);
            return out;
        }
    }
    out.decks = anki.deck_names().unwrap_or_default();
    out.note_type_ready = anki
        .model_names()
        .map(|names| names.iter().any(|n| n == jp_anki::NOTE_TYPE))
        .unwrap_or(false);
    fill_learning(&mut out);
    out
}

fn fill_learning(out: &mut AnkiStatus) {
    // 找不到 collection 很正常（没装 Anki、或者装在别处），不当错误
    if let Ok(state) = jp_anki::learning::load(None, "") {
        out.collection_path = state.collection_path.clone();
        out.known_words = state.words.len();
        out.studied_words = state.studied_count();
    }
}

/// 组装导出选项。
pub fn options(deck: String, max_examples: usize, max_dicts: Option<usize>) -> ExportOptions {
    ExportOptions {
        deck,
        card: CardOptions {
            max_examples: max_examples.clamp(1, 8),
            max_dicts,
        },
        ..Default::default()
    }
}

/// 把 AnkiConnect 的错误转成一句给用户的话。
pub fn describe(err: &AnkiError) -> String {
    err.advice()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_count_is_clamped_to_something_sane() {
        // 0 条例句的卡片没有意义；几十条会让卡片没法看
        assert_eq!(options("D".into(), 0, None).card.max_examples, 1);
        assert_eq!(options("D".into(), 99, None).card.max_examples, 8);
        assert_eq!(options("D".into(), 3, None).card.max_examples, 3);
    }
}
