//! 拿真实曲库跑一遍刮削，**真的发网络请求**。
//!
//!     cargo run --release -p jp-scraper --example scrape_library -- [几首] [起始序号]
//!
//! 默认只跑 12 首。这是对外部公开接口的只读查询，限流器会把请求摊开
//! （iTunes 4 req/s、MusicBrainz 1 req/s），不要一次跑整库去砸人家的服务。
//!
//! 为什么必须跑真的：离线测试用的是我自己造的响应，验的是「我以为
//! 接口长这样」。iTunes 用 403 表示限流、MusicBrainz 的 releases 顺序
//! 是任意的——这两个坑都只有真请求才会暴露。

use std::path::Path;

use jp_scraper::{ScrapeStatus, TrackFile};

fn main() -> anyhow::Result<()> {
    let limit: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(12);
    let offset: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();
    let db = root.join("corpus.db");
    anyhow::ensure!(db.is_file(), "找不到 {}", db.display());

    // 直接从库里取，不重新扫盘——这里验的是刮削，不是扫描
    let conn =
        rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare(
        "SELECT id, title, artist, album, year, genre, audio_path, duration_sec
         FROM songs ORDER BY id",
    )?;
    let rows: Vec<(String, TrackFile)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                TrackFile {
                    path: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                    embedded_title: row.get(1)?,
                    embedded_artist: row.get(2)?,
                    embedded_album: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    embedded_year: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    embedded_genre: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    duration_sec: row.get(7)?,
                    ..Default::default()
                },
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let sample: Vec<_> = rows.into_iter().skip(offset).take(limit).collect();
    println!("对 {} 首发真实请求（iTunes + MusicBrainz）\n", sample.len());

    let resolver = jp_scraper::default_resolver(false)?;
    let started = std::time::Instant::now();
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    let mut errors = std::collections::BTreeMap::<&str, usize>::new();
    let mut wrong_title = Vec::new();

    for (song_id, track) in &sample {
        let result = resolver.resolve(track);
        *counts.entry(result.status.as_str()).or_default() += 1;
        if result.error_type != jp_scraper::ErrorType::None {
            *errors.entry(result.error_type.as_str()).or_default() += 1;
        }

        let mark = match result.status {
            ScrapeStatus::Success => "✅",
            ScrapeStatus::LowConfidence => "❓",
            _ => "❌",
        };
        let got = result
            .candidate
            .as_ref()
            .map(|c| format!("{} / {} / {}", c.title, c.artist, c.album))
            .unwrap_or_else(|| result.error_message.clone());
        println!(
            "{mark} [{song_id}] {} / {}\n     → {:.3}  {got}",
            track.title(),
            track.artist(),
            result.confidence
        );
        if let Some(b) = result.breakdown() {
            println!("     {}", b.explain());
        }
        // 曲名对不上的单独记下来——这是最能说明「匹配错了」的信号
        if let Some(c) = &result.candidate
            && jp_normalize::matching_key(&c.title) != jp_normalize::matching_key(track.title())
        {
            wrong_title.push((song_id.clone(), track.title().to_string(), c.title.clone()));
        }
    }

    println!("\n用时 {:?}", started.elapsed());
    println!("状态分布：");
    for (status, n) in &counts {
        println!("  {status:<16} {n}");
    }
    if !errors.is_empty() {
        println!("失败原因：");
        for (kind, n) in &errors {
            println!("  {kind:<16} {n}");
        }
    }
    if wrong_title.is_empty() {
        println!("\n采纳的候选里没有曲名对不上的");
    } else {
        println!("\n⚠️ 曲名对不上的（{}）：", wrong_title.len());
        for (id, want, got) in &wrong_title {
            println!("  [{id}] 本地={want:?} 候选={got:?}");
        }
    }
    Ok(())
}
