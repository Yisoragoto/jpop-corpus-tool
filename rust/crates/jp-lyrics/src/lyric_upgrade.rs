//! One serial, cancellable queue for imported songs and library-wide upgrades.
//! Network calls never hold the UI database lock. All writes still use the
//! fingerprint-checked, transactional timed-lyrics replacement path.
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use jp_scraper::timed_sources::{SearchOutput, SourceOptions, TimedFound};
use rusqlite::Connection;
use serde::Serialize;

use crate::{lyrics::Attached, timed_lyrics as timed};

/// Existing requests retain the source timeout; after Stop, no additional
/// provider, fallback, guest-session or download requests can be issued.
///
/// It also spaces requests to the same host by [`HOST_GAP`]: a whole-library run
/// asks several unofficial endpoints per song back to back, and the old NetEase-only
/// fill was throttled the same way. Queueing is better than getting blocked.
struct CancellableTransport {
    inner: Box<dyn jp_scraper::Transport>,
    cancelled: Arc<AtomicBool>,
    last: Arc<Mutex<HashMap<String, Instant>>>,
}

const HOST_GAP: Duration = Duration::from_millis(600);

fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

impl CancellableTransport {
    /// Waits until this host may be asked again; false when Stop was pressed meanwhile.
    fn wait_turn(&self, url: &str) -> bool {
        let key = host(url).to_ascii_lowercase();
        loop {
            if self.cancelled.load(Ordering::SeqCst) {
                return false;
            }
            let wait = {
                let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
                let now = Instant::now();
                match last.get(&key).map(|t| HOST_GAP.saturating_sub(now - *t)) {
                    Some(left) if !left.is_zero() => left,
                    _ => {
                        last.insert(key, now);
                        return true;
                    }
                }
            };
            std::thread::sleep(wait.min(Duration::from_millis(100)));
        }
    }
}
impl jp_scraper::Transport for CancellableTransport {
    fn get(
        &self,
        url: &str,
        headers: &[(String, String)],
        timeouts: jp_scraper::http::Timeouts,
    ) -> std::result::Result<jp_scraper::http::HttpResponse, jp_scraper::ProviderError> {
        if !self.wait_turn(url) {
            return Err(jp_scraper::ProviderError::unavailable("cancelled"));
        }
        self.inner.get(url, headers, timeouts)
    }
    fn post(
        &self,
        url: &str,
        body: &[u8],
        headers: &[(String, String)],
        timeouts: jp_scraper::http::Timeouts,
    ) -> std::result::Result<jp_scraper::http::HttpResponse, jp_scraper::ProviderError> {
        if !self.wait_turn(url) {
            return Err(jp_scraper::ProviderError::unavailable("cancelled"));
        }
        self.inner.post(url, body, headers, timeouts)
    }
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub running: bool,
    pub cancelled: bool,
    pub total: usize,
    pub done: usize,
    pub upgraded: usize,
    pub line_filled: usize,
    pub skipped: usize,
    pub not_found: usize,
    pub failed: usize,
    pub title: String,
    pub error: String,
    pub results: Vec<Item>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub song_id: String,
    pub title: String,
    pub status: String,
    pub source: String,
    pub message: String,
}

#[derive(Default)]
struct Queue {
    pending: VecDeque<String>,
    seen: HashSet<String>,
    progress: Progress,
}

#[derive(Default)]
pub struct Batch {
    gate: Arc<crate::job::Job>,
    queue: Mutex<Queue>,
}

impl Batch {
    pub fn status(&self) -> Progress {
        self.queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .progress
            .clone()
    }
    pub fn cancel(&self) {
        self.gate.request_cancel();
        self.queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .progress
            .cancelled = true;
    }
}

pub fn protected(conn: &Connection, song_id: &str) -> Result<bool> {
    // 你在「歌词来源」里选定的（包括手动挑的在线候选）也不让自动流程换
    if jp_corpus::lyric_sources::selection(conn, song_id)?.is_some_and(|s| s.chosen_by == "user") {
        return Ok(true);
    }
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM lyric_versions WHERE song_id=?1 AND is_current=1 AND source_type IN ('manual_file','manual_edit','restored')) OR EXISTS(SELECT 1 FROM word_timings w JOIN utterances u ON u.id=w.utterance_id WHERE u.song_id=?1 AND w.source='manual')", [song_id], |r| r.get(0))?)
}

fn same_identity(before: &timed::Snapshot, found: &TimedFound) -> bool {
    !before.artist.trim().is_empty()
        && before.duration.is_some_and(|d| d.is_finite() && d > 0.0)
        && jp_normalize::matching_key(&before.title) == jp_normalize::matching_key(&found.title)
        && jp_normalize::matching_key(&before.artist) == jp_normalize::matching_key(&found.artist)
}

