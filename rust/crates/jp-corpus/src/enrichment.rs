//! 刮削只补空值，两种壳都不能用网络候选覆盖用户原有的标签。
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

pub struct Metadata<'a> {
    pub extra: &'a std::collections::BTreeMap<String, String>,
    pub album: &'a str,
    pub year: &'a str,
    pub genre: &'a str,
    pub release_date: &'a str,
    pub provider: &'a str,
    pub provider_id: &'a str,
    pub confidence: Option<f64>,
}

pub fn fill_missing(conn: &Connection, song_id: &str, candidate: &Metadata<'_>) -> Result<usize> {
    let (album, year, genre): (String, String, String) = conn.query_row(
        "SELECT COALESCE(album,''), COALESCE(year,''), COALESCE(genre,'') FROM songs WHERE id=?1",
        params![song_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let mut filled = 0;
    let provenance = |field: &str| crate::lineage::set_provenance(
        conn, "song", song_id, field, candidate.provider, candidate.provider_id, candidate.confidence,
    );
    for (field, empty, value) in [
        ("album", album.is_empty(), candidate.album),
        ("year", year.is_empty(), candidate.year),
        ("genre", genre.is_empty(), candidate.genre),
    ] {
        if empty && !value.is_empty() { provenance(field)?; filled += 1; }
    }
    conn.execute(
        "UPDATE songs SET
          album=CASE WHEN COALESCE(album,'')='' THEN ?1 ELSE album END,
          year=CASE WHEN COALESCE(year,'')='' THEN ?2 ELSE year END,
          genre=CASE WHEN COALESCE(genre,'')='' THEN ?3 ELSE genre END WHERE id=?4",
        params![candidate.album, candidate.year, candidate.genre, song_id],
    ).context("写 songs 失败")?;
    // 本地年份和网络日期冲突时不补日期，避免把再版的日期安到初版上。
    if !candidate.release_date.is_empty() {
        let written = conn.execute(
            "UPDATE songs SET release_date=?1 WHERE id=?2 AND COALESCE(release_date,'')=''
             AND (COALESCE(year,'')='' OR substr(year,1,4)=substr(?1,1,4))",
            params![candidate.release_date, song_id],
        ).context("写发行日期失败")?;
        if written > 0 { provenance("release_date")?; }
    }
    crate::identity::apply(conn, song_id, candidate.extra, candidate.provider, candidate.provider_id, candidate.confidence)?;
    Ok(filled)
}

pub fn link_album(conn: &Connection, song_id: &str, album: &str, artist: &str, year: &str) -> Result<()> {
    let title = album.trim();
    if title.is_empty() { return Ok(()) }
    let album_artist = artist.split(['/', '／']).map(str::trim).find(|s| !s.is_empty()).unwrap_or("");
    let title_key = jp_normalize::matching_key(title);
    let artist_key = jp_normalize::matching_key(album_artist);
    let existing: Option<i64> = conn.query_row("SELECT id FROM albums WHERE normalized_title=?1 AND normalized_album_artist=?2", params![title_key,artist_key], |r| r.get(0)).optional()?;
    let id = match existing {
        Some(id) => id,
        None => {
            conn.execute("INSERT INTO albums (title,normalized_title,album_artist,normalized_album_artist,year,created_at) VALUES (?1,?2,?3,?4,?5,strftime('%Y-%m-%dT%H:%M:%S+00:00','now'))", params![title,title_key,album_artist,artist_key,year])?;
            conn.last_insert_rowid()
        }
    };
    conn.execute("UPDATE songs SET album_id=?1 WHERE id=?2", params![id,song_id])?;
    Ok(())
}
