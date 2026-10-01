//! 元数据查询并发有没有用？
//!
//!     cargo run --release -p jp-scraper --example probe_resolve_parallel [几首]
//!
//! 我先前的判断是「MusicBrainz 限每秒一次，并发收益有限」——那是没测就下的
//! 结论。限流器限的是**两次请求的间隔**，不是同时在飞的数量：
//! 4 个线程排队时请求仍每 1.1 秒发一个，但各自的响应时间是重叠的。
//! 所以吞吐上限是 1/1.1s，而不是 1/(单首总耗时)。
//!
//! 到底差多少，跑一遍就知道。

use std::sync::Arc;
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let count: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();
    let conn = rusqlite::Connection::open_with_flags(
        root.join("corpus.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let mut stmt =
        conn.prepare("SELECT title, artist, album, duration_sec FROM songs ORDER BY id LIMIT ?1")?;
    let tracks: Vec<jp_scraper::TrackFile> = stmt
        .query_map(rusqlite::params![(count * 2) as i64], |row| {
            Ok(jp_scraper::TrackFile {
                embedded_title: row.get(0)?,
                embedded_artist: row.get(1)?,
                embedded_album: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                duration_sec: row.get(3)?,
                ..Default::default()
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let serial_set = &tracks[..count];
    // 两组用不同的歌，免得第二组吃到 provider 的缓存
    let parallel_set = &tracks[count..count * 2];

    let resolver = Arc::new(jp_scraper::default_resolver(false)?);

    // ── 串行 ──
    let started = Instant::now();
    let mut ok_count = 0;
    for t in serial_set {
        if resolver.resolve(t).ok() {
            ok_count += 1;
        }
    }
    let serial = started.elapsed();
    println!(
        "串行   {count} 首 · {serial:?} · {:.2} 秒/首 · 成功 {ok_count}",
        serial.as_secs_f64() / count as f64
    );

    // ── 4 路并发 ──
    let started = Instant::now();
    let handles: Vec<_> = parallel_set
        .chunks(count.div_ceil(4))
        .map(|chunk| {
            let resolver = resolver.clone();
            let chunk: Vec<_> = chunk.to_vec();
            std::thread::spawn(move || chunk.iter().filter(|t| resolver.resolve(t).ok()).count())
        })
        .collect();
    let ok_count: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
    let parallel = started.elapsed();
    println!(
        "4 路并发 {count} 首 · {parallel:?} · {:.2} 秒/首 · 成功 {ok_count}",
        parallel.as_secs_f64() / count as f64
    );

    println!(
        "\n提速 {:.1}×　整库 209 首：串行 {:.0} 分钟 → 并发 {:.0} 分钟",
        serial.as_secs_f64() / parallel.as_secs_f64().max(0.001),
        serial.as_secs_f64() / count as f64 * 209.0 / 60.0,
        parallel.as_secs_f64() / count as f64 * 209.0 / 60.0,
    );
    Ok(())
}
