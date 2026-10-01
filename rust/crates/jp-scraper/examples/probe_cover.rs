//! 单独试一次封面下载，看是网络慢还是我们自己的超时配置不对。
//!
//!     cargo run --release -p jp-scraper --example probe_cover

use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    use jp_scraper::http::{HttpClient, HttpConfig, Timeouts, Transport};
    use jp_scraper::providers::{MetadataProvider, itunes::ITunesProvider};
    use jp_scraper::{SearchQuery, UreqTransport};

    // 先真去搜一首，拿到真实的封面 URL
    let itunes = ITunesProvider::production(HttpClient::new(Box::new(UreqTransport)));
    let q = SearchQuery {
        title: "さよならはエモーション".into(),
        artist: "サカナクション".into(),
        ..Default::default()
    };
    let cands = itunes.search_tracks(&q, 5)?;
    let c = cands.first().ok_or_else(|| anyhow::anyhow!("没搜到"))?;
    println!("封面 URL : {}", c.artwork_url);
    println!("缩略图   : {}", c.thumb_url);

    for (label, total) in [
        ("10s（原来的默认）", Duration::from_secs(10)),
        ("90s（现在的封面配置）", Duration::from_secs(90)),
    ] {
        let started = Instant::now();
        let result = UreqTransport.get(
            &c.artwork_url,
            &[],
            Timeouts::new(Duration::from_secs(10), total),
        );
        match result {
            Ok(r) => println!(
                "  {label:<18} → HTTP {} · {} 字节 · {:?}",
                r.status,
                r.body.len(),
                started.elapsed()
            ),
            Err(e) => println!(
                "  {label:<18} → 失败 {e}（{:?}）· {:?}",
                e.error_type,
                started.elapsed()
            ),
        }
    }

    // 再走一遍 HttpClient（带重试和 UA），看差别在哪
    // 走生产用的封面配置 + 尺寸回退
    let client = HttpClient::new(Box::new(UreqTransport))
        .with_provider_name("cover")
        .with_config(HttpConfig::for_artwork());
    let sizes: Vec<String> = [600u32, 300, 100]
        .iter()
        .map(|s| jp_scraper::providers::itunes::hi_res_artwork(&c.artwork_url, *s))
        .collect();
    let dest = std::env::temp_dir().join("jp-probe-cover.jpg");
    let _ = std::fs::remove_file(&dest);
    let started = Instant::now();
    match jp_scraper::save_image_any(&client, &sizes, &dest, true) {
        Ok((index, outcome)) => println!(
            "  生产配置 + 回退      → 用了第 {} 档（{}）· {outcome:?} · {:?}",
            index + 1,
            sizes[index].rsplit('/').next().unwrap_or(""),
            started.elapsed()
        ),
        Err(e) => println!(
            "  生产配置 + 回退      → 失败 {e} · {:?}",
            started.elapsed()
        ),
    }
    let _ = std::fs::remove_file(&dest);
    Ok(())
}
