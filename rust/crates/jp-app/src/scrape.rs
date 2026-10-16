//! 刮削的后台作业。
//!
//! 单首刮削很快（一两秒），但整库 209 首要十几分钟。所以批量走后台线程，
//! 并且**自己开一条数据库连接**——占着 UI 那条连接十几分钟的话，
//! 期间所有查询都会卡住。
//!
//! 进度通过 `scrape://progress` 事件推给前端，取消靠一个原子标志。

use std::sync::Arc;

use anyhow::{Context, Result};
use jp_scraper::{ScrapeStatus, ScrapeStore, TrackFile};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

/// 一次批量刮削的进度。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrapeProgress {
    /// "track" 或 "artist"。两种作业共用一个事件通道，UI 据此分开显示。
    pub kind: String,
    pub done: usize,
    pub total: usize,
    pub song_id: String,
    pub title: String,
    pub status: String,
    pub confidence: f64,
    /// 采纳的候选，失败时是空串
    pub matched: String,
    /// 封面下没下下来
    pub cover_saved: bool,
    pub finished: bool,
    pub cancelled: bool,
    /// 整批**断在半路**的原因（库打不开、resolver 建不起来……），
    /// 正常结束时是空串。
    ///
    /// 原来没有这个字段：出错时只往日志里写一行，发给界面的终态仍然是
    /// `finished: true, cancelled: false`——用户看到的是「完成」。
    /// anki / dict / lyrics 三处早就报了，只有刮削把错误吞了。
    pub error: String,
}

/// 批量刮削作业的运行/取消状态。四个后台作业共用 `crate::job::Job`——
/// 原来这里自己写了一份，带着「第二次启动会抹掉取消」和
/// 「panic 之后界面永久卡住」两个毛病。
///
/// 和 Anki、词典、补齐歌词**各自独立**：刮削排的是外部服务的限流队列，
/// 两个批量刮削同时跑会把额度用光，别的作业和它没冲突。
pub type ScrapeJob = crate::job::Job;

/// 库里一首歌，转成刮削要的输入。
///
/// **优先用 `track_original_metadata` 里的原值**：`songs` 行可能已经被
/// 上一轮刮削覆盖过，拿它去重新匹配等于拿刮削结果再刮一次。
pub fn track_file_for(conn: &rusqlite::Connection, song_id: &str) -> Result<Option<TrackFile>> {
    use rusqlite::OptionalExtension;

    let row = conn
        .query_row(
            "SELECT title, artist, album, year, genre, audio_path, duration_sec
             FROM songs WHERE id=?1",
            rusqlite::params![song_id],
            |row| {
                Ok(TrackFile {
                    embedded_title: row.get(0)?,
                    embedded_artist: row.get(1)?,
                    embedded_album: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    embedded_year: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    embedded_genre: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    path: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    duration_sec: row.get(6)?,
                    ..Default::default()
                })
            },
        )
        .optional()?;
    let Some(track) = row else { return Ok(None) };

    // 有原始元数据就用原始的
    let store = ScrapeStore::new(conn);
    if !track.path.is_empty()
        && let Ok(Some(original)) = store.original_metadata(&track.path)
    {
        return Ok(Some(original));
    }
    Ok(Some(track))
}

/// 一首歌刮完之后的结论。
///
/// **不含封面。** 封面下载慢（实测一张 13~27 秒）而且不碰数据库，
/// 分出去才能并发——见 [`download_cover`]。
pub struct TrackOutcome {
    pub status: ScrapeStatus,
    pub confidence: f64,
    /// 采纳的候选，人类可读的一行
    pub matched: String,
    /// 自动采纳的那条候选。只有它才该去下封面。
    pub accepted: Option<jp_scraper::ScrapeCandidate>,
    /// 本地歌手名，决定封面落到哪个目录
    pub artist: String,
}

/// 刮一首并落库。**不下封面**。
pub fn scrape_one(
    conn: &rusqlite::Connection,
    resolver: &jp_scraper::MetadataResolver,
    song_id: &str,
    track: &TrackFile,
) -> Result<TrackOutcome> {
    prepare(conn, song_id, track)?;
    let result = resolver.resolve(track);
    finish(conn, song_id, track, result)
}

/// 网络之前：登记原始元数据，状态置 running。
///
/// 和 [`finish`] 分开是为了让中间那一步（`resolver.resolve`）能并发跑——
/// `resolve` 是纯函数，不碰数据库。实测 4 路并发提速 3.0×
/// （5.60 → 1.88 秒/首，整库 20 分钟 → 7 分钟）。
///
/// 我先前判断「MusicBrainz 限每秒一次，并发收益有限」是错的：
/// 限流器限的是两次请求的**间隔**，不是同时在飞的数量，各自的响应
/// 时间是重叠的。
pub fn prepare(conn: &rusqlite::Connection, song_id: &str, track: &TrackFile) -> Result<()> {
    let store = ScrapeStore::new(conn);
    store.ensure_schema()?;
    store.register(track, song_id)?;
    if !track.path.is_empty() {
        store.mark_running(&track.path)?;
    }
    Ok(())
}

/// 网络之后：落状态，够格的写进 `songs`。
pub fn finish(
    conn: &rusqlite::Connection,
    song_id: &str,
    track: &TrackFile,
    mut result: jp_scraper::ResolvedTrack,
) -> Result<TrackOutcome> {
    let store = ScrapeStore::new(conn);
    // resolve 用的 path 可能是空的（库里没有 audio_path），
    // 落库要有主键，退回用 song_id 造一个稳定的标识
    if result.file_path.is_empty() {
        result.file_path = format!("song:{song_id}");
    }
    store.record(&result, song_id)?;

    let mut accepted = None;
    let matched = match &result.candidate {
        Some(c) => {
            // 只有自动采纳的才写 songs 行和封面。需要人工确认的
            // 停在这里等复核——写错的元数据比空的更难发现。
            if result.status == ScrapeStatus::Success {
                apply_candidate(conn, song_id, c, Some(result.confidence))?;
                accepted = Some(c.clone());
            }
            format!("{} / {} / {}", c.title, c.artist, c.album)
        }
        None => String::new(),
    };
    Ok(TrackOutcome {
        status: result.status,
        confidence: result.confidence,
        matched,
        accepted,
        artist: track.artist().to_string(),
    })
}

