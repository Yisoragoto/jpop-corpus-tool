//! 在库的**副本**上核对曲库维护：
//!
//! ```text
//! cargo run --release -p jp-import --example maintain_check -- relink <库副本> <音频目录>
//! cargo run --release -p jp-import --example maintain_check -- delete <库副本> <曲目 id>
//! ```
//!
//! - `relink`：把所有曲目的音频路径改成不存在的路径，扫描音频目录求建议，逐首和原路径比对；
//! - `delete`：删一首歌，检查按它 id / 路径挂着的行一行不剩、全文索引完整、分词校正留着。
//!
//! 拒绝往真库里写。

use std::path::Path;

use anyhow::{Context, bail, ensure};
use rusqlite::Connection;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [mode, db, target] = args.as_slice() else {
        bail!("用法: maintain_check relink|delete <库副本> <音频目录|曲目 id>");
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).unwrap().to_path_buf();
    let real = root.join("corpus.db");
    if real.exists() && std::fs::canonicalize(db)? == std::fs::canonicalize(&real)? {
        bail!("{} 是真库，不往里写", real.display());
    }
    let mut conn = Connection::open(db)?;
    match mode.as_str() {
        "relink" => relink(&mut conn, Path::new(target)),
        "delete" => delete(&mut conn, target),
        _ => bail!("不认识的模式 {mode}"),
    }
}

