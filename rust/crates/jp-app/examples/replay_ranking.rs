//! 用库里缓存的候选池重放排序：看「去后缀 + 同分看文字」这两处改动会不会改掉已有的匹配。
//!
//! 只读。用法：`cargo run -p jp-app --example replay_ranking -- <corpus.db>`
use jp_scraper::matching::MatchScorer;
use jp_scraper::models::{ScrapeCandidate, TrackFile};
use jp_scraper::providers::itunes::strip_release_suffix;
use rusqlite::Connection;

fn main() -> anyhow::Result<()> {
    let db = std::env::args()
        .nth(1)
        .expect("用法: replay_ranking <corpus.db>");
    let conn = Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare(
        "SELECT s.file_path, s.provider, s.provider_id, s.confidence, s.candidates_json, m.source_json
           FROM scrape_state s
           JOIN track_original_metadata m ON m.file_path = s.file_path
          WHERE s.candidates_json IS NOT NULL AND m.source_json IS NOT NULL
          ORDER BY s.file_path",
    )?;
    let rows: Vec<(String, String, String, f64, String, String)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<Result<_, _>>()?;

    let scorer = MatchScorer::default();
    let (mut replayed, mut same_winner, mut album_changed, mut score_changed) = (0, 0, 0, 0);
    let mut changes: Vec<String> = Vec::new();
    let mut albums: Vec<String> = Vec::new();

    for (path, provider, provider_id, confidence, candidates_json, source_json) in rows {
        let track: TrackFile = serde_json::from_str(&source_json)?;
        let stored: Vec<ScrapeCandidate> = serde_json::from_str(&candidates_json)?;
        if stored.is_empty() {
            continue;
        }
        replayed += 1;
        let stored_album = stored[0].album.clone();
        let fixed: Vec<ScrapeCandidate> = stored
            .iter()
            .cloned()
            .map(|mut c| {
                if c.provider == "itunes" {
                    c.album = strip_release_suffix(&c.album);
                }
                c
            })
            .collect();
        let ranked = scorer.rank(&track, fixed);
        let best = &ranked[0];
        if best.provider == provider && best.provider_id == provider_id {
            same_winner += 1;
        } else {
            changes.push(format!(
                "{path}: {provider}/{provider_id} ({confidence:.4}) -> {}/{} ({:.4})",
                best.provider,
                best.provider_id,
                best.score()
            ));
        }
        if (best.score() - confidence).abs() > 1e-9 {
            score_changed += 1;
        }
        if best.album != stored_album {
            album_changed += 1;
            albums.push(format!("{path}: {stored_album:?} -> {:?}", best.album));
        }
    }

    println!(
        "重放 {replayed} 首；赢家不变 {same_winner}；分数变化 {score_changed}；专辑名变化 {album_changed}"
    );
    for line in changes.iter().take(30) {
        println!("  换人 {line}");
    }
    for line in albums.iter().take(30) {
        println!("  专辑 {line}");
    }
    Ok(())
}
