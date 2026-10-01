//! 封面下载：并发度和尺寸各能省多少？
//!
//!     cargo run --release -p jp-scraper --example probe_cover_scale
//!
//! 整条流水线现在 4.96 秒/首，其中约 3 秒是封面。要压到 5 分钟以内
//! （1.44 秒/首）就得知道两件事：
//!
//! * 并发度从 4 往上加还有没有用，还是已经撞到总带宽
//! * 600×600 换成 300×300 能省多少（数据量约 1/3）
//!
//! 两件都只能测，不能推。

use std::sync::Arc;
use std::time::Instant;

use jp_scraper::http::{HttpClient, HttpConfig};
use jp_scraper::providers::{MetadataProvider, itunes::ITunesProvider, itunes::hi_res_artwork};
use jp_scraper::{SearchQuery, UreqTransport};

fn main() -> anyhow::Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();
    let conn = rusqlite::Connection::open_with_flags(
        root.join("corpus.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let mut stmt = conn.prepare("SELECT title, artist FROM songs ORDER BY id LIMIT 16")?;
    let songs: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;

    // 先拿一批真实封面 URL
    let itunes = ITunesProvider::production(HttpClient::new(Box::new(UreqTransport)));
    let mut base_urls: Vec<String> = Vec::new();
    for (title, artist) in &songs {
        let q = SearchQuery {
            title: title.clone(),
            artist: artist.clone(),
            ..Default::default()
        };
        if let Ok(cands) = itunes.search_tracks(&q, 3)
            && let Some(c) = cands.first()
            && !c.artwork_url.is_empty()
            && !base_urls.contains(&c.artwork_url)
        {
            base_urls.push(c.artwork_url.clone());
        }
        if base_urls.len() >= 16 {
            break;
        }
    }
    println!("拿到 {} 个不同的封面 URL\n", base_urls.len());

    let fetch = |url: String| -> usize {
        let client =
            HttpClient::new(Box::new(UreqTransport)).with_config(HttpConfig::for_artwork());
        client.get_bytes(&url).map(|b| b.len()).unwrap_or(0)
    };

    // 每组用不同的图，免得吃到 CDN 缓存把结果做漂亮
    let mut cursor = 0usize;
    let mut take = |n: usize| -> Vec<String> {
        let out: Vec<String> = base_urls
            .iter()
            .cycle()
            .skip(cursor)
            .take(n)
            .cloned()
            .collect();
        cursor += n;
        out
    };

    println!("── 并发度（600×600）──");
    for workers in [4usize, 8, 12] {
        let urls = take(workers);
        let started = Instant::now();
        let handles: Vec<_> = urls
            .into_iter()
            .map(|u| std::thread::spawn(move || fetch(u)))
            .collect();
        let bytes: usize = handles.into_iter().map(|h| h.join().unwrap_or(0)).sum();
        let dt = started.elapsed();
        println!(
            "  {workers:>2} 路 · {bytes:>8} 字节 · {dt:?} · {:.1} KB/s · 每张摊 {:.2} 秒",
            bytes as f64 / 1024.0 / dt.as_secs_f64(),
            dt.as_secs_f64() / workers as f64
        );
    }

    println!("\n── 尺寸（4 路并发）──");
    for size in [600u32, 300, 200] {
        let urls: Vec<String> = take(4).iter().map(|u| hi_res_artwork(u, size)).collect();
        let started = Instant::now();
        let handles: Vec<_> = urls
            .into_iter()
            .map(|u| std::thread::spawn(move || fetch(u)))
            .collect();
        let bytes: usize = handles.into_iter().map(|h| h.join().unwrap_or(0)).sum();
        let dt = started.elapsed();
        println!(
            "  {size}×{size} · {bytes:>8} 字节（每张 {:.0} KB）· {dt:?} · 每张摊 {:.2} 秒",
            bytes as f64 / 1024.0 / 4.0,
            dt.as_secs_f64() / 4.0
        );
    }

    let _ = Arc::new(());
    Ok(())
}
