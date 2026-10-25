//! 客户端无关的校验与只读候选搜索。
use std::sync::Mutex;

use anyhow::Result;
use rusqlite::Connection;

use crate::timed_lyrics::{self as timed, SearchView, Session, Snapshot, SearchTicket};

pub fn revalidate(conn: &mut Connection, song_id: Option<String>) -> Result<usize> {
    let tx = conn.transaction()?;
    let ids: Vec<String> = match song_id {
        Some(id) => vec![id],
        None => {
            let mut stmt = tx.prepare("SELECT DISTINCT song_id FROM lyric_versions WHERE is_current=1")?;
            stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
        }
    };
    let mut count = 0;
    for id in &ids {
        count += usize::from(jp_corpus::lineage::revalidate(&tx, id)?);
    }
    tx.commit()?;
    Ok(count)
}

/// 调用者在准备快照和搜索票据后释放数据库锁，再进入网络阶段。
pub fn search(
    session: &Mutex<Session>, before: Snapshot,
    options: jp_scraper::timed_sources::SourceOptions, ticket: SearchTicket,
) -> Result<SearchView> {
    use jp_scraper::providers::LyricsProvider;
    let mut lyric_options = options.clone();
    lyric_options.apple = false;
    let provider = jp_scraper::providers::ConfiguredLyricsProvider(lyric_options);
    let mut found = provider.search_lyrics(&jp_scraper::SearchQuery {
        title: before.title.clone(), artist: before.artist.clone(),
        duration_sec: before.duration, ..Default::default()
    });
    found.candidates.extend(found.fallback.drain(..).filter_map(timed::from_basic));
    let mut accepted = Vec::new();
    for candidate in found.candidates {
        if timed::compatible(&before, &candidate) {
            accepted.push(candidate);
        } else {
            found.warnings.push(jp_scraper::timed_sources::SourceWarning {
                source: candidate.source,
                message: "有结果，但正文差异过大、没有真实逐字时间或时间轴超出本地音频，未采用".into(),
            });
        }
    }
    for warning in &mut found.warnings {
        warning.message = timed::redact(&warning.message, &options);
    }
    let current_lines = before.line_count();
    let current_timed_lines = before.timed_lines();
    let candidates = session.lock().unwrap_or_else(|e| e.into_inner())
        .finish_search(ticket, &before, accepted)?;
    Ok(SearchView {
        song_id: before.song_id, title: before.title, current_lines,
        current_timed_lines, candidates, warnings: found.warnings,
    })
}
