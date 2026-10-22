//! 外部身份及其出处一起落库；只补空字段，人工清空的字段也不能被自动刮削填回。
use std::collections::BTreeMap;
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, params};

pub fn external_id(conn: &Connection, entity: &str, id: &str, provider: &str, value: &str, source: &str, confidence: Option<f64>) -> Result<()> {
    conn.execute(
        "INSERT INTO external_ids (entity_type,entity_id,provider,external_id,source,confidence) VALUES (?1,?2,?3,?4,?5,?6)
         ON CONFLICT(entity_type,entity_id,provider) DO UPDATE SET external_id=excluded.external_id,source=excluded.source,confidence=excluded.confidence",
        params![entity,id,provider,value,source,confidence],
    )?;
    Ok(())
}

pub fn apply(conn: &Connection, song_id: &str, values: &BTreeMap<String,String>, source: &str, reference: &str, confidence: Option<f64>) -> Result<()> {
    if values.is_empty() { return Ok(()) }
    let count = values.get("track_count").filter(|s| !s.is_empty()).map(|s| s.parse::<i64>().context("专辑曲目数不是整数")).transpose()?;
    ensure!(count.is_none_or(|n| n > 0), "专辑曲目数必须大于零");
    // 保存点也能嵌进导入/人工确认的外层事务，避免写了 ISRC 却丢了对应来源。
    atomic(conn, || -> Result<()> {
        let (album_id, album, artist, year): (Option<i64>,String,String,String) = conn.query_row(
            "SELECT album_id,COALESCE(album,''),artist,COALESCE(year,'') FROM songs WHERE id=?1", [song_id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        )?;
        let album_id = if album_id.is_none() && !album.trim().is_empty() {
            crate::enrichment::link_album(conn,song_id,&album,&artist,&year)?;
            conn.query_row("SELECT album_id FROM songs WHERE id=?1", [song_id], |r| r.get::<_,Option<i64>>(0))?
        } else { album_id };
        let mut fields = vec![("song",song_id.to_string(),"songs","isrc","isrc"), ("song",song_id.to_string(),"songs","recording_mbid","musicbrainz:recording")];
        // 本地专辑与候选不同，不能把再版/合辑的厂牌安到当前专辑上。
        let same_album = values.get("album_title").is_none_or(|title| jp_normalize::matching_key(title) == jp_normalize::matching_key(&album));
        if let Some(album_id) = album_id.filter(|_| same_album) {
            for (field,provider) in [("release_group_mbid","musicbrainz:release-group"),("label",""),("barcode","barcode"),("album_type",""),("track_count","")] {
                fields.push(("album",album_id.to_string(),"albums",field,provider));
            }
        }
        for (entity,id,table,field,provider) in fields {
            let Some(value) = values.get(field).filter(|s| !s.is_empty()) else { continue };
            if crate::lineage::source_of(conn,entity,&id,field)?.as_deref() == Some("manual") { continue; }
            // table/field 全来自上面的固定表，不接受调用方的 SQL 标识符。
            let changed = conn.execute(&format!("UPDATE {table} SET {field}=?1 WHERE id=?2 AND COALESCE({field},'')=''"), params![value,id])?;
            if changed > 0 {
                crate::lineage::set_provenance(conn,entity,&id,field,source,reference,confidence)?;
                if !provider.is_empty() { external_id(conn,entity,&id,provider,value,source,confidence)?; }
            }
        }
        Ok(())
    })
}

pub(crate) fn atomic(conn: &Connection, work: impl FnOnce() -> Result<()>) -> Result<()> {
    conn.execute_batch("SAVEPOINT jp_content_identity")?;
    let result = work();
    if result.is_err() { let _ = conn.execute_batch("ROLLBACK TO jp_content_identity"); }
    conn.execute_batch("RELEASE jp_content_identity")?;
    result
}

pub fn forget(conn: &Connection, entity: &str, id: &str) -> Result<()> {
    conn.execute("DELETE FROM external_ids WHERE entity_type=?1 AND entity_id=?2", params![entity,id])?;
    conn.execute("DELETE FROM field_provenance WHERE entity_type=?1 AND entity_id=?2", params![entity,id])?;
    Ok(())
}

pub fn fields(conn: &Connection, song_id: &str) -> Result<BTreeMap<String,String>> {
    let names = ["isrc","recording_mbid","audio_fingerprint","album_id","release_group_mbid","label","barcode","album_type","track_count"];
    Ok(conn.query_row("SELECT s.isrc,s.recording_mbid,s.audio_fingerprint,CAST(s.album_id AS TEXT),a.release_group_mbid,a.label,a.barcode,a.album_type,CAST(a.track_count AS TEXT) FROM songs s LEFT JOIN albums a ON a.id=s.album_id WHERE s.id=?1", [song_id], |r| {
        names.iter().enumerate().map(|(i,name)|Ok((name.to_string(),r.get::<_,Option<String>>(i)?.unwrap_or_default()))).collect::<rusqlite::Result<_>>()
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fields_ids_and_provenance_are_saved_and_manual_fields_are_protected() {
        let corpus = crate::fixture::corpus().unwrap(); let conn = corpus.connection();
        let values = [("isrc","JPTEST000001"),("recording_mbid","recording-1"),("label","Fixture Records"),("release_group_mbid","group-1"),("barcode","1234"),("album_type","Album"),("track_count","12")].into_iter().map(|(k,v)|(k.into(),v.into())).collect();
        apply(conn,"001",&values,"musicbrainz","recording-1",Some(0.98)).unwrap();
        let (isrc,label): (String,String) = conn.query_row("SELECT s.isrc,a.label FROM songs s JOIN albums a ON a.id=s.album_id WHERE s.id='001'", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!((isrc.as_str(),label.as_str()),("JPTEST000001","Fixture Records"));
        let source = crate::lineage::provenance(conn,"song","001").unwrap();
        assert!(source.iter().any(|p| p.field=="isrc" && p.source=="musicbrainz" && p.confidence==Some(0.98)));
        let album_id: String = conn.query_row("SELECT CAST(album_id AS TEXT) FROM songs WHERE id='001'", [], |r| r.get(0)).unwrap();
        assert!(crate::lineage::provenance(conn,"album",&album_id).unwrap().iter().any(|p| p.field=="label" && p.source=="musicbrainz"));
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM external_ids",[],|r|r.get(0)).unwrap(); assert_eq!(n,4);
        crate::lineage::set_provenance(conn,"song","002","isrc","manual","user",None).unwrap();
        apply(conn,"002",&values,"musicbrainz","recording-1",Some(0.98)).unwrap();
        assert!(conn.query_row("SELECT isrc IS NULL FROM songs WHERE id='002'",[],|r|r.get::<_,bool>(0)).unwrap());
        let mut invalid = values.clone(); invalid.insert("track_count".into(),"oops".into());
        assert!(apply(conn,"003",&invalid,"deezer","1",None).is_err());
        assert!(conn.query_row("SELECT isrc IS NULL FROM songs WHERE id='003'",[],|r|r.get::<_,bool>(0)).unwrap());
        forget(conn,"song","001").unwrap();
        assert!(crate::lineage::source_of(conn,"song","001","isrc").unwrap().is_none());
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM external_ids WHERE entity_type='song' AND entity_id='001'",[],|r|r.get(0)).unwrap(); assert_eq!(n,0);
    }
}
