//! 补齐歌词：音频旁边的 .lrc、在线搜、用户自己指定的文件，三条路一个出口。
//!
//! ## 为什么需要这一层
//!
//! 0.1.x 的歌词是 `scripts/01_fetch_lrc.py` 用 `syncedlyrics` 下回来的，
//! 存成 `raw/lyrics_lrc/{song_id}.lrc`，然后 `02_parse_lrc_tokenize.py` 才入库。
//! Tauri 版的导入**只认已经在磁盘上的 .lrc**（扫描时在音频旁边找），
//! 于是从别处拷进来的歌——音乐 App 的下载目录里只有音频——导进来是一行歌词都没有。
//! 用户看到的就是「以前导入会自动匹配歌词，现在不会了」。
//!
//! ## 三条路，一个出口
//!
//! | 来源 | 什么时候用 |
//! |---|---|
//! | `Sibling` 音频旁边的 `<同名>.lrc` | 导入之后才下了歌词、或者导入时漏了 |
//! | `Netease` 在线搜 | 旁边什么都没有（见 [`jp_scraper::lyrics`]） |
//! | `Manual` 用户自己选的文件 | 自动匹配挑不出来，或者用户有更好的一份 |
//!
//! 三条路都走 [`apply_lrc_text`]：**先把 .lrc 落到 `raw/lyrics_lrc/{song_id}.lrc`，
//! 再入库**。顺序是刻意的——
//!
//! * 落盘在前：写文件失败就什么都没改，重试一次即可；
//! * 一个事务里清旧挂新：歌词行、全文索引、分词、作词作曲必须同时成立，
//!   不然会得到一首「有歌词但搜不到」或者「分了一半词」的歌；
//! * 文件留着是为了可恢复：库坏了、或者想换个分词器重新入库，手上还有原始歌词，
//!   和 0.1.x 的目录结构也对得上。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

/// 一首还没有歌词的歌。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingLyrics {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub duration_sec: Option<f64>,
    pub audio_path: String,
    /// 音频旁边就有一份 .lrc——这种不用上网
    pub sibling_lrc: Option<String>,
}

/// 歌词是哪来的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LyricsSource {
    /// 音频旁边的同名 .lrc
    Sibling,
    /// 在线搜到的
    Netease,
    /// 用户自己选的文件
    Manual,
}

impl LyricsSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sibling => "sibling",
            Self::Netease => "netease",
            Self::Manual => "manual",
        }
    }
}

/// 一首歌挂上歌词之后的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attached {
    pub song_id: String,
    pub source: LyricsSource,
    /// 在线搜到的那条叫什么（「曲名 — 歌手」），本地来源是文件名
    pub matched: String,
    pub lyric_lines: usize,
    pub tokens: usize,
    pub credits: usize,
    pub corrections_restored: usize,
    /// 落盘的 .lrc 路径
    pub lrc_path: String,
}

/// 库里还没有歌词的歌。
///
/// 判据是 `utterances` 里一行都没有，而不是 `source_file` 为空：
/// 存量库里 `source_file` 全是 NULL（Python 侧没写过这一列），
/// 按它来判断会把 206 首有歌词的歌全都当成缺歌词。
pub fn missing(conn: &Connection) -> Result<Vec<MissingLyrics>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.title, s.artist, s.duration_sec, COALESCE(s.audio_path,'') \
         FROM songs s \
         WHERE NOT EXISTS (SELECT 1 FROM utterances u WHERE u.song_id = s.id) \
         ORDER BY s.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<f64>>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (song_id, title, artist, duration_sec, audio_path) = row?;
        let sibling_lrc = sibling_lrc(&audio_path).map(|p| p.display().to_string());
        out.push(MissingLyrics {
            song_id,
            title,
            artist,
            duration_sec,
            audio_path,
            sibling_lrc,
        });
    }
    Ok(out)
}

/// 音频旁边的同名 .lrc，有才返回。
///
/// 导入时找的就是这个（见 `jp_import::scan`）。在这里再找一次是为了
/// 「导入完才去下歌词」的情况：文件后来才出现，没必要为此重导一遍歌。
pub fn sibling_lrc(audio_path: &str) -> Option<PathBuf> {
    if audio_path.is_empty() {
        return None;
    }
    let candidate = Path::new(audio_path).with_extension("lrc");
    candidate.is_file().then_some(candidate)
}