/// Count the units the player will actually accept after canonical import,
/// rather than treating a filename or a provider's word count as proof.
fn precision(before: &timed::Snapshot, found: &TimedFound) -> (usize, usize) {
    let parsed = jp_import::lrc::parse(&timed::canonical(found));
    let lines = parsed
        .lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let row = jp_corpus::LyricLine {
                utterance_id: 0,
                line_idx: i as i64,
                text: l.text.clone(),
                time_sec: l.time_sec,
                tokens: vec![],
            };
            // 入库时会先 normalize（空格标点并进相邻单位），这里按入库后的样子数，
            // 否则带空格的候选（英文词、全角空格）会被低估、落选。
            let segments: Vec<_> = l
                .words
                .iter()
                .map(|w| jp_corpus::word_timing::Segment {
                    start: w.start,
                    end: w.end,
                    time_sec: w.time_sec,
                    end_sec: w.end_sec,
                })
                .collect();
            let marks: Vec<_> = jp_corpus::word_timing::normalize(&l.text, &segments)
                .into_iter()
                .map(|w| jp_corpus::word_timing::WordTiming {
                    utterance_id: 0,
                    char_start: w.start as i64,
                    char_end: w.end as i64,
                    start_sec: w.time_sec,
                    end_sec: w.end_sec,
                    source: "lrc".into(),
                })
                .collect();
            let next = parsed.lines[i + 1..]
                .iter()
                .filter_map(|l| l.time_sec)
                .find(|t| l.time_sec.is_none_or(|s| *t > s));
            let end = match (next, before.duration) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            jp_corpus::display_timeline::for_line(&row, &marks, end).mode
                == jp_corpus::display_timeline::DisplayMode::Karaoke
        })
        .filter(|v| *v)
        .count();
    (lines, parsed.lines.len())
}

/// High precision wins regardless of provider ordering. An upgrade must improve
/// actual coverage and not discard already useful native timing.
fn choose(before: &timed::Snapshot, found: &SearchOutput) -> Option<(TimedFound, bool)> {
    let mut candidates: Vec<_> = found
        .candidates
        .iter()
        .filter(|c| {
            same_identity(before, c)
                && timed::compatible(before, c)
                && timed::agrees_for_automatic(before, c)
        })
        .map(|c| (c, precision(before, c)))
        .filter(|(_, (valid, total))| {
            *valid > before.timed_lines() && *total > 0 && *valid * 2 >= *total
        })
        .collect();
    candidates.sort_by(|a, b| {
        (b.1.0 * a.1.1)
            .cmp(&(a.1.0 * b.1.1))
            .then_with(|| b.1.0.cmp(&a.1.0))
    });
    if let Some((candidate, _)) = candidates.first() {
        return Some(((*candidate).clone(), true));
    }
    // Existing lyrics are never replaced with a lower precision fallback.
    if before.line_count() > 0 {
        return None;
    }
    found
        .fallback
        .iter()
        .cloned()
        .filter_map(timed::from_basic)
        .filter(|c| {
            same_identity(before, c)
                && timed::compatible(before, c)
                && timed::agrees_for_automatic(before, c)
        })
        .max_by_key(|c| c.format.eq_ignore_ascii_case("LRC"))
        .map(|c| (c, false))
}

/// Why nothing was adopted although candidates came back: the check that the
/// closest candidate failed. Shown next to 「待逐首复核」 so the user knows what
/// to look at before adopting one by hand.
fn review_reason(before: &timed::Snapshot, found: &SearchOutput) -> &'static str {
    if before.artist.trim().is_empty() || !before.duration.is_some_and(|d| d.is_finite() && d > 0.0) {
        return "这首歌缺少歌手或时长，无法自动确认是同一首";
    }
    let stage = |c: &TimedFound| {
        if !same_identity(before, c) {
            0
        } else if !timed::compatible(before, c) {
            1
        } else if !timed::agrees_for_automatic(before, c) {
            2
        } else {
            3
        }
    };
    match found.candidates.iter().map(stage).max() {
        Some(3) => "候选能逐字点亮的行不比现在多，或不到一半",
        Some(2) => "候选正文和现有歌词不完全一致（写法、标点或段落不同），自动换会改动原文",
        Some(1) => "候选的时长或正文和现有歌词差别太大",
        Some(_) => "候选的歌名或歌手和这首歌对不上",
        None => "只找到逐行歌词；已有歌词不会被逐行版本替换",
    }
}

