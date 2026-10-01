//! 「问完一个 provider 就判断」值不值得？
//!
//!     cargo run --release -p jp-scraper --example probe_early_stop [几首]
//!
//! 现在的逻辑是一条查询把所有 provider 都问一遍再判断。iTunes 通常
//! 一问就给 0.99，但仍然会去问 MusicBrainz——而 MB 限每秒一次，
//! 是整条链路里最慢的一环。
//!
//! 改成「够好就不问下一个」能省多少、会不会改变结果，只能测。
//!
//! **A/B 交替跑**：网络本身波动很大（同一批图两次测差 9 倍），
//! 分成两段跑的话结果全是网络漂移。这里每首歌两种配置紧挨着各跑一次。

use std::time::Instant;

use jp_scraper::{MatchScorer, MetadataResolver, ResolverConfig, TrackFile};

fn main() -> anyhow::Result<()> {
    let count: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

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
    let tracks: Vec<TrackFile> = stmt
        .query_map(rusqlite::params![count as i64], |row| {
            Ok(TrackFile {
                embedded_title: row.get(0)?,
                embedded_artist: row.get(1)?,
                embedded_album: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                duration_sec: row.get(3)?,
                ..Default::default()
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let build = |per_provider: bool| -> anyhow::Result<MetadataResolver> {
        Ok(MetadataResolver::new(jp_scraper::default_providers())?
            .with_scorer(MatchScorer::default())
            .with_config(ResolverConfig {
                stop_after_each_provider: per_provider,
                ..Default::default()
            }))
    };
    let all = build(false)?;
    let each = build(true)?;

    let mut all_time = std::time::Duration::ZERO;
    let mut each_time = std::time::Duration::ZERO;
    let mut differ = Vec::new();

    for track in &tracks {
        // 交替：网络漂移对两边影响一样
        let t0 = Instant::now();
        let a = all.resolve(track);
        all_time += t0.elapsed();

        let t0 = Instant::now();
        let b = each.resolve(track);
        each_time += t0.elapsed();

        let key = |r: &jp_scraper::ResolvedTrack| {
            r.candidate
                .as_ref()
                .map(|c| format!("{}/{}/{}/{}", c.provider, c.title, c.album, c.year))
                .unwrap_or_else(|| format!("<{}>", r.status.as_str()))
        };
        let (ka, kb) = (key(&a), key(&b));
        if ka != kb || (a.confidence - b.confidence).abs() > 1e-9 {
            differ.push(format!(
                "  {:?}\n    全问 : {ka} ({:.4})\n    早停 : {kb} ({:.4})",
                track.title(),
                a.confidence,
                b.confidence
            ));
        }
    }

    let n = tracks.len() as f64;
    println!(
        "全问一遍   {:.2} 秒/首（合计 {all_time:?}）",
        all_time.as_secs_f64() / n
    );
    println!(
        "够好就早停 {:.2} 秒/首（合计 {each_time:?}）",
        each_time.as_secs_f64() / n
    );
    println!(
        "提速 {:.1}×",
        all_time.as_secs_f64() / each_time.as_secs_f64().max(0.001)
    );
    println!("\n采纳结果不同的：{} / {}", differ.len(), tracks.len());
    for d in differ.iter().take(8) {
        println!("{d}");
    }
    Ok(())
}