/// 把一段 LRC 文本挂到一首已经在库里的歌上。三条来源的共同出口。
///
/// 先落盘 `raw/lyrics_lrc/{song_id}.lrc`，再在**一个事务**里清掉旧歌词、挂上新的。
/// `clear_lyrics` 让这个函数是可重入的：同一首歌换一份歌词，不会留下两份歌词行，
/// 也不会在全文索引里留下搜得到、点不开的幽灵行。
///
/// 用户校正过的分词不会因为换歌词而丢——校正按「歌手 + 曲名 + 原文」匹配，
/// `attach_lyrics` 末尾会把它们套回新的行号上。
pub fn apply_lrc_text(
    conn: &mut Connection,
    lyrics_dir: &Path,
    song_id: &str,
    artist: &str,
    title: &str,
    lrc_text: &str,
    source: LyricsSource,
    matched: &str,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Attached> {
    if lrc_text.trim().is_empty() {
        bail!("歌词是空的");
    }
    std::fs::create_dir_all(lyrics_dir)
        .with_context(|| format!("建不出歌词目录：{}", lyrics_dir.display()))?;
    let dest = lyrics_dir.join(format!("{song_id}.lrc"));
    write_atomic(&dest, lrc_text.as_bytes())
        .with_context(|| format!("写歌词文件失败：{}", dest.display()))?;

    let tx = conn.transaction()?;
    jp_import::maintain::clear_lyrics(&tx, song_id)?;
    let stats = jp_import::attach_lyrics(&tx, song_id, &dest, analyzer, artist, title)?;
    if stats.lyric_lines == 0 {
        // 解析出 0 行：这份文件不是歌词（空文件、纯标签、纯时间轴）。
        // 回滚，让这首歌继续算「缺歌词」——否则它会从补齐名单里消失，
        // 而用户永远不知道为什么还是没有歌词。
        bail!("这份歌词解析出 0 行，没有改动");
    }
    // source_file 记一笔来处。存量库这一列是空的，不靠它判断有无歌词（见 `missing`），
    // 但重装、迁库时它是唯一能回答「这份歌词哪来的」的东西。
    tx.execute(
        "UPDATE songs SET source_file=?2 WHERE id=?1",
        params![song_id, dest.display().to_string()],
    )?;
    tx.commit()?;

    Ok(Attached {
        song_id: song_id.to_string(),
        source,
        matched: matched.to_string(),
        lyric_lines: stats.lyric_lines,
        tokens: stats.tokens,
        credits: stats.credits,
        corrections_restored: stats.corrections_restored,
        lrc_path: dest.display().to_string(),
    })
}

/// 先写临时文件再改名：写一半断电/崩了的话，原来那份歌词还是完整的。
fn write_atomic(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = dest.with_extension("lrc.part");
    std::fs::write(&tmp, bytes)?;
    // Windows 上 rename 不覆盖已存在的目标，所以先删
    if dest.exists() {
        std::fs::remove_file(dest)?;
    }
    std::fs::rename(&tmp, dest)
}

/// 用户自己选的一份 .lrc / .txt。
///
/// 读进来再按 UTF-8 写出去（`jp_import::parse_lrc_file` 会处理 Shift-JIS / GBK），
/// **不是**直接复制字节：落盘那一份要能被下一次重建语料库原样读回来。
pub fn attach_from_file(
    conn: &mut Connection,
    lyrics_dir: &Path,
    song_id: &str,
    path: &Path,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Attached> {
    let (artist, title) = song_names(conn, song_id)?;
    let text = read_text(path)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    apply_lrc_text(
        conn,
        lyrics_dir,
        song_id,
        &artist,
        &title,
        &text,
        LyricsSource::Manual,
        &name,
        analyzer,
    )
}

/// 读文本，编码按 UTF-8 → CP932 → GBK 试，和 `jp_import::lrc::parse_file` 同一套规则。
fn read_text(path: &Path) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("读不了歌词文件：{}", path.display()))?;
    if let Ok(text) = std::str::from_utf8(&bytes) {
        return Ok(text.trim_start_matches('\u{feff}').to_string());
    }
    for encoding in [encoding_rs::SHIFT_JIS, encoding_rs::GBK] {
        let (text, _, had_errors) = encoding.decode(&bytes);
        if !had_errors {
            return Ok(text.trim_start_matches('\u{feff}').to_string());
        }
    }
    bail!(
        "{} 的编码认不出来（试过 UTF-8、CP932、GBK）。请另存为 UTF-8 再导入——\
         硬读会得到一整份乱码歌词，比没有歌词更糟",
        path.display()
    )
}