fn relink(conn: &mut Connection, audio_dir: &Path) -> anyhow::Result<()> {
    let originals: Vec<(String, String)> = conn
        .prepare("SELECT id, audio_path FROM songs ORDER BY id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    conn.execute("UPDATE songs SET audio_path = 'Z:/gone/' || id || '.flac'", [])?;
    let missing = jp_import::maintain::missing_audio(conn)?;
    let started = std::time::Instant::now();
    let scanned = jp_import::scan_dir(audio_dir, 8);
    let suggestions = jp_import::maintain::suggest_relinks(&missing, &scanned, &[]);
    let elapsed = started.elapsed();

    let mut right = 0;
    let mut wrong = Vec::new();
    for s in &suggestions {
        let original = &originals.iter().find(|(id, _)| *id == s.song_id).context("建议了不存在的曲目")?.1;
        if s.path.replace('\\', "/").eq_ignore_ascii_case(&original.replace('\\', "/")) {
            right += 1;
        } else {
            wrong.push((s.song_id.clone(), s.path.clone(), original.clone(), s.reason.clone()));
        }
    }
    let unmatched: Vec<&str> = missing
        .iter()
        .filter(|m| !suggestions.iter().any(|s| s.song_id == m.song_id))
        .map(|m| m.song_id.as_str())
        .collect();
    let mut reasons = std::collections::BTreeMap::new();
    for s in &suggestions {
        *reasons.entry(s.reason.as_str()).or_insert(0) += 1;
    }
    println!(
        "丢了音频 {} 首，扫描到 {} 个文件（{:.1} 秒）；建议 {} 首：对 {right}，错 {}；没给建议 {} 首 {:?}",
        missing.len(),
        scanned.len(),
        elapsed.as_secs_f64(),
        suggestions.len(),
        wrong.len(),
        unmatched.len(),
        unmatched
    );
    println!("按理由：{reasons:?}");
    for w in &wrong {
        println!("  错配 {w:?}");
    }
    ensure!(wrong.is_empty(), "有错配");
    Ok(())
}

fn count(conn: &Connection, sql: &str, value: &str) -> anyhow::Result<i64> {
    Ok(conn.query_row(sql, [value], |r| r.get(0))?)
}

fn delete(conn: &mut Connection, song_id: &str) -> anyhow::Result<()> {
    let path: String = conn.query_row("SELECT audio_path FROM songs WHERE id=?1", [song_id], |r| r.get(0))?;
    let utterance_ids: Vec<i64> =
        conn.prepare("SELECT id FROM utterances WHERE song_id=?1")?.query_map([song_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let sample: Option<String> = conn
        .query_row("SELECT text FROM utterances WHERE song_id=?1 AND length(text) >= 6 ORDER BY id LIMIT 1", [song_id], |r| r.get(0))
        .ok();
    let before_people: i64 = conn.query_row("SELECT COUNT(*) FROM people", [], |r| r.get(0))?;
    let before_albums: i64 = conn.query_row("SELECT COUNT(*) FROM albums", [], |r| r.get(0))?;

    let tx = conn.transaction()?;
    let report = jp_import::maintain::delete_song(&tx, song_id)?;
    tx.commit()?;
    println!("{}", serde_json::to_string(&report)?);

    for (table, sql, key) in [
        ("songs", "SELECT COUNT(*) FROM songs WHERE id=?1", song_id),
        ("utterances", "SELECT COUNT(*) FROM utterances WHERE song_id=?1", song_id),
        ("track_credits", "SELECT COUNT(*) FROM track_credits WHERE song_id=?1", song_id),
        ("favorites", "SELECT COUNT(*) FROM favorites WHERE entity_type='song' AND entity_id=?1", song_id),
        ("play_history", "SELECT COUNT(*) FROM play_history WHERE song_id=?1", song_id),
        ("scrape_state", "SELECT COUNT(*) FROM scrape_state WHERE song_id=?1", song_id),
        ("scrape_state(path)", "SELECT COUNT(*) FROM scrape_state WHERE file_path=?1", path.as_str()),
        ("scrape_attempts", "SELECT COUNT(*) FROM scrape_attempts WHERE file_path=?1", path.as_str()),
        ("track_original_metadata", "SELECT COUNT(*) FROM track_original_metadata WHERE file_path=?1", path.as_str()),
    ] {
        let n = count(conn, sql, key)?;
        println!("  {table}: 剩 {n}");
        ensure!(n == 0, "{table} 还有 {n} 行");
    }
    let ids = utterance_ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    if !ids.is_empty() {
        let tokens: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM tokens WHERE utterance_id IN ({ids})"), [], |r| r.get(0))?;
        ensure!(tokens == 0, "tokens 还有 {tokens} 行");
    }
    conn.execute("INSERT INTO utterances_fts(utterances_fts) VALUES('integrity-check')", [])?;
    if let Some(text) = sample {
        let still: i64 = conn.query_row(
            "SELECT COUNT(*) FROM utterances_fts f JOIN utterances u ON u.id=f.rowid WHERE utterances_fts MATCH ?1 AND u.song_id=?2",
            [format!("\"{}\"", text.replace('"', "")), song_id.to_owned()],
            |r| r.get(0),
        )?;
        ensure!(still == 0, "全文索引还能搜到这首歌");
        println!("  全文索引完整；「{text}」在这首歌里搜不到了");
    }
    // 孤儿检查：每张专辑都有歌，每个人都有署名
    let orphan_albums: i64 =
        conn.query_row("SELECT COUNT(*) FROM albums a WHERE NOT EXISTS (SELECT 1 FROM songs s WHERE s.album_id=a.id)", [], |r| r.get(0))?;
    let orphan_people: i64 = conn.query_row(
        "SELECT COUNT(*) FROM people p WHERE NOT EXISTS (SELECT 1 FROM track_credits c WHERE c.person_id=p.id)",
        [],
        |r| r.get(0),
    )?;
    let after_people: i64 = conn.query_row("SELECT COUNT(*) FROM people", [], |r| r.get(0))?;
    let after_albums: i64 = conn.query_row("SELECT COUNT(*) FROM albums", [], |r| r.get(0))?;
    println!(
        "  人 {before_people} → {after_people}，专辑 {before_albums} → {after_albums}；删完后没有歌的专辑 {orphan_albums} 张、没有署名的人 {orphan_people} 个"
    );
    Ok(())
}
