use anyhow::Result;
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistRow {
    pub name: String,
    pub image_path: String,
    pub artist_type: String,
    pub country: String,
    pub formed: String,
    pub updated_at: String,
    /// 库里有这个歌手的曲目数
    pub track_count: i64,
    /// 照片文件是不是真的在盘上。库里记了路径但文件被删了也算没有。
    pub has_image: bool,
}

pub fn artist_roster(conn: &rusqlite::Connection) -> Result<Vec<ArtistRow>> {
    use rusqlite::OptionalExtension;
    let names = jp_scrape::library_artists(conn, false)?;

    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let row = conn
            .query_row(
                "SELECT COALESCE(image_path,''), COALESCE(artist_type,''),
                        COALESCE(country,''), COALESCE(formed,''), COALESCE(updated_at,'')
                 FROM artists WHERE name=?1",
                rusqlite::params![name],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?
            .unwrap_or_default();
        let track_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM songs WHERE artist = ?1 OR artist LIKE ?1 || '/%'
                 OR artist LIKE '%/' || ?1 OR artist LIKE '%/' || ?1 || '/%'",
            rusqlite::params![name],
            |r| r.get(0),
        )?;
        let has_image = !row.0.is_empty() && std::path::Path::new(&row.0).is_file();
        out.push(ArtistRow {
            name,
            image_path: row.0,
            artist_type: row.1,
            country: row.2,
            formed: row.3,
            updated_at: row.4,
            track_count,
            has_image,
        });
    }
    Ok(out)
}

/// 手动成员输入适配，两端共用。
pub fn membership_put(
    conn: &rusqlite::Connection,
    band_id: i64,
    member_id: Option<i64>,
    member_name: Option<String>,
    part: &str,
    joined: &str,
    left_at: &str,
) -> Result<jp_corpus::members::PutOutcome> {
    let member = match (member_id, member_name) {
        (Some(id), _) => id,
        (None, Some(name)) => jp_corpus::members::ensure_person(conn, &name)?,
        (None, None) => anyhow::bail!("没给成员"),
    };
    jp_corpus::members::put(conn, band_id, member, part, joined, left_at, "manual")
}