fn song_names(conn: &Connection, song_id: &str) -> Result<(String, String)> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT artist, title FROM songs WHERE id=?1",
        params![song_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()?
    .with_context(|| format!("找不到曲目 {song_id}"))
}

/// 一首歌走一遍「先本地、再在线」。
///
/// 本地优先是因为它必然对：文件就在那首歌旁边，不存在挑错的风险，还不用联网。
pub fn fill_one(
    conn: &mut Connection,
    lyrics_dir: &Path,
    provider: &jp_scraper::lyrics::NeteaseLyrics,
    target: &MissingLyrics,
    analyzer: Option<&jp_tokenizer::Analyzer>,
) -> Result<Option<Attached>> {
    if let Some(local) = sibling_lrc(&target.audio_path) {
        let text = read_text(&local)?;
        let name = local
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        return Ok(Some(apply_lrc_text(
            conn,
            lyrics_dir,
            &target.song_id,
            &target.artist,
            &target.title,
            &text,
            LyricsSource::Sibling,
            &name,
            analyzer,
        )?));
    }

    let found = provider
        .best_lyrics(&target.title, &target.artist, target.duration_sec)
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    let Some((hit, text)) = found else {
        return Ok(None);
    };
    let matched = format!("{} — {}", hit.title, hit.artist);
    Ok(Some(apply_lrc_text(
        conn,
        lyrics_dir,
        &target.song_id,
        &target.artist,
        &target.title,
        &text,
        LyricsSource::Netease,
        &matched,
        analyzer,
    )?))
}

/// 搜歌词用的客户端。
///
/// 限流 600ms：一首歌两次请求（搜索 + 取歌词），整库两百首约三分半。
/// 这是个公开接口，不是给我们用的，排队比被封好。
pub fn provider() -> jp_scraper::lyrics::NeteaseLyrics {
    let client = jp_scraper::HttpClient::new(Box::new(jp_scraper::UreqTransport))
        .with_rate_limiter(Arc::new(jp_scraper::RateLimiter::new(
            std::time::Duration::from_millis(600),
        )));
    jp_scraper::lyrics::NeteaseLyrics::new(client)
}

/// 一次批量补齐的进度。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsProgress {
    pub done: usize,
    pub total: usize,
    pub song_id: String,
    pub title: String,
    /// filled / notFound / failed
    pub status: String,
    /// 这首歌的歌词哪来的（失败和没找到时是空串）
    pub source: String,
    pub matched: String,
    pub lyric_lines: usize,
    /// 失败原因，成功时是空串
    pub message: String,
    // 跑到这一步的累计数，省得前端自己攒
    pub filled: usize,
    pub not_found: usize,
    pub failed: usize,
    pub finished: bool,
    pub cancelled: bool,
}

/// 批量补齐作业的共享状态。和刮削、Anki、词典各自独立。
#[derive(Default)]
pub struct LyricsJob {
    running: AtomicBool,
    cancel: AtomicBool,
}

impl LyricsJob {
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    fn try_start(&self) -> bool {
        self.cancel.store(false, Ordering::SeqCst);
        !self.running.swap(true, Ordering::SeqCst)
    }

