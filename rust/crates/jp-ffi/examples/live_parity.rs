//! 对账：移出曲库（软删除）和真删，面向用户的读接口结果要逐项相同。
//!
//! 用法（都在库副本上跑，别对着真库）：
//!
//! ```text
//! live_parity soft <db> <song_id>...   把这些歌标成已移出
//! live_parity hard <db> <song_id>...   用 jp_import::maintain::delete_song 真删
//! live_parity dump <db> <out.json> <lemma> <person_id> <album_id> <song_id>...
//! ```
//!
//! `dump` 按同一组参数调一遍 macOS 客户端用的读方法（和 Tauri 命令同名同形），
//! 结果按方法名写进一个 JSON。两份 dump 直接 diff。
//! `lemma`、`person_id`、`album_id` 选被移出的歌里出现的，才测得到差别；
//! 后面的 `song_id` 用来查单首歌（被移出的和在库的都给几首）。

use anyhow::{Context, Result, bail};
use jp_ffi::Session;
use serde_json::{Map, Value, json};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("soft") => soft(&args[1], &args[2..]),
        Some("hard") => hard(&args[1], &args[2..]),
        Some("dump") => dump(&args[1], &args[2], &args[3], args[4].parse()?, args[5].parse()?, &args[6..]),
        _ => bail!("用法见文件头"),
    }
}

fn soft(db: &str, ids: &[String]) -> Result<()> {
    // 先经会话打开一次，库升到当前结构（有 deleted_at 列）
    drop(Session::open(db)?);
    let conn = rusqlite::Connection::open(db)?;
    for id in ids {
        let n = conn.execute(
            "UPDATE songs SET deleted_at = '2026-11-06T00:00:00Z' WHERE id = ?1",
            [id],
        )?;
        anyhow::ensure!(n == 1, "没有歌 {id}");
    }
    Ok(())
}

fn hard(db: &str, ids: &[String]) -> Result<()> {
    drop(Session::open(db)?);
    let mut conn = rusqlite::Connection::open(db)?;
    let tx = conn.transaction()?;
    for id in ids {
        jp_import::maintain::delete_song(&tx, id).with_context(|| format!("删不掉 {id}"))?;
    }
    tx.commit()?;
    Ok(())
}

fn dump(db: &str, out: &str, lemma: &str, person: i64, album: i64, songs: &[String]) -> Result<()> {
    let session = Session::open(db)?;
    let mut calls: Vec<(String, Value)> = vec![
        ("overview".into(), json!({})),
        ("tracks".into(), json!({ "limit": 100000 })),
        ("performers".into(), json!({ "limit": 100000 })),
        ("albums".into(), json!({ "limit": 100000 })),
        ("albumTracks".into(), json!({ "albumId": album })),
        ("peopleDirectory".into(), json!({})),
        ("collaborationEdges".into(), json!({})),
        ("timelineReleases".into(), json!({})),
        ("timeline".into(), json!({})),
        ("personById".into(), json!({ "personId": person })),
        ("personWorks".into(), json!({ "personId": person })),
        ("collaborators".into(), json!({ "personId": person, "limit": 100 })),
        ("recentlyPlayed".into(), json!({ "limit": 1000 })),
        ("homeSummary".into(), json!({ "utcOffsetMinutes": 480, "addedLimit": 1000 })),
        ("kwic".into(), json!({ "query": { "keywords": [lemma], "field": "lemma" } })),
        ("searchLyrics".into(), json!({ "text": lemma, "limit": 1000 })),
        ("quickSearch".into(), json!({ "query": lemma, "perKind": 50 })),
        ("wordInCorpus".into(), json!({ "lemma": lemma, "limit": 1000 })),
        ("wordFrequency".into(), json!({ "limit": 100000 })),
        ("frequency".into(), json!({ "query": { "limit": 100000 } })),
        ("statsReport".into(), json!({})),
        ("statsFrequency".into(), json!({ "pos": "", "limit": 0 })),
        ("collocations".into(), json!({ "query": { "node": lemma, "minFreq": 1 } })),
        ("keyness".into(), json!({ "query": { "target": { "songIds": songs }, "minFreq": 1 } })),
        ("ngrams".into(), json!({ "query": { "n": 2, "minFreq": 2 } })),
        ("lexicalProfile".into(), json!({ "query": {} })),
        ("inflection".into(), json!({ "query": {} })),
        ("cqlSearch".into(), json!({ "query": { "query": format!("[lemma=\"{lemma}\"]") } })),
        ("kwicBreakdown".into(), json!({ "query": { "kwic": { "keywords": [lemma], "field": "lemma" } } })),
        ("collocationNetwork".into(), json!({ "query": { "collocation": { "node": lemma, "minFreq": 1 } } })),
        ("orthography".into(), json!({ "query": {} })),
        ("vowels".into(), json!({ "query": {} })),
        ("qualityOverview".into(), json!({ "limit": 100000 })),
        ("subcorpusFacets".into(), json!({})),
        ("subcorpusPreview".into(), json!({ "filter": {} })),
        ("completeness".into(), json!({})),
        ("scrapeSummary".into(), json!({})),
    ];
    for id in songs {
        for method in ["track", "lyrics", "trackCredits", "songProfile", "lineSpans", "lyricTimeline"] {
            calls.push((format!("{method} {id}"), json!({ "songId": id, "limit": 100 })));
        }
    }

    let mut results = Map::new();
    for (name, args) in calls {
        let method = name.split(' ').next().unwrap_or(&name);
        let started = std::time::Instant::now();
        let value = match session.call(method, args) {
            Ok(value) => value,
            Err(err) => json!({ "error": format!("{err:#}") }),
        };
        if std::env::var_os("LIVE_PARITY_TIMING").is_some() {
            eprintln!("{:>8.1} ms  {name}", started.elapsed().as_secs_f64() * 1000.0);
        }
        results.insert(name, value);
    }
    std::fs::write(out, serde_json::to_string_pretty(&Value::Object(results))?)?;
    Ok(())
}