/// 把候选写进 `songs`。
///
/// **只补空字段，不覆盖已有值**——需求「永远不要覆盖用户本地文件里的
/// 原始 metadata」。原值另有一份存在 `track_original_metadata` 里。
///
/// 补上的字段记下来源（provider）和置信度（自动采纳时的匹配分；你手动确认的是 `None`）。
fn apply_candidate(
    conn: &rusqlite::Connection,
    song_id: &str,
    candidate: &jp_scraper::ScrapeCandidate,
    confidence: Option<f64>,
) -> Result<usize> {
    // 先看哪几个字段是空的。
    //
    // **不能用 `execute` 的返回值当「填了几个」**：那是受影响的行数，
    // UPDATE 永远命中这一行，所以恒为 1，不管 CASE 里有没有真的改动。
    let (album, year, genre): (String, String, String) = conn.query_row(
        "SELECT COALESCE(album,''), COALESCE(year,''), COALESCE(genre,'')
         FROM songs WHERE id=?1",
        rusqlite::params![song_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let filled = [
        (album.is_empty(), &candidate.album),
        (year.is_empty(), &candidate.year),
        (genre.is_empty(), &candidate.genre),
    ]
    .iter()
    .filter(|(empty, value)| *empty && !value.is_empty())
    .count();
    let provenance = |field: &str| {
        jp_corpus::lineage::set_provenance(conn, "song", song_id, field, &candidate.provider, &candidate.provider_id, confidence)
    };
    for (field, empty, value) in [
        ("album", album.is_empty(), &candidate.album),
        ("year", year.is_empty(), &candidate.year),
        ("genre", genre.is_empty(), &candidate.genre),
    ] {
        if empty && !value.is_empty() {
            provenance(field)?;
        }
    }

    conn.execute(
        "UPDATE songs SET
            album = CASE WHEN COALESCE(album,'')='' THEN ?1 ELSE album END,
            year  = CASE WHEN COALESCE(year,'')=''  THEN ?2 ELSE year  END,
            genre = CASE WHEN COALESCE(genre,'')='' THEN ?3 ELSE genre END
         WHERE id=?4",
        rusqlite::params![candidate.album, candidate.year, candidate.genre, song_id],
    )
    .context("写 songs 失败")?;

    // 完整发行日期（时间轴按月份排要用）。同样只补空的；
    // 而且**年份对得上才补**：本地文件写的是 2019、候选说 2020-03-04，那这个日期不是这首歌的
    let date = &candidate.release_date;
    if !date.is_empty() {
        let written = conn
            .execute(
                "UPDATE songs SET release_date = ?1
                 WHERE id = ?2 AND COALESCE(release_date,'') = ''
                   AND (COALESCE(year,'') = '' OR substr(year,1,4) = substr(?1,1,4))",
                rusqlite::params![date, song_id],
            )
            .context("写发行日期失败")?;
        if written > 0 {
            provenance("release_date")?;
        }
    }
    Ok(filled)
}

/// 下封面到盘上。**不碰数据库**，所以能在任意线程里跑。
///
/// 走 [`jp_scraper::save_cover_any`]：一个候选的几档尺寸和几个源按顺序试，
/// **方的图优先**（CAA 上有的 release 传的是 16:9 的 MV 截图）。
///
/// 返回落盘路径；没有 URL 或全都下不下来时返回 None。
pub fn download_cover(
    covers_dir: &std::path::Path,
    song_id: &str,
    artist: &str,
    candidate: &jp_scraper::ScrapeCandidate,
) -> Option<String> {
    let urls = artwork_sizes(candidate);
    if urls.is_empty() {
        return None;
    }
    let dest = jp_scraper::cover_path_for(covers_dir, song_id, artist);
    let client = jp_scraper::HttpClient::new(Box::new(jp_scraper::UreqTransport))
        .with_provider_name("cover")
        // 图片比 JSON 大一个量级，超时要单独给
        .with_config(jp_scraper::HttpConfig::for_artwork());
    // overwrite=false：已经有图就不重下
    match jp_scraper::save_cover_any(&client, &urls, &dest, false) {
        Ok((index, _)) => {
            if index > 0 {
                crate::log::warn(format!("{song_id} 的封面退到了第 {} 档尺寸", index + 1));
            }
            Some(dest.display().to_string())
        }
        // 封面下不下来不该让整首歌算失败——元数据已经对上了
        Err(err) => {
            crate::log::warn(format!("封面下载失败 {song_id}: {err}"));
            None
        }
    }
}

/// 音频文件里内嵌的封面存成这首歌的封面文件，和下载来的放在同一个位置
/// （`raw/covers/{歌手}/{song_id}.jpg`），删歌、迁库、换封面的代码都不用分两种情况。
///
/// 那个位置已经有图就不覆盖（和 `download_cover` 的 overwrite=false 一样）。
/// PNG 也存成 `.jpg` 这个名字：下载那条路一直这样，webview 按内容认图，不看扩展名。
///
/// 返回落盘路径；文件里没有图、或者写不下去时返回 None。**不碰数据库**。
pub fn save_embedded_cover(
    covers_dir: &std::path::Path,
    song_id: &str,
    artist: &str,
    audio_path: &std::path::Path,
) -> Option<String> {
    let cover = jp_import::embedded::read_cover(audio_path)?;
    let dest = jp_scraper::cover_path_for(covers_dir, song_id, artist);
    if std::fs::metadata(&dest).is_ok_and(|m| m.len() > 0) {
        return Some(dest.display().to_string());
    }
    let written = dest
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| {
            // 先写临时文件再改名：写一半的图不会被当成封面
            let tmp = dest.with_extension("jpg.part");
            std::fs::write(&tmp, &cover.data)?;
            std::fs::rename(&tmp, &dest)
        });
    match written {
        Ok(()) => Some(dest.display().to_string()),
        Err(err) => {
            crate::log::warn(format!("内嵌封面写不下去 {song_id}: {err}"));
            None
        }
    }
}

/// 刚导入的歌里，文件内嵌了封面的，取出来记进库。返回取到了几张。
///
/// 导入本身一首歌一个事务、已经提交了，封面放在后面单独做：
/// 一张图写不下去不该让这首歌算导入失败。
pub fn save_imported_covers(state: &crate::state::AppState, report: &jp_import::ImportReport) -> usize {
    let corpus = state.corpus();
    let conn = corpus.connection();
    let mut saved = 0;
    for track in &report.tracks {
        if !matches!(track.outcome, jp_import::Outcome::Imported { .. }) {
            continue;
        }
        let Ok(artist) = conn.query_row(
            "SELECT artist FROM songs WHERE id=?1",
            rusqlite::params![track.song_id],
            |r| r.get::<_, String>(0),
        ) else {
            continue;
        };
        let audio = std::path::Path::new(&track.path);
        if let Some(path) = save_embedded_cover(&state.covers_dir, &track.song_id, &artist, audio) {
            match record_cover(conn, &track.song_id, &path) {
                Ok(()) => saved += 1,
                Err(err) => crate::log::warn(format!("封面记不进库 {}: {err:#}", track.song_id)),
            }
        }
    }
    saved
}

/// 把封面路径记进 `songs`。已经有值的不动。
pub fn record_cover(conn: &rusqlite::Connection, song_id: &str, path: &str) -> Result<()> {
    conn.execute(
        "UPDATE songs SET cover_path=?1 WHERE id=?2 AND COALESCE(cover_path,'')=''",
        rusqlite::params![path, song_id],
    )?;
    Ok(())
}

/// 一个在飞的元数据查询。
struct MetaJob {
    song_id: String,
    track: TrackFile,
    handle: std::thread::JoinHandle<jp_scraper::ResolvedTrack>,
}

/// 收下一个查完的元数据：落库，够格的派个线程去下封面。
///
/// 返回 (状态, 置信度, 采纳的候选, 是否派了封面下载)。
fn settle_meta(
    conn: &rusqlite::Connection,
    job: MetaJob,
    covers_dir: &std::path::Path,
    covers: &mut Vec<CoverHandle>,
) -> (ScrapeStatus, f64, String, bool) {
    let MetaJob {
        song_id,
        track,
        handle,
    } = job;
    // 查询线程 panic 也不该让整批停下
    let Ok(result) = handle.join() else {
        crate::log::warn(format!("{song_id} 的查询线程异常退出"));
        return (ScrapeStatus::Failed, 0.0, String::new(), false);
    };
    let outcome = match finish(conn, &song_id, &track, result) {
        Ok(v) => v,
        Err(err) => {
            crate::log::warn(format!("落库 {song_id} 失败: {err:#}"));
            return (ScrapeStatus::Failed, 0.0, String::new(), false);
        }
    };

    let mut dispatched = false;
    if let Some(candidate) = outcome.accepted {
        let dir = covers_dir.to_path_buf();
        let id = song_id.clone();
        let artist = outcome.artist.clone();
        covers.push(std::thread::spawn(move || {
            download_cover(&dir, &id, &artist, &candidate).map(|p| (id, p))
        }));
        dispatched = true;
    }
    (
        outcome.status,
        outcome.confidence,
        outcome.matched,
        dispatched,
    )
}

type CoverHandle = std::thread::JoinHandle<Option<(String, String)>>;

/// 收下已经完成的封面下载，写进 `songs`。
///
/// `block` 为真时等全部下完（作业收尾或被取消时用）；为假时只收
/// 已经结束的，不阻塞主循环。
fn drain_covers(conn: &rusqlite::Connection, in_flight: &mut Vec<CoverHandle>, block: bool) {
    if block {
        while !in_flight.is_empty() {
            drain_one_cover(conn, in_flight);
        }
        return;
    }
    // 只收已经结束的。`is_finished` 不阻塞。
    let mut index = 0;
    while index < in_flight.len() {
        if in_flight[index].is_finished() {
            let handle = in_flight.remove(index);
            finish_cover(conn, handle);
        } else {
            index += 1;
        }
    }
}

/// 等最早派出去的那个下完。用于把在飞的数量压回上限。
fn drain_one_cover(conn: &rusqlite::Connection, in_flight: &mut Vec<CoverHandle>) {
    if in_flight.is_empty() {
        return;
    }
    finish_cover(conn, in_flight.remove(0));
}

fn finish_cover(conn: &rusqlite::Connection, handle: CoverHandle) {
    // 下载线程 panic 也不该让整批停下
    match handle.join() {
        Ok(Some((song_id, path))) => {
            if let Err(err) = record_cover(conn, &song_id, &path) {
                crate::log::warn(format!("记录封面路径失败 {song_id}: {err:#}"));
            }
        }
        Ok(None) => {}
        Err(_) => crate::log::warn("封面下载线程异常退出"),
    }
}

/// 封面下载的尺寸阶梯，由大到小；第一档下不来就退下一档。
///
/// **第一档是 600。** 以前是 300，理由是「界面上封面最大只有 132px」——
/// 那条理由已经不成立了：曲库的网格视图一格最宽 220px、全屏歌词右侧那张更大，
/// 2× HiDPI 下 300 的图已经能看出糊。
///
/// 阶梯本身照旧：600 下不来退 300（约 1/4 数据量），再退 200、100，
/// **拿到一张小图也比一张都没有强**。MusicBrainz 那边会归到 CAA 的 500 档，
/// 见 `jp_scraper::artwork_urls`。
const ARTWORK_SIZES: &[u32] = &[600, 300, 200, 100];

/// 一条候选的封面 URL，由大到小。
///
/// iTunes 的 CDN 把尺寸写在路径里（`/600x600bb.jpg`），换个数字就是
/// 另一档分辨率，不用再发一次查询。
fn artwork_sizes(candidate: &jp_scraper::ScrapeCandidate) -> Vec<String> {
    jp_scraper::artwork_urls(candidate, ARTWORK_SIZES)
}

/// 在后台线程刮一批。立刻返回，进度走 `scrape://progress` 事件。
pub fn spawn_batch<R: Runtime>(
    app: AppHandle<R>,
    job: Arc<ScrapeJob>,
    db_path: std::path::PathBuf,
    covers_dir: std::path::PathBuf,
    song_ids: Vec<String>,
) -> Result<()> {
    // 线程崩了也要让界面收一条终态（见 `crate::job`）
    let crashed = app.clone();
    let guard = job
        .start_with(move || {
            let _ = crashed.emit(
                "scrape://progress",
                ScrapeProgress {
                    kind: "track".into(),
                    finished: true,
                    error: "刮削的后台线程崩了，详情见日志".into(),
                    ..Default::default()
                },
            );
        })
        .ok_or_else(|| anyhow::anyhow!("已经有一个刮削作业在跑"))?;

    std::thread::spawn(move || {
        let total = song_ids.len();
        let outcome = (|| -> Result<bool> {
            // 自己开一条连接：占着 UI 那条十几分钟的话，期间所有查询都会卡住
            let conn = rusqlite::Connection::open(&db_path)?;
            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
            let resolver = jp_scraper::default_resolver(false)?;

            // 两级并发。都是量出来的，不是猜的：
            //
            // * 元数据：串行 5.60 秒/首，4 路并发 1.88 秒/首（3.0×）。
            //   MusicBrainz 限的是两次请求的**间隔**不是并发数，
            //   各自的响应时间是重叠的。
            // * 封面：串行 8.0 KB/s，4 路并发 24.2 KB/s（3.0×）。
            //   慢的是单连接被限速，不是总带宽。
            //
            // 整库 209 首：串行近两小时 → 现在约 10 分钟。
            //
            // **数据库只有主循环碰。** `resolve` 和图片下载都是纯网络，
            // 不需要连接；工作线程把结果交回来，主循环再写库。
            const MAX_META_IN_FLIGHT: usize = 4;
            const MAX_COVERS_IN_FLIGHT: usize = 4;
            let resolver = std::sync::Arc::new(resolver);
            let mut covers: Vec<CoverHandle> = Vec::new();
            let mut metas: std::collections::VecDeque<MetaJob> = std::collections::VecDeque::new();
            let mut next = 0usize;
            let mut done = 0usize;

            loop {
                if job.cancelled() {
                    // 取消时把已经拿到的收干净，不然白跑了
                    while let Some(pending) = metas.pop_front() {
                        settle_meta(&conn, pending, &covers_dir, &mut covers);
                    }
                    drain_covers(&conn, &mut covers, true);
                    return Ok(true);
                }

                // 派单：把在飞的元数据查询补满
                while metas.len() < MAX_META_IN_FLIGHT && next < song_ids.len() {
                    let song_id = song_ids[next].clone();
                    next += 1;
                    let Some(track) = track_file_for(&conn, &song_id)? else {
                        continue;
                    };
                    // 登记和置 running 在主线程做，网络那步才并发
                    if let Err(err) = prepare(&conn, &song_id, &track) {
                        crate::log::warn(format!("登记 {song_id} 失败: {err:#}"));
                        continue;
                    }
                    let resolver = resolver.clone();
                    let for_thread = track.clone();
                    metas.push_back(MetaJob {
                        song_id,
                        track,
                        handle: std::thread::spawn(move || resolver.resolve(&for_thread)),
                    });
                }

                let Some(pending) = metas.pop_front() else {
                    break; // 派完了也收完了
                };
                let song_id = pending.song_id.clone();
                let title = pending.track.title().to_string();
                let (status, confidence, matched, cover_saved) =
                    settle_meta(&conn, pending, &covers_dir, &mut covers);
                done += 1;

                // 封面在飞的太多就等一等，别把内存和连接数撑爆
                while covers.len() > MAX_COVERS_IN_FLIGHT {
                    drain_one_cover(&conn, &mut covers);
                }
                drain_covers(&conn, &mut covers, false);

                let _ = app.emit(
                    "scrape://progress",
                    ScrapeProgress {
                        kind: "track".into(),
                        done,
                        total,
                        song_id,
                        title,
                        status: status.as_str().to_string(),
                        confidence,
                        matched,
                        cover_saved,
                        finished: false,
                        cancelled: false,
                        error: String::new(),
                    },
                );
            }

            // 循环结束，等剩下的封面下完
            drain_covers(&conn, &mut covers, true);
            Ok(false)
        })();

        let (cancelled, error) = match outcome {
            Ok(cancelled) => (cancelled, String::new()),
            Err(err) => {
                crate::log::error(format!("刮削作业中断: {err:#}"));
                // 整批断了要**说出来**，不能只是悄悄「完成」
                (false, format!("{err:#}"))
            }
        };
        drop(guard);
        let _ = app.emit(
            "scrape://progress",
            ScrapeProgress {
                kind: "track".into(),
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

// ────────────────────────── 歌手照片与资料 ──────────────────────────

/// 一个歌手刮下来的结果。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistOutcome {
    pub name: String,
    /// MusicBrainz 认出来的规范名，没认出来时是空串
    pub canonical_name: String,
    pub artist_type: String,
    pub country: String,
    pub formed: String,
    /// 乐队解散 / 个人去世的日期，可能只有年份
    pub ended: String,
    pub has_ended: bool,
    /// MusicBrainz 的编号。同名有好几个条目时不认，这里就是空的
    pub mbid: String,
    pub image_path: String,
    /// 这一轮从 MusicBrainz 写进去的成员关系条数（被你手动改过、否掉过的不算）
    pub memberships: usize,
    /// 一条都没查到
    pub not_found: bool,
}

/// 刮一个歌手：MusicBrainz 拿资料和别名，Deezer 拿照片。
///
/// **顺序不能反。** 大量日本歌手在 Deezer 上按罗马字收录
/// （ずっと真夜中でいいのに。→ ZUTOMAYO），不先从 MusicBrainz 拿到别名
/// 就基本找不到照片。
pub fn scrape_artist_one(
    conn: &rusqlite::Connection,
    artists_dir: &std::path::Path,
    name: &str,
) -> Result<ArtistOutcome> {
    use jp_scraper::MetadataProvider;

    let (mb, deezer) = jp_scraper::artist_providers();
    let mut out = ArtistOutcome {
        name: name.to_string(),
        ..Default::default()
    };

    // 一边查不到不该让另一边也白费——分别处理错误
    let facts = match mb.get_artist(name, &[]) {
        Ok(v) => v,
        Err(err) => {
            crate::log::warn(format!("MusicBrainz 查 {name} 失败: {err}"));
            None
        }
    };
    let aliases = if let Some(f) = &facts {
        out.canonical_name = f.name.clone();
        out.artist_type = f.artist_type.clone();
        out.country = f.country.clone();
        out.formed = f.formed.clone();
        out.ended = f.ended.clone();
        out.has_ended = f.has_ended;
        out.mbid = f.provider_id.clone();
        let mut names = f.aliases.clone();
        if !f.name.is_empty() {
            names.push(f.name.clone());
        }
        names
    } else {
        Vec::new()
    };

    match deezer.get_artist(name, &aliases) {
        Ok(Some(photo)) if !photo.image_url.is_empty() => {
            let dest = jp_scraper::artist_image_path_for(artists_dir, name);
            let client = jp_scraper::HttpClient::new(Box::new(jp_scraper::UreqTransport))
                .with_provider_name("artist-image")
                .with_config(jp_scraper::HttpConfig::for_artwork());
            // overwrite=false：已经有图就不重下
            match jp_scraper::save_image(&client, &photo.image_url, &dest, false) {
                Ok(_) => out.image_path = dest.display().to_string(),
                Err(err) => crate::log::warn(format!("{name} 的照片下载失败: {err}")),
            }
        }
        Ok(_) => {}
        Err(err) => crate::log::warn(format!("Deezer 查 {name} 失败: {err}")),
    }

    out.not_found = facts.is_none() && out.image_path.is_empty();
    if !out.not_found {
        upsert_artist(conn, &out)?;
        record_artist_provenance(conn, &out)?;
    }

    // 成员关系：只在唯一认出了这个条目（有 MBID）时才查，多一次请求
    if !out.mbid.is_empty() {
        match mb.relations(&out.mbid) {
            Ok(rel) => match apply_relations(conn, name, &rel) {
                Ok(n) => out.memberships = n,
                Err(err) => crate::log::warn(format!("写 {name} 的成员关系失败: {err:#}")),
            },
            Err(err) => crate::log::warn(format!("MusicBrainz 查 {name} 的成员失败: {err}")),
        }
    }
    Ok(out)
}

/// 把 MusicBrainz 的成员关系写进 `band_members`（来源 musicbrainz）。
///
/// * 查的是乐队：每个成员都写，成员不在库里就按名字建一个人（他可能没署过名，比如鼓手）；
/// * 查的是个人：只写**库里已经有的**乐队——没在库里出现过的乐队建出来也是空壳。
///
/// 你手动加过、确认过、否掉过的，[`jp_corpus::members::put`] 会挡下，这里不用管。
/// 返回实际写进去（新增或更新）的条数。
pub fn apply_relations(
    conn: &rusqlite::Connection,
    name: &str,
    rel: &jp_scraper::providers::musicbrainz::ArtistRelations,
) -> Result<usize> {
    use jp_corpus::members::{PutOutcome, ensure_person, put};
    use rusqlite::OptionalExtension;

    let me = ensure_person(conn, name)?;
    let mut written = 0;
    let mut count = |outcome: PutOutcome| {
        if matches!(outcome, PutOutcome::Inserted | PutOutcome::Updated) {
            written += 1;
        }
    };
    for m in &rel.members {
        if m.name.trim().is_empty() {
            continue;
        }
        let member = ensure_person(conn, &m.name)?;
        if member != me {
            count(put(conn, me, member, &m.part, &m.joined, &m.left_at, "musicbrainz")?);
        }
    }
    for b in &rel.member_of {
        let key = jp_normalize::matching_key(&b.name);
        let band: Option<i64> = conn
            .query_row("SELECT id FROM people WHERE normalized_name=?1", rusqlite::params![key], |r| r.get(0))
            .optional()?;
        if let Some(band) = band.filter(|b| *b != me) {
            count(put(conn, band, me, &b.part, &b.joined, &b.left_at, "musicbrainz")?);
        }
    }
    Ok(written)
}

/// MusicBrainz 资料的置信度先验：名字或别名归一化后相等、而且**只有一个条目**（拿到了 MBID）
/// 是 1.0；同名不止一个、没敢认 MBID 时是 0.5（资料可能是同名的另一位的）。
pub const MB_UNIQUE_CONFIDENCE: f64 = 1.0;
pub const MB_AMBIGUOUS_CONFIDENCE: f64 = 0.5;
/// Deezer 照片的置信度先验：名字或别名相等才收，但 Deezer 上不少乐队的「照片」其实是专辑封面
pub const DEEZER_IMAGE_CONFIDENCE: f64 = 0.7;

/// 刮到的歌手资料各字段记下来源和置信度（`field_provenance`，entity_type = artist，id 是名字）。
/// 只记这一轮真的拿到值的字段；照片是你手动换的就不记（它没被覆盖）。
fn record_artist_provenance(conn: &rusqlite::Connection, out: &ArtistOutcome) -> Result<()> {
    use jp_corpus::lineage::set_provenance;
    let mb_conf = if out.mbid.is_empty() { MB_AMBIGUOUS_CONFIDENCE } else { MB_UNIQUE_CONFIDENCE };
    for (field, value) in [
        ("artist_type", &out.artist_type),
        ("country", &out.country),
        ("formed", &out.formed),
        ("ended", &out.ended),
        ("mbid", &out.mbid),
    ] {
        if !value.is_empty() {
            set_provenance(conn, "artist", &out.name, field, "musicbrainz", &out.mbid, Some(mb_conf))?;
        }
    }
    if !out.image_path.is_empty() {
        let manual: bool = conn
            .query_row("SELECT COALESCE(image_manual,0)=1 FROM artists WHERE name=?1", rusqlite::params![out.name], |r| r.get(0))
            .unwrap_or(false);
        if !manual {
            set_provenance(conn, "artist", &out.name, "image_path", "deezer", "", Some(DEEZER_IMAGE_CONFIDENCE))?;
        }
    }
    Ok(())
}

/// 写 `artists` 表。**空值不覆盖已有值**——这一轮没查到照片，
/// 不该把上一轮拿到的抹掉。
fn upsert_artist(conn: &rusqlite::Connection, out: &ArtistOutcome) -> Result<()> {
    conn.execute(
        "INSERT INTO artists (name, image_path, artist_type, country, formed, ended, has_ended, mbid, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,datetime('now'))
         ON CONFLICT(name) DO UPDATE SET
            -- 照片是你手动换的：刮削不动它
            image_path  = CASE WHEN COALESCE(image_manual, 0) = 1 THEN image_path
                               ELSE COALESCE(NULLIF(excluded.image_path, ''), image_path) END,
            artist_type = COALESCE(NULLIF(excluded.artist_type, ''), artist_type),
            country     = COALESCE(NULLIF(excluded.country, ''),     country),
            formed      = COALESCE(NULLIF(excluded.formed, ''),      formed),
            ended       = COALESCE(NULLIF(excluded.ended, ''),       ended),
            has_ended   = MAX(COALESCE(has_ended, 0), excluded.has_ended),
            mbid        = COALESCE(NULLIF(excluded.mbid, ''),        mbid),
            updated_at  = excluded.updated_at",
        rusqlite::params![
            out.name,
            out.image_path,
            out.artist_type,
            out.country,
            out.formed,
            out.ended,
            out.has_ended,
            out.mbid
        ],
    )
    .context("写 artists 失败")?;
    Ok(())
}

// ────────────────────────────── 手动换照片 ──────────────────────────────
//
// 刮削有时会把专辑封面当成歌手照片（Deezer 上很多乐队就是这样）。你换的照片单独存一份
// `<名字>.manual-<时间>.<扩展名>`，`image_manual = 1`，之后刮削不再覆盖；恢复时删掉它，
// 用回刮削下的那张（没有就空着）。文件名带时间，换第二次时 webview 不会拿缓存里的旧图。

fn manual_prefix(name: &str) -> String {
    format!("{}.manual-", jp_scraper::safe_name(name))
}

fn remove_manual_images(artists_dir: &std::path::Path, name: &str) {
    let prefix = manual_prefix(name);
    if let Ok(entries) = std::fs::read_dir(artists_dir) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// 用你选的图片当这位歌手的照片。返回存下来的路径。
pub fn set_artist_image(
    conn: &rusqlite::Connection,
    artists_dir: &std::path::Path,
    name: &str,
    source: &std::path::Path,
) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("没有歌手名");
    }
    let data = std::fs::read(source).with_context(|| format!("读不了 {}", source.display()))?;
    let kind = jp_scraper::sniff_image(&data).context("这不是 JPG、PNG 或 WebP 图片")?;
    std::fs::create_dir_all(artists_dir).context("建不了歌手照片目录")?;
    remove_manual_images(artists_dir, name);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dest = artists_dir.join(format!("{}{stamp}.{}", manual_prefix(name), kind.extension()));
    std::fs::write(&dest, &data).with_context(|| format!("写不了 {}", dest.display()))?;
    let path = dest.display().to_string();
    conn.execute(
        "INSERT INTO artists (name, image_path, image_manual, updated_at) VALUES (?1, ?2, 1, datetime('now'))
         ON CONFLICT(name) DO UPDATE SET image_path = excluded.image_path, image_manual = 1, updated_at = excluded.updated_at",
        rusqlite::params![name, path],
    )
    .context("写 artists 失败")?;
    jp_corpus::lineage::set_provenance(conn, "artist", name, "image_path", "manual", "", None)?;
    Ok(path)
}

/// 不要手动换的那张了：用回刮削下的照片（没有就空着），之后刮削又能更新它。返回现在的路径。
pub fn reset_artist_image(conn: &rusqlite::Connection, artists_dir: &std::path::Path, name: &str) -> Result<String> {
    remove_manual_images(artists_dir, name);
    let scraped = jp_scraper::artist_image_path_for(artists_dir, name);
    let path = if scraped.is_file() { scraped.display().to_string() } else { String::new() };
    conn.execute(
        "UPDATE artists SET image_path = ?2, image_manual = 0, updated_at = datetime('now') WHERE name = ?1",
        rusqlite::params![name, path],
    )
    .context("写 artists 失败")?;
    if path.is_empty() {
        conn.execute(
            "DELETE FROM field_provenance WHERE entity_type='artist' AND entity_id=?1 AND field='image_path'",
            rusqlite::params![name],
        )?;
    } else {
        jp_corpus::lineage::set_provenance(conn, "artist", name, "image_path", "deezer", "", Some(DEEZER_IMAGE_CONFIDENCE))?;
    }
    Ok(path)
}

/// 库里出现过的歌手，按斜杠拆开去重。
///
/// `only_missing` 为真时只返回**还没有照片、或者还没认出 MusicBrainz 条目**的——重跑幂等。
pub fn library_artists(conn: &rusqlite::Connection, only_missing: bool) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT artist FROM songs WHERE COALESCE(artist,'')<>''")?;
    let raw: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<Result<Vec<String>, _>>()?;

    // 合作曲写成「A/B」，两个人都要刮
    let mut names: Vec<String> = Vec::new();
    for entry in raw {
        for part in entry.split(['/', '／']) {
            let part = part.trim();
            if !part.is_empty() && !names.iter().any(|n| n == part) {
                names.push(part.to_string());
            }
        }
    }
    if !only_missing {
        names.sort();
        return Ok(names);
    }

    let mut have = std::collections::BTreeSet::new();
    // 「齐了」= 有照片、也认出了 MusicBrainz 条目。缺 MBID 的再查一次，成员关系是跟着 MBID 拿的
    let mut stmt = conn.prepare(
        "SELECT name FROM artists WHERE COALESCE(image_path,'')<>'' AND COALESCE(mbid,'')<>''",
    )?;
    for row in stmt.query_map([], |r| r.get::<_, String>(0))? {
        have.insert(row?);
    }
    names.retain(|n| !have.contains(n));
    names.sort();
    Ok(names)
}

/// 后台刮一批歌手。和曲目共用同一个作业标志——两边都要排 MusicBrainz
/// 的队，同时跑只会互相拖慢。
pub fn spawn_artist_batch<R: Runtime>(
    app: AppHandle<R>,
    job: Arc<ScrapeJob>,
    db_path: std::path::PathBuf,
    artists_dir: std::path::PathBuf,
    names: Vec<String>,
) -> Result<()> {
    // 线程崩了也要让界面收一条终态（见 `crate::job`）
    let crashed = app.clone();
    let guard = job
        .start_with(move || {
            let _ = crashed.emit(
                "scrape://progress",
                ScrapeProgress {
                    kind: "artist".into(),
                    finished: true,
                    error: "刮削的后台线程崩了，详情见日志".into(),
                    ..Default::default()
                },
            );
        })
        .ok_or_else(|| anyhow::anyhow!("已经有一个刮削作业在跑"))?;

    std::thread::spawn(move || {
        let total = names.len();
        let outcome = (|| -> Result<bool> {
            let conn = rusqlite::Connection::open(&db_path)?;
            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
            for (index, name) in names.iter().enumerate() {
                if job.cancelled() {
                    return Ok(true);
                }
                let got = scrape_artist_one(&conn, &artists_dir, name).unwrap_or_else(|err| {
                    crate::log::warn(format!("刮歌手 {name} 失败: {err:#}"));
                    ArtistOutcome {
                        name: name.clone(),
                        not_found: true,
                        ..Default::default()
                    }
                });
                let _ = app.emit(
                    "scrape://progress",
                    ScrapeProgress {
                        kind: "artist".into(),
                        done: index + 1,
                        total,
                        song_id: String::new(),
                        title: name.clone(),
                        status: if got.not_found { "failed" } else { "success" }.into(),
                        confidence: 0.0,
                        matched: if got.canonical_name.is_empty() {
                            String::new()
                        } else {
                            format!(
                                "{} · {} · {}",
                                got.canonical_name, got.artist_type, got.country
                            )
                        },
                        cover_saved: !got.image_path.is_empty(),
                        finished: false,
                        cancelled: false,
                        error: String::new(),
                    },
                );
            }
            Ok(false)
        })();

        let (cancelled, error) = match outcome {
            Ok(cancelled) => (cancelled, String::new()),
            Err(err) => {
                crate::log::error(format!("歌手刮削作业中断: {err:#}"));
                (false, format!("{err:#}"))
            }
        };
        drop(guard);
        let _ = app.emit(
            "scrape://progress",
            ScrapeProgress {
                kind: "artist".into(),
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

// ────────────────────────── 专辑封面 ──────────────────────────

/// 用曲目封面填 `albums.artwork_path`。
///
/// 不额外发网络请求：同一张专辑的曲目封面就是这张专辑的封面
/// （iTunes 给的本来就是专辑图）。取该专辑里最小 song_id 的那张，
/// 结果稳定、可重跑。
///
/// 返回填了几张。**已经有图的不动**。
pub fn fill_album_artwork(conn: &rusqlite::Connection) -> Result<usize> {
    let changed = conn.execute(
        "UPDATE albums SET artwork_path = (
             SELECT s.cover_path FROM songs s
             WHERE s.album_id = albums.id AND COALESCE(s.cover_path,'') <> ''
             ORDER BY s.id LIMIT 1
         )
         WHERE COALESCE(artwork_path,'') = ''
           AND EXISTS (
             SELECT 1 FROM songs s
             WHERE s.album_id = albums.id AND COALESCE(s.cover_path,'') <> ''
           )",
        [],
    )?;
    Ok(changed)
}

/// 补封面的结果
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverBackfill {
    /// 检查了几首（库里没有封面的）
    pub checked: usize,
    /// 这次补上了几首
    pub filled: usize,
    /// 刮削记录里连候选都没有，没法补
    pub no_candidate: usize,
    /// 有候选但图还是下不下来
    pub failed: usize,
    /// 前几条说明，界面上显示
    pub samples: Vec<String>,
}

/// 给没有封面的歌补封面。**不重新搜索**：直接用刮削时存下来的候选。
///
/// 为什么需要它：识别成功但没有封面是常事——MusicBrainz 赢下匹配时，
/// 封面在 Cover Art Archive 而不是 MusicBrainz 本身，老代码没去取；
/// iTunes 候选有图却分数低，也用不上。现在
/// [`jp_scraper::artwork_urls`] 会把 CAA 的 release / release-group 地址都排进去，
/// 这里按顺序试：先试赢下匹配的那个候选，再试**同一位歌手、分数够高、自带封面**的其他候选。
///
/// 在这之前先看音频文件自己有没有内嵌封面，有就直接用（见 [`save_embedded_cover`]）。
///
/// 只补空的，已经有封面的一律不动；`dry_run` 时只下到 `covers_dir`（给试跑用），不写库。
pub fn backfill_covers(
    conn: &rusqlite::Connection,
    covers_dir: &std::path::Path,
    dry_run: bool,
) -> Result<CoverBackfill> {
    let mut out = CoverBackfill::default();
    let mut stmt = conn.prepare(
        "SELECT s.id, s.artist, COALESCE(st.provider_id,''), COALESCE(st.candidates_json,''),
                COALESCE(s.audio_path,'')
         FROM songs s LEFT JOIN scrape_state st ON st.song_id = s.id
         WHERE COALESCE(s.cover_path,'') = '' ORDER BY s.id",
    )?;
    let rows: Vec<(String, String, String, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    for (song_id, artist, provider_id, candidates_json, audio_path) in rows {
        out.checked += 1;
        // 文件里自带封面的先用它：不用联网，也不会配错图。
        // 当初导入时还不认内嵌封面的那些歌，靠这一步补上。
        if !audio_path.is_empty()
            && let Some(path) =
                save_embedded_cover(covers_dir, &song_id, &artist, std::path::Path::new(&audio_path))
        {
            out.filled += 1;
            if !dry_run {
                record_cover(conn, &song_id, &path)?;
            }
            if out.samples.len() < 8 {
                out.samples.push(format!("{song_id}：文件内嵌的封面"));
            }
            continue;
        }
        let candidates: Vec<jp_scraper::ScrapeCandidate> =
            serde_json::from_str(&candidates_json).unwrap_or_default();
        let ordered = cover_candidates(&candidates, &provider_id);
        if ordered.is_empty() {
            out.no_candidate += 1;
            if out.samples.len() < 8 {
                out.samples
                    .push(format!("{song_id}：刮削记录里没有候选，重刮一次再来"));
            }
            continue;
        }
        let mut done = None;
        for candidate in ordered {
            if let Some(path) = download_cover(covers_dir, &song_id, &artist, candidate) {
                done = Some((path, candidate));
                break;
            }
        }
        match done {
            Some((path, candidate)) => {
                out.filled += 1;
                if !dry_run {
                    record_cover(conn, &song_id, &path)?;
                }
                if out.samples.len() < 8 {
                    out.samples.push(format!(
                        "{song_id}：{}（{}）",
                        if candidate.album.is_empty() {
                            candidate.title.clone()
                        } else {
                            candidate.album.clone()
                        },
                        candidate.provider
                    ));
                }
            }
            None => {
                out.failed += 1;
                if out.samples.len() < 8 {
                    out.samples
                        .push(format!("{song_id}：候选都没有能下下来的图"));
                }
            }
        }
    }
    Ok(out)
}

/// 补封面时按什么顺序试候选：赢下匹配的那个排第一，然后是同一位歌手里分数够高、自带封面的。
///
/// 不同歌手的不要——同名歌太多，拿错封面比没有封面更糟。
fn cover_candidates<'a>(
    candidates: &'a [jp_scraper::ScrapeCandidate],
    winner_id: &str,
) -> Vec<&'a jp_scraper::ScrapeCandidate> {
    let winner = candidates.iter().find(|c| c.provider_id == winner_id);
    // 用刮削那套歌手比对：合作曲写成「A/B」、provider 只写主唱把另一位塞进 feat.，都算同一位。
    // 直接比字符串的话，「ずっと真夜中でいいのに。 feat. Mori Calliope」和「ずっと真夜中でいいのに。」
    // 会被判成两位歌手，明明有图的候选就用不上了
    let same_artist = |c: &jp_scraper::ScrapeCandidate| match winner {
        Some(w) => {
            jp_scraper::matching::artist_similarity(
                &jp_normalize::normalize_artist(&w.artist),
                &jp_normalize::normalize_artist(&c.artist),
            ) >= 0.9
        }
        None => true,
    };
    let mut out: Vec<&jp_scraper::ScrapeCandidate> = Vec::new();
    out.extend(winner);
    let mut others: Vec<&jp_scraper::ScrapeCandidate> = candidates
        .iter()
        .filter(|c| {
            c.provider_id != winner_id && c.score() >= COVER_FALLBACK_SCORE && same_artist(c)
        })
        .collect();
    others.sort_by(|a, b| b.score().total_cmp(&a.score()));
    out.extend(others);
    out
}

/// 拿别的候选的封面时的分数下限。低于这个宁可没有封面
const COVER_FALLBACK_SCORE: f64 = 0.75;

// ────────────────────────── 人工确认 ──────────────────────────

/// 用户在复核界面选定一条候选之后要做的事。
///
/// **这一步之前是空的**：只把 `scrape_state` 置成 success，
/// `songs` 一个字没写、封面也没下——点「采用」等于什么都没发生。
///
/// 现在做三件：
///
/// 1. 把候选的 metadata 补进 `songs`（**只补空字段**）
/// 2. 把这首歌的信用标成 `source='manual'`
/// 3. 下封面
///
/// 第 2 条的意思：`source='manual'` 在这套 schema 里就是「人工版本，
/// 自动流程不许动」（`add_credit` 和 Python 的回填都认这个）。
/// 用户是看着本地值和候选并排比过才选的，这就是人工版本。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualApply {
    /// 补进 songs 的字段数
    pub fields_filled: usize,
    /// 标成 manual 的信用条数
    pub credits_protected: usize,
    /// 新加的信用条数（候选里的演唱者本来没有信用记录）
    pub credits_added: usize,
    pub cover_path: Option<String>,
}

pub fn apply_manual(
    conn: &rusqlite::Connection,
    covers_dir: &std::path::Path,
    song_id: &str,
    local_artist: &str,
    candidate: &jp_scraper::ScrapeCandidate,
) -> Result<ManualApply> {
    // 你在复核里点的「就是这条」：置信度不适用（NULL），来源仍记 provider
    let fields_filled = apply_candidate(conn, song_id, candidate, None)?;

    // 已有的信用标成 manual：自动流程从此不许动它们
    let credits_protected = conn.execute(
        "UPDATE track_credits SET source='manual'
         WHERE song_id=?1 AND source <> 'manual'",
        rusqlite::params![song_id],
    )?;

    // 候选里的演唱者如果还没有信用记录，补一条 manual 的。
    // add_credit 已经存在时返回 0，不会重复也不会覆盖。
    let mut credits_added = 0usize;
    for (position, name) in split_slash(&candidate.artist).into_iter().enumerate() {
        if let Some(person_id) = jp_import::get_or_create_person(conn, &name)? {
            credits_added +=
                jp_import::add_credit(conn, song_id, person_id, "performer", position, "manual")?;
        }
    }

    // 封面用本地歌手名建目录，和自动流程一致——否则同一首歌
    // 会在两个目录下各有一张图
    let cover_path = download_cover(covers_dir, song_id, local_artist, candidate);
    if let Some(path) = &cover_path {
        record_cover(conn, song_id, path)?;
    }

    Ok(ManualApply {
        fields_filled,
        credits_protected,
        credits_added,
        cover_path,
    })
}

/// 按斜杠拆歌手串。和 `jp-import` 的做法一致——
/// 顿号和 × 会误伤乐队名（「Mrs. GREEN APPLE」这类）。
fn split_slash(name: &str) -> Vec<String> {
    let parts: Vec<String> = name
        .split(['/', '／'])
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            Vec::new()
        } else {
            vec![trimmed.to_string()]
        }
    } else {
        parts
    }
}

#[cfg(test)]
mod tests {
    /// 手动换的照片刮削不覆盖；恢复后用回刮削下的那张，刮削又能更新它
    #[test]
    fn a_hand_picked_artist_photo_survives_scraping_until_reset() {
        let dir = std::env::temp_dir().join(format!("jp-app-artist-photo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("logo.png");
        std::fs::write(&src, b"\x89PNG\r\n\x1a\nrest").unwrap();
        let scraped = jp_scraper::artist_image_path_for(&dir, "ヨルシカ");
        std::fs::write(&scraped, [0xff, 0xd8, 0xff, 0]).unwrap();

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        jp_corpus::Corpus::ensure_schema_on(&conn).unwrap();
        let image = |c: &rusqlite::Connection| -> String {
            c.query_row("SELECT image_path FROM artists WHERE name = 'ヨルシカ'", [], |r| r.get(0)).unwrap()
        };

        let manual = super::set_artist_image(&conn, &dir, "ヨルシカ", &src).unwrap();
        assert!(manual.ends_with(".png") && manual.contains(".manual-"), "{manual}");
        assert_eq!(image(&conn), manual);

        let out = super::ArtistOutcome {
            name: "ヨルシカ".into(),
            image_path: scraped.display().to_string(),
            ..Default::default()
        };
        super::upsert_artist(&conn, &out).unwrap();
        super::record_artist_provenance(&conn, &out).unwrap();
        assert_eq!(image(&conn), manual, "刮削把手动换的照片盖掉了");
        let source = |c: &rusqlite::Connection| jp_corpus::lineage::source_of(c, "artist", "ヨルシカ", "image_path").unwrap();
        assert_eq!(source(&conn).as_deref(), Some("manual"), "照片来源还是你换的那张");

        // 不是图片的文件不收
        let txt = dir.join("note.txt");
        std::fs::write(&txt, "hello").unwrap();
        assert!(super::set_artist_image(&conn, &dir, "ヨルシカ", &txt).is_err());

        assert_eq!(super::reset_artist_image(&conn, &dir, "ヨルシカ").unwrap(), scraped.display().to_string());
        assert!(!std::path::Path::new(&manual).exists());
        assert_eq!(source(&conn).as_deref(), Some("deezer"));
        super::upsert_artist(&conn, &out).unwrap();
        assert_eq!(image(&conn), scraped.display().to_string());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 刮到的歌手资料记来源和置信度：认出唯一条目的比同名好几个的可信
    #[test]
    fn scraped_artist_facts_carry_their_source_and_confidence() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        jp_corpus::Corpus::ensure_schema_on(&conn).unwrap();
        let unique = super::ArtistOutcome {
            name: "ヨルシカ".into(),
            artist_type: "Group".into(),
            country: "JP".into(),
            mbid: "mbid-1".into(),
            ..Default::default()
        };
        super::upsert_artist(&conn, &unique).unwrap();
        super::record_artist_provenance(&conn, &unique).unwrap();
        let prov = jp_corpus::lineage::provenance(&conn, "artist", "ヨルシカ").unwrap();
        let fields: Vec<&str> = prov.iter().map(|p| p.field.as_str()).collect();
        assert_eq!(fields, ["artist_type", "country", "mbid"], "空字段不记");
        assert!(prov.iter().all(|p| p.source == "musicbrainz" && p.confidence == Some(super::MB_UNIQUE_CONFIDENCE)));

        let ambiguous = super::ArtistOutcome { name: "同名".into(), country: "JP".into(), ..Default::default() };
        super::upsert_artist(&conn, &ambiguous).unwrap();
        super::record_artist_provenance(&conn, &ambiguous).unwrap();
        let prov = jp_corpus::lineage::provenance(&conn, "artist", "同名").unwrap();
        assert_eq!(prov[0].confidence, Some(super::MB_AMBIGUOUS_CONFIDENCE));
    }

    /// 文件里自带封面的，补封面时先用它，不看刮削候选（这首歌根本没刮削过）；
    /// 已经有图的位置不覆盖，第二次跑已经没有要补的
    #[test]
    fn backfill_takes_the_cover_embedded_in_the_audio_first() {
        use lofty::file::TaggedFileExt;
        use lofty::tag::TagExt;

        let dir = std::env::temp_dir().join(format!("jp-app-embedded-cover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let audio = dir.join("song.flac");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/formats/tone.flac"),
            &audio,
        )
        .unwrap();
        let picture = vec![0xff, 0xd8, 0xff, 0xe0, 1, 2, 3, 4];
        let tagged = lofty::probe::Probe::open(&audio).unwrap().read().unwrap();
        let mut tag = tagged
            .primary_tag()
            .cloned()
            .unwrap_or_else(|| lofty::tag::Tag::new(tagged.primary_tag_type()));
        tag.push_picture(
            lofty::picture::Picture::unchecked(picture.clone())
                .pic_type(lofty::picture::PictureType::CoverFront)
                .build(),
        );
        tag.save_to_path(&audio, lofty::config::WriteOptions::default()).unwrap();

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        jp_corpus::Corpus::ensure_schema_on(&conn).unwrap();
        conn.execute(
            "INSERT INTO songs (id, title, artist, audio_path) VALUES ('001', '曲', '歌手', ?1)",
            [audio.display().to_string()],
        )
        .unwrap();
        // 一首没有内嵌图、也没刮削过的：照旧报「没有候选」，不受影响
        conn.execute("INSERT INTO songs (id, title, artist, audio_path) VALUES ('002', '曲', '歌手', '')", [])
            .unwrap();

        let covers = dir.join("covers");
        let report = super::backfill_covers(&conn, &covers, false).unwrap();
        assert_eq!((report.checked, report.filled, report.no_candidate), (2, 1, 1), "{report:?}");
        let stored: String = conn
            .query_row("SELECT cover_path FROM songs WHERE id='001'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(std::path::PathBuf::from(&stored), jp_scraper::cover_path_for(&covers, "001", "歌手"));
        assert_eq!(std::fs::read(&stored).unwrap(), picture);

        // 那个位置已有的图不覆盖
        std::fs::write(&stored, b"user's own").unwrap();
        let again = super::save_embedded_cover(&covers, "001", "歌手", &audio);
        assert_eq!(again.as_deref(), Some(stored.as_str()));
        assert_eq!(std::fs::read(&stored).unwrap(), b"user's own");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 补封面时的候选排序：赢家第一，其余按分数，跨歌手的不要
    #[test]
    fn cover_fallback_keeps_the_same_artist_and_skips_weak_candidates() {
        let make = |id: &str, artist: &str, score: f64| {
            let mut c = jp_scraper::ScrapeCandidate {
                provider: "itunes".into(),
                provider_id: id.into(),
                artist: artist.into(),
                ..Default::default()
            };
            c.breakdown = Some(jp_scraper::MatchBreakdown {
                final_score: score,
                ..Default::default()
            });
            c
        };
        let candidates = vec![
            // 赢家：合作曲，provider 把另一位写进了歌手名
            make("win", "ずっと真夜中でいいのに。 feat. Mori Calliope", 0.98),
            make("same-artist-high", "ずっと真夜中でいいのに。", 0.94),
            make("same-artist-low", "ずっと真夜中でいいのに。", 0.60),
            make("other-artist", "YOASOBI", 0.99),
        ];
        let ids: Vec<&str> = super::cover_candidates(&candidates, "win")
            .iter()
            .map(|c| c.provider_id.as_str())
            .collect();
        // feat. 的那位不该让同一位歌手的候选被判成外人；分数低的和别的歌手都排除
        assert_eq!(ids, ["win", "same-artist-high"]);

        // 赢家不在候选里（记录残缺）时，不按歌手卡，但分数线还在
        let ids: Vec<&str> = super::cover_candidates(&candidates, "gone")
            .iter()
            .map(|c| c.provider_id.as_str())
            .collect();
        assert_eq!(ids, ["other-artist", "win", "same-artist-high"]);
    }

    use super::*;

    /// 人工确认要用到的库。
    ///
    /// **走 `jp_corpus` 的 `schema.sql`，不要在这里手抄建表语句。**
    fn manual_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        jp_corpus::Corpus::ensure_schema_on(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO songs (id, title, artist, album, year, genre)
                VALUES ('001', '夜行', 'ヨルシカ', '', '', '');
             INSERT INTO people (id, name, normalized_name) VALUES (1, 'ヨルシカ', 'ヨルシカ');
             INSERT INTO track_credits (song_id, person_id, role, source)
                VALUES ('001', 1, 'performer', 'library');",
        )
        .unwrap();
        conn
    }

    fn candidate(artist: &str) -> jp_scraper::ScrapeCandidate {
        jp_scraper::ScrapeCandidate {
            provider: "itunes".into(),
            title: "夜行".into(),
            artist: artist.into(),
            album: "盗作".into(),
            year: "2020".into(),
            genre: "J-Pop".into(),
            // 空 artwork_url：不发网络请求，这几条测的是落库那部分
            ..Default::default()
        }
    }

    #[test]
    fn accepting_a_candidate_actually_writes_the_metadata() {
        // 这一条钉住的是一个真实缺口：之前「采用」只改 scrape_state 的状态，
        // songs 一个字没写、封面也没下——点了等于什么都没发生。
        let conn = manual_db();
        let out = apply_manual(
            &conn,
            std::path::Path::new("X:/nowhere"),
            "001",
            "ヨルシカ",
            &candidate("ヨルシカ"),
        )
        .unwrap();
        assert!(out.fields_filled > 0);

        let (album, year, genre): (String, String, String) = conn
            .query_row(
                "SELECT album, year, genre FROM songs WHERE id='001'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (album.as_str(), year.as_str(), genre.as_str()),
            ("盗作", "2020", "J-Pop")
        );
    }

    #[test]
    fn accepting_never_overwrites_a_field_that_already_has_a_value() {
        // 需求：永远不覆盖用户本地文件里的原始 metadata
        let conn = manual_db();
        conn.execute("UPDATE songs SET album='本地写的专辑' WHERE id='001'", [])
            .unwrap();
        apply_manual(
            &conn,
            std::path::Path::new("X:/nowhere"),
            "001",
            "ヨルシカ",
            &candidate("ヨルシカ"),
        )
        .unwrap();
        let album: String = conn
            .query_row("SELECT album FROM songs WHERE id='001'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(album, "本地写的专辑");
    }

    #[test]
    fn accepting_marks_the_credits_as_manual() {
        // source='manual' 在这套 schema 里就是「人工版本，自动流程不许动」
        let conn = manual_db();
        let out = apply_manual(
            &conn,
            std::path::Path::new("X:/nowhere"),
            "001",
            "ヨルシカ",
            &candidate("ヨルシカ"),
        )
        .unwrap();
        assert_eq!(out.credits_protected, 1);
        // 歌手同名，不该重复加一条
        assert_eq!(out.credits_added, 0);

        let source: String = conn
            .query_row(
                "SELECT source FROM track_credits WHERE song_id='001'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(source, "manual");
    }

    #[test]
    fn a_candidate_naming_another_artist_adds_a_manual_credit() {
        let conn = manual_db();
        let out = apply_manual(
            &conn,
            std::path::Path::new("X:/nowhere"),
            "001",
            "ヨルシカ",
            &candidate("ヨルシカ/suis"),
        )
        .unwrap();
        assert_eq!(out.credits_added, 1, "suis 该被记上");

        let rows: Vec<(String, String)> = conn
            .prepare(
                "SELECT p.name, c.source FROM track_credits c
                 JOIN people p ON p.id = c.person_id
                 WHERE c.song_id='001' ORDER BY p.name",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                ("suis".to_string(), "manual".to_string()),
                ("ヨルシカ".to_string(), "manual".to_string())
            ]
        );
    }

    #[test]
    fn applying_twice_changes_nothing_the_second_time() {
        let conn = manual_db();
        let dir = std::path::Path::new("X:/nowhere");
        apply_manual(&conn, dir, "001", "ヨルシカ", &candidate("ヨルシカ")).unwrap();
        let again = apply_manual(&conn, dir, "001", "ヨルシカ", &candidate("ヨルシカ")).unwrap();
        assert_eq!(again.fields_filled, 0, "字段都填过了");
        assert_eq!(again.credits_protected, 0, "已经是 manual 了");
        assert_eq!(again.credits_added, 0);
    }

    #[test]
    fn a_candidate_without_artwork_does_not_pretend_to_have_saved_one() {
        let conn = manual_db();
        let out = apply_manual(
            &conn,
            std::path::Path::new("X:/nowhere"),
            "001",
            "ヨルシカ",
            &candidate("ヨルシカ"),
        )
        .unwrap();
        assert!(out.cover_path.is_none());
        let cover: Option<String> = conn
            .query_row("SELECT cover_path FROM songs WHERE id='001'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(cover.unwrap_or_default().is_empty());
    }

    fn release_date_of(conn: &rusqlite::Connection) -> String {
        conn.query_row("SELECT COALESCE(release_date,'') FROM songs WHERE id='001'", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn accepting_a_candidate_fills_the_full_release_date() {
        let conn = manual_db();
        let mut c = candidate("ヨルシカ");
        c.release_date = "2020-07-29".into();
        apply_manual(&conn, std::path::Path::new("X:/nowhere"), "001", "ヨルシカ", &c).unwrap();
        assert_eq!(release_date_of(&conn), "2020-07-29");
    }

    /// 本地文件写的是 2019，候选的日期是 2020 的：那个日期不是这首歌的，不能补
    #[test]
    fn a_release_date_from_another_year_is_not_taken() {
        let conn = manual_db();
        conn.execute_batch("UPDATE songs SET year='2019' WHERE id='001'").unwrap();
        let mut c = candidate("ヨルシカ");
        c.release_date = "2020-07-29".into();
        apply_manual(&conn, std::path::Path::new("X:/nowhere"), "001", "ヨルシカ", &c).unwrap();
        assert_eq!(release_date_of(&conn), "");
    }

    fn rel(name: &str, part: &str, joined: &str, left_at: &str) -> jp_scraper::providers::musicbrainz::BandRelation {
        jp_scraper::providers::musicbrainz::BandRelation {
            mbid: String::new(),
            name: name.into(),
            part: part.into(),
            joined: joined.into(),
            left_at: left_at.into(),
        }
    }

    #[test]
    fn a_band_lookup_writes_its_members_and_creates_people_who_never_got_a_credit() {
        let conn = manual_db();
        conn.execute_batch("INSERT INTO people (id, name, normalized_name) VALUES (2, 'n-buna', 'nbuna')")
            .unwrap();
        let relations = jp_scraper::providers::musicbrainz::ArtistRelations {
            members: vec![rel("n-buna", "吉他", "2017", ""), rel("suis", "主唱", "2017", "")],
            ..Default::default()
        };
        assert_eq!(apply_relations(&conn, "ヨルシカ", &relations).unwrap(), 2);
        let names: Vec<_> = jp_corpus::members::members_of(&conn, 1)
            .unwrap()
            .into_iter()
            .map(|m| (m.member_name, m.source))
            .collect();
        assert_eq!(
            names,
            [("n-buna".to_string(), "musicbrainz".to_string()), ("suis".to_string(), "musicbrainz".to_string())]
        );
    }

    /// 查个人时，库里没有的乐队不建空壳
    #[test]
    fn a_person_lookup_only_links_bands_already_in_the_library() {
        let conn = manual_db();
        let relations = jp_scraper::providers::musicbrainz::ArtistRelations {
            member_of: vec![rel("ヨルシカ", "吉他", "2017", ""), rel("库里没有的乐队", "", "2010", "2012")],
            ..Default::default()
        };
        assert_eq!(apply_relations(&conn, "n-buna", &relations).unwrap(), 1);
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM people WHERE name='库里没有的乐队'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    /// 你否掉的、手动写的，第二次刮削不动
    #[test]
    fn scraping_again_leaves_your_own_decisions_alone() {
        let conn = manual_db();
        let relations = jp_scraper::providers::musicbrainz::ArtistRelations {
            members: vec![rel("n-buna", "吉他", "2017", ""), rel("suis", "主唱", "2017", "")],
            ..Default::default()
        };
        apply_relations(&conn, "ヨルシカ", &relations).unwrap();
        let suis = jp_corpus::members::ensure_person(&conn, "suis").unwrap();
        let nbuna = jp_corpus::members::ensure_person(&conn, "n-buna").unwrap();
        jp_corpus::members::reject(&conn, 1, suis).unwrap();
        jp_corpus::members::put(&conn, 1, nbuna, "作词作曲 · 吉他", "2017", "", "manual").unwrap();

        assert_eq!(apply_relations(&conn, "ヨルシカ", &relations).unwrap(), 0);
        let members = jp_corpus::members::members_of(&conn, 1).unwrap();
        assert_eq!(members.len(), 1, "suis 被否掉了，不回来");
        assert_eq!(members[0].part, "作词作曲 · 吉他", "手动写的分工不被刮削改掉");
    }
}