    fn finish(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// 在后台线程补一批歌词。立刻返回，进度走 `lyrics://progress` 事件。
///
/// **自己开一条数据库连接**，和刮削一样的理由：占着 UI 那条连接几分钟的话，
/// 期间所有查询都会卡住。分词器也在线程里单独建一份——`Analyzer` 借不出
/// `'static` 的引用，而重建一次只要一秒多。
pub fn spawn_fill<R: Runtime>(
    app: AppHandle<R>,
    job: Arc<LyricsJob>,
    db_path: PathBuf,
    lyrics_dir: PathBuf,
    song_ids: Option<Vec<String>>,
) -> Result<()> {
    anyhow::ensure!(job.try_start(), "已经有一个补齐歌词的作业在跑");

    std::thread::spawn(move || {
        // 收尾事件里要报的那几个数。闭包里每处理一首就更新，
        // 中途出错也还是这一份——**不能在收尾时另起一个 default()**，
        // 否则用户看到的汇总永远是「补上 0，挑不出来 0」。
        let mut tally = LyricsProgress::default();
        let outcome = (|| -> Result<bool> {
            let mut conn = rusqlite::Connection::open(&db_path)?;
            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

            let mut targets = missing(&conn)?;
            if let Some(wanted) = &song_ids {
                targets.retain(|t| wanted.contains(&t.song_id));
            }
            tally.total = targets.len();

            let project_root = db_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let analyzer = jp_tokenizer::locate_sudachipy(&project_root)
                .and_then(|(res, dict)| jp_tokenizer::Analyzer::from_sudachipy(&res, &dict).ok());
            if analyzer.is_none() {
                eprintln!("[warn] 补齐歌词：没有分词器，歌词会入库但不分词");
            }
            let provider = provider();

            for target in &targets {
                if job.cancel.load(Ordering::SeqCst) {
                    return Ok(true);
                }
                let result = fill_one(
                    &mut conn,
                    &lyrics_dir,
                    &provider,
                    target,
                    analyzer.as_ref(),
                );
                tally.done += 1;
                let progress = match result {
                    Ok(Some(attached)) => {
                        tally.filled += 1;
                        LyricsProgress {
                            status: "filled".into(),
                            source: attached.source.as_str().into(),
                            matched: attached.matched,
                            lyric_lines: attached.lyric_lines,
                            ..Default::default()
                        }
                    }
                    Ok(None) => {
                        tally.not_found += 1;
                        LyricsProgress {
                            status: "notFound".into(),
                            ..Default::default()
                        }
                    }
                    // 一首歌失败不该中断整批：网络会抽，个别文件会坏
                    Err(err) => {
                        tally.failed += 1;
                        LyricsProgress {
                            status: "failed".into(),
                            message: format!("{err:#}"),
                            ..Default::default()
                        }
                    }
                };
                let _ = app.emit(
                    "lyrics://progress",
                    LyricsProgress {
                        done: tally.done,
                        total: tally.total,
                        song_id: target.song_id.clone(),
                        title: target.title.clone(),
                        filled: tally.filled,
                        not_found: tally.not_found,
                        failed: tally.failed,
                        ..progress
                    },
                );
            }
            Ok(false)
        })();

        let (cancelled, message) = match outcome {
            Ok(cancelled) => (cancelled, String::new()),
            Err(err) => {
                eprintln!("[error] 补齐歌词作业中断: {err:#}");
                // 整批断了要说出来，不能只是悄悄「完成」
                (false, format!("{err:#}"))
            }
        };
        job.finish();
        let _ = app.emit(
            "lyrics://progress",
            LyricsProgress {
                done: tally.done,
                total: tally.total,
                finished: true,
                cancelled,
                message,
                ..tally
            },
        );
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 和真库同构的一小块：补齐歌词碰得到的表
    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE songs (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, artist TEXT NOT NULL,
                year TEXT, album TEXT, genre TEXT, audio_path TEXT,
                corpus_type TEXT NOT NULL DEFAULT 'song', source_file TEXT,
                cover_path TEXT, duration_sec REAL, album_id INTEGER);
             CREATE TABLE utterances (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                song_id TEXT NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
                line_idx INTEGER NOT NULL, time_sec REAL, text TEXT NOT NULL, chapter_id INTEGER);
             CREATE VIRTUAL TABLE utterances_fts USING fts5(text, content=utterances, content_rowid=id, tokenize='trigram');
             CREATE TABLE tokens (
                id INTEGER PRIMARY KEY AUTOINCREMENT, utterance_id INTEGER NOT NULL REFERENCES utterances(id),
                token_idx INTEGER NOT NULL, surface TEXT NOT NULL, lemma TEXT NOT NULL, pos TEXT, dep TEXT, head TEXT);
             CREATE TABLE people (
                id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                normalized_name TEXT NOT NULL UNIQUE, sort_name TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL DEFAULT '');
             CREATE TABLE track_credits (
                song_id TEXT NOT NULL, person_id INTEGER NOT NULL, role TEXT NOT NULL,
                position INTEGER NOT NULL DEFAULT 0, source TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (song_id, person_id, role));",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO songs (id, title, artist, audio_path, duration_sec) \
             VALUES ('001','ネイティブダンサー','サカナクション','E:/music/a.flac', 264.2)",
            [],
        )
        .unwrap();
        conn
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jp-app-lyrics-{}-{:?}-{name}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_song_without_lyrics_is_listed_and_disappears_once_filled() {
        let mut conn = db();
        let dir = scratch("fill");
        assert_eq!(missing(&conn).unwrap().len(), 1);

        let attached = apply_lrc_text(
            &mut conn,
            &dir,
            "001",
            "サカナクション",
            "ネイティブダンサー",
            "作词 : 山口　一郎\n[00:15.00]踊りませんか\n[00:20.00]夜が終わるまで\n",
            LyricsSource::Netease,
            "ネイティブダンサー — サカナクション",
            None,
        )
        .unwrap();
        assert_eq!(attached.lyric_lines, 2);
        assert_eq!(attached.credits, 1, "LRC 里的作词没进 track_credits");
        assert!(missing(&conn).unwrap().is_empty(), "挂上歌词后还算缺歌词");

        // 落盘的那一份是给「可恢复」用的，必须真在
        let on_disk = dir.join("001.lrc");
        assert!(on_disk.is_file());
        assert!(std::fs::read_to_string(&on_disk).unwrap().contains("踊りませんか"));

        // 全文检索能搜到——外部内容表没有触发器，漏同步这里就是 0
        let hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM utterances_fts WHERE utterances_fts MATCH '踊りま'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1);
    }

    /// 换一份歌词：不能变成两份。
    #[test]
    fn applying_twice_replaces_instead_of_duplicating() {
        let mut conn = db();
        let dir = scratch("twice");
        let args = ("サカナクション", "ネイティブダンサー");
        for text in ["[00:15.00]一回目\n", "[00:15.00]二回目\n[00:20.00]もう一行\n"] {
            apply_lrc_text(
                &mut conn,
                &dir,
                "001",
                args.0,
                args.1,
                text,
                LyricsSource::Manual,
                "x.lrc",
                None,
            )
            .unwrap();
        }
        let lines: Vec<String> = conn
            .prepare("SELECT text FROM utterances WHERE song_id='001' ORDER BY line_idx")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(lines, vec!["二回目", "もう一行"]);
        let ghosts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM utterances_fts WHERE utterances_fts MATCH '一回目'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ghosts, 0, "旧歌词还留在全文索引里");
    }

    /// 解析出 0 行的文件不能让这首歌「看起来有歌词了」——
    /// 它会从补齐名单里消失，而用户永远不知道为什么还是没词。
    #[test]
    fn a_file_that_parses_to_nothing_changes_nothing() {
        let mut conn = db();
        let dir = scratch("empty");
        let err = apply_lrc_text(
            &mut conn,
            &dir,
            "001",
            "サカナクション",
            "ネイティブダンサー",
            "[ar:サカナクション]\n[ti:ネイティブダンサー]\n",
            LyricsSource::Manual,
            "tags-only.lrc",
            None,
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("0 行"), "{err:#}");
        assert_eq!(missing(&conn).unwrap().len(), 1, "这首歌还该在补齐名单里");
    }

    /// Shift-JIS 的歌词文件要能读对。硬按 UTF-8 读会得到一整份乱码，
    /// 而且因为「有歌词」了，补齐流程再也不会来看它。
    #[test]
    fn a_shift_jis_file_is_decoded_not_mangled() {
        let dir = scratch("sjis");
        let path = dir.join("sjis.lrc");
        std::fs::write(
            &path,
            b"\x5b\x30\x30\x3a\x31\x35\x2e\x30\x30\x5d\x8c\x4e\x82\xf0\x91\xd2\x82\xc1\x82\xc4\x82\xa2\x82\xe9\x0a",
        )
        .unwrap();
        assert_eq!(read_text(&path).unwrap().trim(), "[00:15.00]君を待っている");

        let mut conn = db();
        let attached = attach_from_file(&mut conn, &dir, "001", &path, None).unwrap();
        assert_eq!(attached.lyric_lines, 1);
        let text: String = conn
            .query_row("SELECT text FROM utterances WHERE song_id='001'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(text, "君を待っている");
    }

    #[test]
    fn a_sibling_lrc_is_found_next_to_the_audio() {
        let dir = scratch("sibling");
        let audio = dir.join("song.flac");
        std::fs::write(&audio, b"x").unwrap();
        assert!(sibling_lrc(&audio.display().to_string()).is_none());
        std::fs::write(dir.join("song.lrc"), "[00:01.00]あ\n").unwrap();
        assert!(sibling_lrc(&audio.display().to_string()).is_some());
        // 空路径（库里有音频丢了的歌）不该 panic
        assert!(sibling_lrc("").is_none());
    }

    #[test]
    fn two_jobs_do_not_run_at_once() {
        let job = LyricsJob::default();
        assert!(job.try_start());
        assert!(!job.try_start(), "第二个作业抢到了运行权");
        job.finish();
        assert!(job.try_start());
    }
}
