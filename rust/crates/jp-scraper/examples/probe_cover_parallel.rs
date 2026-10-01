//! 封面下载并发有没有用？
//!
//!     cargo run --release -p jp-scraper --example probe_cover_parallel
//!
//! 实测单张 600x600 要 13~27 秒。整库 209 首就是一到两个小时，不可用。
//! 但「慢」有两种可能，处理方式完全不同：
//!
//! * **单连接被限速** → 并发有用，开几路就快几倍
//! * **总带宽就这么多** → 并发没用，只能降尺寸
//!
//! 这个例子分别测串行和 4 路并发，用同一批图。

use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    use jp_scraper::http::{HttpClient, HttpConfig};
    use jp_scraper::providers::{MetadataProvider, itunes::ITunesProvider};
    use jp_scraper::{SearchQuery, UreqTransport};

    // 先拿 4 张不同的真实封面
    let itunes = ITunesProvider::production(HttpClient::new(Box::new(UreqTransport)));
    let mut urls: Vec<String> = Vec::new();
    for (title, artist) in [
        ("さよならはエモーション", "サカナクション"),
        ("エンドレス", "サカナクション"),
        ("ネイティブダンサー", "サカナクション"),
        ("ユリイカ", "サカナクション"),
    ] {
        let q = SearchQuery {
            title: title.into(),
            artist: artist.into(),
            ..Default::default()
        };
        if let Some(c) = itunes.search_tracks(&q, 3)?.first()
            && !c.artwork_url.is_empty()
        {
            urls.push(c.artwork_url.clone());
        }
    }
    println!("拿到 {} 个封面 URL\n", urls.len());

    let fetch = |url: String| -> (usize, Duration) {
        let started = Instant::now();
        let client =
            HttpClient::new(Box::new(UreqTransport)).with_config(HttpConfig::for_artwork());
        let n = client.get_bytes(&url).map(|b| b.len()).unwrap_or(0);
        (n, started.elapsed())
    };

    // ── 串行 ──
    let started = Instant::now();
    let mut total_bytes = 0usize;
    for url in &urls {
        let (n, dt) = fetch(url.clone());
        total_bytes += n;
        println!("  串行 {n:>7} 字节 · {dt:?}");
    }
    let serial = started.elapsed();
    println!(
        "串行合计 {:?}（{} 字节，{:.1} KB/s）\n",
        serial,
        total_bytes,
        total_bytes as f64 / 1024.0 / serial.as_secs_f64()
    );

    // ── 4 路并发 ──
    let started = Instant::now();
    let handles: Vec<_> = urls
        .iter()
        .cloned()
        .map(|url| std::thread::spawn(move || fetch(url)))
        .collect();
    let mut total_bytes = 0usize;
    for h in handles {
        let (n, dt) = h.join().unwrap();
        total_bytes += n;
        println!("  并发 {n:>7} 字节 · {dt:?}");
    }
    let parallel = started.elapsed();
    println!(
        "并发合计 {:?}（{} 字节，{:.1} KB/s）",
        parallel,
        total_bytes,
        total_bytes as f64 / 1024.0 / parallel.as_secs_f64()
    );
    println!(
        "\n提速 {:.1}×",
        serial.as_secs_f64() / parallel.as_secs_f64().max(0.001)
    );
    Ok(())
}