pub fn apply_best(
    conn: &mut Connection,
    dir: &Path,
    before: &timed::Snapshot,
    found: &SearchOutput,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Option<(Attached, bool)>> {
    if protected(conn, &before.song_id)? {
        return Ok(None);
    }
    let Some((candidate, high)) = choose(before, found) else {
        return Ok(None);
    };
    let pending = timed::automatic_pending(before, candidate);
    Ok(Some((timed::apply(conn, dir, &pending, analyzer)?, high)))
}

fn local_fallback(
    conn: &mut Connection,
    dir: &Path,
    before: &timed::Snapshot,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Option<(Attached, bool)>> {
    if before.line_count() > 0 {
        return Ok(None);
    }
    let audio: String = conn.query_row(
        "SELECT COALESCE(audio_path,'') FROM songs WHERE id=?1",
        [&before.song_id],
        |r| r.get(0),
    )?;
    if audio.is_empty() {
        return Ok(None);
    }
    // 本地来源的优先级：旁边的同名文件 → 内嵌歌词（见 jp_corpus::lyric_sources）
    let sibling = crate::lyrics::sibling_lrc(&audio);
    let (text, source, reference) = if let Some(path) = sibling {
        (
            crate::lyrics::read_text(&path)?,
            "sibling",
            path.display().to_string(),
        )
    } else if let Some(e) = jp_import::embedded::read_lyrics(Path::new(&audio)) {
        (e.text, "embedded", audio)
    } else {
        return Ok(None);
    };
    let parsed = jp_import::lrc::parse(&text);
    let native = parsed.lines.iter().any(|l| !l.words.is_empty());
    let synced = parsed.lines.iter().any(|l| l.time_sec.is_some());
    let lyrics = jp_scraper::timed_lyrics::TimedLyrics {
        lines: parsed
            .lines
            .into_iter()
            .map(|l| jp_scraper::timed_lyrics::TimedLine {
                start_ms: (l.time_sec.unwrap_or(0.0) * 1000.0).round() as u64,
                end_ms: None,
                text: l.text,
                words: l
                    .words
                    .into_iter()
                    .map(|w| jp_scraper::timed_lyrics::TimedWord {
                        start: w.start,
                        end: w.end,
                        start_ms: (w.time_sec * 1000.0).round() as u64,
                        end_ms: w.end_sec.map(|s| (s * 1000.0).round() as u64),
                    })
                    .collect(),
            })
            .collect(),
    };
    let candidate = TimedFound {
        source: source.into(),
        format: if native {
            "ELRC"
        } else if synced {
            "LRC"
        } else {
            "TEXT"
        }
        .into(),
        reference,
        title: before.title.clone(),
        artist: before.artist.clone(),
        raw_text: text,
        lyrics,
    };
    let high = precision(before, &candidate).0 > 0;
    let pending = timed::automatic_pending(before, candidate);
    Ok(Some((timed::apply(conn, dir, &pending, analyzer)?, high)))
}

/// Used by the single-song fill action as well as the shared queue. Recheck
/// after network I/O; manual edits or another instance cannot be overwritten.
pub fn fetch_one(
    conn: &mut Connection,
    dir: &Path,
    song_id: &str,
    options: &SourceOptions,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Option<(Attached, bool)>> {
    let before = timed::snapshot(conn, song_id)?;
    if protected(conn, song_id)?
        || (before.line_count() > 0 && before.timed_lines() == before.line_count())
    {
        return Ok(None);
    }
    // 本地的（旁边的文件、内嵌）优先于在线：只在还没有歌词时用得上
    if let Some(local) = local_fallback(conn, dir, &before, analyzer)? {
        return Ok(Some(local));
    }
    let mut options = options.clone();
    options.apple = false;
    let found =
        jp_scraper::timed_sources::search(&before.title, &before.artist, before.duration, &options);
    let applied = apply_best(conn, dir, &before, &found, analyzer)?;
    record_lookup(conn, song_id, &before, &found, applied.is_some(), &options)?;
    Ok(applied)
}

/// 在线查询的结论：找到了、候选挑不准（等你选）、请求失败（随时可重试）、没找到（负缓存）。
pub(crate) fn lookup_status(found: &SearchOutput, applied: bool) -> &'static str {
    if applied {
        "found"
    } else if !found.candidates.is_empty() || !found.fallback.is_empty() {
        "ambiguous"
    } else if !found.warnings.is_empty() {
        "failed"
    } else {
        "not_found"
    }
}

fn record_lookup(
    conn: &Connection,
    song_id: &str,
    before: &timed::Snapshot,
    found: &SearchOutput,
    applied: bool,
    options: &SourceOptions,
) -> Result<()> {
    let status = lookup_status(found, applied);
    let message = match status {
        "ambiguous" => review_reason(before, found).to_string(),
        "failed" => found
            .warnings
            .iter()
            .map(|w| format!("{}: {}", w.source, timed::redact(&w.message, options)))
            .collect::<Vec<_>>()
            .join(" · "),
        _ => String::new(),
    };
    jp_corpus::lyric_sources::record_lookup(conn, song_id, "online", status, &message)
}

pub fn enqueue(
    report: impl Fn(&Progress) + Send + Sync + 'static,
    batch: Arc<Batch>,
    db_path: PathBuf,
    lyrics_dir: PathBuf,
    ids: Vec<String>,
    options: SourceOptions,
) -> Result<()> {
    enqueue_inner(report, batch, db_path, lyrics_dir, ids, options, None, |work| {
        std::thread::spawn(work);
    })
}

/// FFI 的 Job 已经有工作线程，直接在那条线程上运行同一条队列。
pub fn run_batch(
    batch: Arc<Batch>, db_path: PathBuf, lyrics_dir: PathBuf, ids: Vec<String>,
    options: SourceOptions, cancelled: Arc<AtomicBool>,
    report: impl Fn(&Progress) + Send + Sync + 'static,
) -> Result<Progress> {
    if ids.is_empty() {
        let progress = Progress { cancelled: cancelled.load(Ordering::SeqCst), ..Default::default() };
        batch.queue.lock().unwrap_or_else(|e| e.into_inner()).progress = progress.clone();
        report(&progress);
        return Ok(progress);
    }
    enqueue_inner(report, Arc::clone(&batch), db_path, lyrics_dir, ids, options, Some(cancelled), |work| work())?;
    Ok(batch.status())
}

#[allow(clippy::too_many_arguments)]
fn enqueue_inner(
    report: impl Fn(&Progress) + Send + Sync + 'static,
    batch: Arc<Batch>, db_path: PathBuf, lyrics_dir: PathBuf, ids: Vec<String>,
    options: SourceOptions, cancelled: Option<Arc<AtomicBool>>,
    spawn: impl FnOnce(Box<dyn FnOnce() + Send>),
) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut queue = batch.queue.lock().unwrap_or_else(|e| e.into_inner());
    if queue.progress.running {
        // Importing more songs joins the current queue instead of silently
        // dropping them while a whole-library job is in flight.
        for id in ids {
            if queue.seen.insert(id.clone()) {
                queue.pending.push_back(id);
                queue.progress.total += 1;
            }
        }
        return Ok(());
    }
    let report = Arc::new(report);
    let crashed_report = Arc::clone(&report);
    let crashed_batch = Arc::downgrade(&batch);
    let guard = batch
        .gate
        .start_with(move || {
            if let Some(batch) = crashed_batch.upgrade() {
                let mut q = batch.queue.lock().unwrap_or_else(|e| e.into_inner());
                q.progress.running = false;
                q.progress.error = "歌词批量任务意外中断，请重试".into();
                crashed_report(&q.progress);
            }
        })
        .ok_or_else(|| anyhow::anyhow!("歌词批量任务正在结束，请稍后重试"))?;
    *queue = Queue::default();
    for id in ids {
        if queue.seen.insert(id.clone()) {
            queue.pending.push_back(id);
        }
    }
    queue.progress = Progress {
        running: true,
        total: queue.pending.len(),
        ..Default::default()
    };
    report(&queue.progress);
    drop(queue);
    let cancelled = cancelled.unwrap_or_else(|| batch.gate.cancel_flag());
    spawn(Box::new(move || {
        let mut guard = Some(guard);
        let outcome = (|| -> Result<()> {
            let mut conn = Connection::open(&db_path)?;
            conn.busy_timeout(std::time::Duration::from_secs(30))?;
            conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
            let root = db_path.parent().unwrap_or(Path::new("."));
            let analyzer = jp_tokenizer::locate_sudachipy(root)
                .and_then(|(res, dict)| jp_tokenizer::Analyzer::from_sudachipy(&res, &dict).ok());
            // Shared across songs, so the gap also holds between one song's last
            // request and the next song's first one.
            let hosts = Arc::new(Mutex::new(HashMap::new()));
            loop {
                let id = {
                    let mut q = batch.queue.lock().unwrap_or_else(|e| e.into_inner());
                    if cancelled.load(Ordering::SeqCst) || q.pending.is_empty() {
                        q.progress.cancelled = cancelled.load(Ordering::SeqCst);
                        q.progress.running = false;
                        q.pending.clear();
                        drop(guard.take());
                        report(&q.progress);
                        return Ok(());
                    }
                    q.pending.pop_front()
                };
                let Some(id) = id else {
                    continue;
                };
                let before = timed::snapshot(&conn, &id);
                let title = before
                    .as_ref()
                    .map(|s| s.title.clone())
                    .unwrap_or_else(|_| id.clone());
                {
                    let mut q = batch.queue.lock().unwrap_or_else(|e| e.into_inner());
                    q.progress.title = title.clone();
                    report(&q.progress);
                }
                let result = (|| -> Result<(&str, String, String)> {
                    let before = before?;
                    if protected(&conn, &id)? {
                        return Ok(("protected", String::new(), String::new()));
                    }
                    if before.line_count() > 0 && before.timed_lines() == before.line_count() {
                        return Ok(("complete", String::new(), String::new()));
                    }
                    // 本地有的不上网
                    if let Some((attached, high)) =
                        local_fallback(&mut conn, &lyrics_dir, &before, analyzer.as_ref())?
                    {
                        return Ok((
                            if high { "upgraded" } else { "lineFilled" },
                            attached.source.as_str().into(),
                            String::new(),
                        ));
                    }
                    // 上次在线没找到、负缓存还没过期：批量里不再去问（单首「补齐本首」照样会问）
                    if jp_corpus::lyric_sources::recently_not_found(&conn, &id, "online")? {
                        return Ok(("cachedNotFound", String::new(), String::new()));
                    }
                    let mut opts = options.clone();
                    opts.apple = false;
                    let found = jp_scraper::timed_sources::search_with_transport(
                        &before.title,
                        &before.artist,
                        before.duration,
                        &opts,
                        Box::new(CancellableTransport {
                            inner: Box::new(jp_scraper::UreqTransport),
                            cancelled: Arc::clone(&cancelled),
                            last: Arc::clone(&hosts),
                        }),
                    );
                    // Cancellation during a request discards its result before writing.
                    if cancelled.load(Ordering::SeqCst) {
                        return Ok(("cancelled", String::new(), String::new()));
                    }
                    let applied = apply_best(&mut conn, &lyrics_dir, &before, &found, analyzer.as_ref())?;
                    record_lookup(&conn, &id, &before, &found, applied.is_some(), &opts)?;
                    match applied {
                        Some((attached, high)) => Ok((
                            if high { "upgraded" } else { "lineFilled" },
                            attached.source.as_str().into(),
                            String::new(),
                        )),
                        None => {
                            // 已经是逐字、只是没找到覆盖更多行的候选：不是要人复核的事。
                            let kept = before.timed_lines() > 0
                                && found.candidates.iter().any(|c| {
                                    same_identity(&before, c)
                                        && timed::compatible(&before, c)
                                        && timed::agrees_for_automatic(&before, c)
                                });
                            let status = if kept {
                                "kept"
                            } else if !found.candidates.is_empty() || !found.fallback.is_empty() {
                                "review"
                            } else if !found.warnings.is_empty() {
                                "failed"
                            } else {
                                "notFound"
                            };
                            let message = if status == "kept" {
                                String::new()
                            } else if status == "review" {
                                review_reason(&before, &found).to_string()
                            } else {
                                found
                                    .warnings
                                    .iter()
                                    .map(|w| {
                                        format!("{}: {}", w.source, timed::redact(&w.message, &opts))
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" · ")
                            };
                            Ok((status, String::new(), message))
                        }
                    }
                })();
                let (status, source, message) = result.unwrap_or_else(|e| {
                    (
                        "failed",
                        String::new(),
                        timed::redact(&format!("{e:#}"), &options),
                    )
                });
                let mut q = batch.queue.lock().unwrap_or_else(|e| e.into_inner());
                if status != "cancelled" {
                    q.progress.done += 1;
                }
                match status {
                    "upgraded" => q.progress.upgraded += 1,
                    "lineFilled" => q.progress.line_filled += 1,
                    "failed" => q.progress.failed += 1,
                    "notFound" | "cachedNotFound" => q.progress.not_found += 1,
                    "cancelled" => {}
                    _ => q.progress.skipped += 1,
                }
                q.progress.results.push(Item {
                    song_id: id,
                    title,
                    status: status.into(),
                    source,
                    message,
                });
                report(&q.progress);
            }
        })();
        if outcome.is_ok() {
            return;
        }
        let mut q = batch.queue.lock().unwrap_or_else(|e| e.into_inner());
        q.progress.cancelled = cancelled.load(Ordering::SeqCst);
        q.progress.running = false;
        if let Err(e) = outcome {
            q.progress.error = timed::redact(&format!("{e:#}"), &options);
        }
        q.pending.clear();
        drop(guard.take());
        report(&q.progress);
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jp_scraper::timed_lyrics::{TimedLine, TimedLyrics, TimedWord};

    fn database(raw: Option<&str>) -> jp_corpus::Corpus {
        let corpus = jp_corpus::Corpus::open_in_memory().unwrap();
        corpus.ensure_schema().unwrap();
        corpus.connection().execute("INSERT INTO songs(id,title,artist,duration_sec) VALUES('batch','試験曲','テスト歌手',60)",[]).unwrap();
        if let Some(raw) = raw {
            jp_import::attach_lyrics_text(
                corpus.connection(),
                "batch",
                raw,
                &jp_import::LyricSource::new("online", "netease", "old", "user"),
                None,
                "テスト歌手",
                "試験曲",
            )
            .unwrap();
        }
        corpus
    }
    fn candidate() -> TimedFound {
        TimedFound {
            source: "qq".into(),
            format: "QRC".into(),
            reference: "synthetic".into(),
            title: "試験曲".into(),
            artist: "テスト歌手".into(),
            raw_text: "synthetic-original".into(),
            lyrics: TimedLyrics {
                lines: vec![TimedLine {
                    text: "あの空".into(),
                    start_ms: 10_000,
                    end_ms: Some(12_000),
                    words: vec![
                        TimedWord {
                            start: 0,
                            end: 1,
                            start_ms: 10_000,
                            end_ms: Some(10_500),
                        },
                        TimedWord {
                            start: 1,
                            end: 3,
                            start_ms: 10_700,
                            end_ms: Some(12_000),
                        },
                    ],
                }],
            },
        }
    }
    fn output(c: TimedFound) -> SearchOutput {
        SearchOutput {
            candidates: vec![c],
            ..Default::default()
        }
    }
    struct Clock(Arc<Mutex<Vec<(String, Instant)>>>);
    impl jp_scraper::Transport for Clock {
        fn get(
            &self,
            url: &str,
            _: &[(String, String)],
            _: jp_scraper::http::Timeouts,
        ) -> std::result::Result<jp_scraper::http::HttpResponse, jp_scraper::ProviderError> {
            self.0.lock().unwrap().push((url.into(), Instant::now()));
            Err(jp_scraper::ProviderError::unavailable("offline"))
        }
        fn post(
            &self,
            url: &str,
            _: &[u8],
            h: &[(String, String)],
            t: jp_scraper::http::Timeouts,
        ) -> std::result::Result<jp_scraper::http::HttpResponse, jp_scraper::ProviderError> {
            self.get(url, h, t)
        }
    }

    #[test]
    fn requests_to_one_host_are_spaced_but_other_hosts_are_not_held_up() {
        use jp_scraper::Transport;
        let log = Arc::new(Mutex::new(Vec::new()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let transport = CancellableTransport {
            inner: Box::new(Clock(log.clone())),
            cancelled: cancelled.clone(),
            last: Arc::default(),
        };
        let t = jp_scraper::http::Timeouts::new(Duration::from_secs(1), Duration::from_secs(1));
        let _ = transport.get("https://a.example/x?q=1", &[], t);
        let _ = transport.get("https://b.example/y", &[], t);
        let _ = transport.get("https://A.example/z", &[], t);
        {
            let log = log.lock().unwrap();
            assert_eq!(log.len(), 3);
            assert!(log[1].1 - log[0].1 < HOST_GAP / 2, "another host goes straight away");
            // 间隔从放行那一刻算起，这里量的是放行后真正发出的时刻，两头各有几毫秒调度误差（CI 上见过）
            assert!(log[2].1 - log[0].1 >= HOST_GAP * 9 / 10, "same host waits its turn");
        }
        cancelled.store(true, Ordering::SeqCst);
        let started = Instant::now();
        assert!(transport.get("https://a.example/again", &[], t).is_err());
        assert!(started.elapsed() < HOST_GAP / 2, "Stop does not wait out the gap");
        assert_eq!(log.lock().unwrap().len(), 3, "nothing is sent after Stop");
    }

    #[test]
    fn precision_counts_lines_as_they_will_be_stored() {
        // 「夜が　明ける」：全角空格没有时间，入库时并进相邻单位，这一行应该算能逐字点亮。
        let corpus = database(None);
        let before = timed::snapshot(corpus.connection(), "batch").unwrap();
        let mut found = candidate();
        found.lyrics.lines[0] = TimedLine {
            text: "夜が\u{3000}明ける".into(),
            start_ms: 10_000,
            end_ms: Some(12_000),
            words: vec![
                TimedWord { start: 0, end: 2, start_ms: 10_000, end_ms: Some(10_800) },
                TimedWord { start: 3, end: 6, start_ms: 10_900, end_ms: Some(12_000) },
            ],
        };
        assert_eq!(precision(&before, &found), (1, 1));
    }

    #[test]
    fn empty_and_line_only_songs_are_both_upgrade_targets() {
        let empty = database(None);
        assert_eq!(timed::targets(empty.connection()).unwrap().len(), 1);
        let lines = database(Some("[00:10]あの空"));
        assert_eq!(timed::targets(lines.connection()).unwrap().len(), 1);
        let snapshot = timed::snapshot(lines.connection(), "batch").unwrap();
        assert!(choose(&snapshot, &output(candidate())).unwrap().1);
    }
    #[test]
    fn automatic_replacement_uses_the_players_validated_word_coverage() {
        let mut corpus = database(Some("[00:10]あの空"));
        let before = timed::snapshot(corpus.connection(), "batch").unwrap();
        let dir = std::env::temp_dir().join(format!("jpop-batch-{}", std::process::id()));
        let (attached, high) = apply_best(
            corpus.connection_mut(),
            &dir,
            &before,
            &output(candidate()),
            None,
        )
        .unwrap()
        .unwrap();
        assert!(high);
        assert_eq!(attached.source, crate::lyrics::LyricsSource::Qq);
        assert_eq!(
            timed::snapshot(corpus.connection(), "batch")
                .unwrap()
                .timed_lines(),
            1
        );
        let origin: String = corpus
            .connection()
            .query_row(
                "SELECT timing_origin FROM lyric_versions WHERE song_id='batch' AND is_current=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(origin, "native");
        let _ = std::fs::remove_file(dir.join("batch.lrc"));
        let _ = std::fs::remove_dir(dir);
    }
    #[test]
    fn a_line_fallback_only_fills_an_empty_song() {
        let fallback = jp_scraper::timed_sources::BasicFound {
            format: "LRC".into(),
            mode: "lineSynced".into(),
            source: "lrclib".into(),
            reference: "synthetic".into(),
            title: "試験曲".into(),
            artist: "テスト歌手".into(),
            raw_text: "[00:10]あの空".into(),
        };
        let found = SearchOutput {
            fallback: vec![fallback],
            ..Default::default()
        };
        let empty = database(None);
        assert!(
            !choose(
                &timed::snapshot(empty.connection(), "batch").unwrap(),
                &found
            )
            .unwrap()
            .1
        );
        let existing = database(Some("[00:10]あの空"));
        assert!(
            choose(
                &timed::snapshot(existing.connection(), "batch").unwrap(),
                &found
            )
            .is_none()
        );
    }
    #[test]
    fn same_title_wrong_artist_or_different_body_requires_review() {
        let corpus = database(Some("[00:10]あの空"));
        let before = timed::snapshot(corpus.connection(), "batch").unwrap();
        let mut wrong = candidate();
        wrong.artist = "別の歌手".into();
        assert!(choose(&before, &output(wrong)).is_none());
        let mut wrong = candidate();
        wrong.lyrics.lines[0].text = "あの海".into();
        assert!(choose(&before, &output(wrong)).is_none());
    }
    #[test]
    fn a_review_says_which_check_the_closest_candidate_failed() {
        let corpus = database(Some("[00:10]あの空"));
        let before = timed::snapshot(corpus.connection(), "batch").unwrap();
        let mut other_text = candidate();
        other_text.lyrics.lines[0].text = "あの海".into();
        let mut wrong_artist = candidate();
        wrong_artist.artist = "別の歌手".into();
        let found = SearchOutput { candidates: vec![wrong_artist.clone(), other_text], ..Default::default() };
        assert_eq!(review_reason(&before, &found), "候选的时长或正文和现有歌词差别太大");
        let found = SearchOutput { candidates: vec![wrong_artist], ..Default::default() };
        assert_eq!(review_reason(&before, &found), "候选的歌名或歌手和这首歌对不上");
        assert_eq!(review_reason(&before, &SearchOutput::default()), "只找到逐行歌词；已有歌词不会被逐行版本替换");
    }

    #[test]
    fn manual_edits_after_search_are_rechecked_inside_the_write_transaction() {
        let mut corpus = database(Some("[00:10]あの空"));
        let before = timed::snapshot(corpus.connection(), "batch").unwrap();
        let pending = timed::automatic_pending(&before, candidate());
        corpus.connection().execute("UPDATE lyric_versions SET source_type='manual_edit' WHERE song_id='batch' AND is_current=1",[]).unwrap();
        let dir = std::env::temp_dir().join("unused-jpop-protected");
        assert!(timed::apply(corpus.connection_mut(), &dir, &pending, None).is_err());
        assert_eq!(
            timed::snapshot(corpus.connection(), "batch")
                .unwrap()
                .timed_lines(),
            0
        );
    }
    #[test]
    fn importing_during_a_batch_appends_without_duplicates_or_losing_cancel() {
        let batch = Arc::new(Batch::default());
        let _guard = batch.gate.start().unwrap();
        {
            let mut q = batch.queue.lock().unwrap();
            q.progress.running = true;
            q.progress.total = 1;
            q.seen.insert("one".into());
        }
        batch.cancel();
        enqueue(
            |_| {},
            batch.clone(),
            PathBuf::new(),
            PathBuf::new(),
            vec!["one".into(), "two".into(), "two".into()],
            SourceOptions::default(),
        )
        .unwrap();
        assert!(batch.gate.cancelled());
        assert_eq!(batch.status().total, 2);
        assert_eq!(batch.queue.lock().unwrap().pending.len(), 1);
    }

    #[test]
    fn the_background_worker_finishes_a_real_isolated_queue_without_network() {
        let root = std::env::temp_dir().join(format!("jpop-offline-queue-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let db = root.join("corpus.db");
        {
            let corpus = jp_corpus::Corpus::open_writable(&db).unwrap();
            corpus.ensure_schema().unwrap();
            corpus.connection().execute("INSERT OR REPLACE INTO songs(id,title,artist,duration_sec) VALUES('queued','試験曲','テスト歌手',60)",[]).unwrap();
        }
        let batch = Arc::new(Batch::default());
        let options = SourceOptions {
            netease: false,
            qq: false,
            kugou: false,
            amll: false,
            lrclib: false,
            apple: false,
            ..Default::default()
        };
        enqueue(
            |_| {},
            batch.clone(),
            db,
            root.join("lyrics"),
            vec!["queued".into(), "queued".into()],
            options,
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while batch.status().running && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let result = batch.status();
        assert!(!result.running);
        assert_eq!(result.total, 1);
        assert_eq!(result.done, 1);
        assert_eq!(result.not_found, 1);
        assert_eq!(result.failed, 0);
        assert_eq!(result.results.len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_fallback_is_still_available_and_does_not_forge_word_timing() {
        let root = std::env::temp_dir().join(format!("jpop-local-batch-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let audio = root.join("fixture.flac");
        std::fs::write(audio.with_extension("lrc"), "[00:10]あの空").unwrap();
        let mut corpus = database(None);
        corpus
            .connection()
            .execute(
                "UPDATE songs SET audio_path=?1 WHERE id='batch'",
                [audio.display().to_string()],
            )
            .unwrap();
        let before = timed::snapshot(corpus.connection(), "batch").unwrap();
        let (attached, high) =
            local_fallback(corpus.connection_mut(), &root.join("lyrics"), &before, None)
                .unwrap()
                .unwrap();
        assert!(!high);
        assert_eq!(attached.source, crate::lyrics::LyricsSource::Sibling);
        assert_eq!(
            timed::snapshot(corpus.connection(), "batch")
                .unwrap()
                .timed_lines(),
            0
        );
        let kind: String = corpus
            .connection()
            .query_row(
                "SELECT source_type FROM lyric_versions WHERE song_id='batch' AND is_current=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kind, "sibling");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    #[ignore = "需要公开 QQ 歌词服务；仅在独立临时库测试真实批量获取"]
    fn public_qq_batch_upgrades_an_isolated_song_end_to_end() {
        let root = std::env::temp_dir().join(format!("jpop-live-batch-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let db = root.join("corpus.db");
        {
            let corpus = jp_corpus::Corpus::open_writable(&db).unwrap();
            corpus.ensure_schema().unwrap();
            corpus.connection().execute("INSERT INTO songs(id,title,artist,duration_sec) VALUES('live','夜に駆ける','YOASOBI',261)",[]).unwrap();
        }
        let batch = Arc::new(Batch::default());
        let options = SourceOptions {
            netease: false,
            qq: true,
            kugou: false,
            amll: false,
            lrclib: false,
            apple: false,
            ..Default::default()
        };
        enqueue(
            |_| {},
            batch.clone(),
            db.clone(),
            root.join("lyrics"),
            vec!["live".into()],
            options,
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
        while batch.status().running && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let progress = batch.status();
        assert!(!progress.running, "queue timed out");
        let conn = Connection::open(&db).unwrap();
        let view = timed::snapshot(&conn, "live").unwrap();
        println!(
            "QQ batch: done={} upgraded={} failed={} lines={} valid_native={}",
            progress.done,
            progress.upgraded,
            progress.failed,
            view.line_count(),
            view.timed_lines()
        );
        // Real lyrics stay in this disposable directory, never in fixtures or the user library.
        drop(conn);
        let _ = std::fs::remove_dir_all(root);
        assert_eq!(
            progress.upgraded,
            1,
            "{}",
            progress
                .results
                .iter()
                .map(|r| r.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        );
        assert!(view.timed_lines() > 0);
    }
}
