//! 曲库导航：Artist → Album → Track，以及信用与合作关系。
//!
//! SQL 直接沿用 `library/queries.py`——那一层就是按「迁到 Rust 时一个字
//! 不用改」的目标写的，这里是它的兑现。

use std::collections::HashMap;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::models::{Album, Collaborator, Credit, FacetCount, LineToken, LyricLine, Person, PersonSummary, Track};
use crate::search::placeholders;

/// 每首曲目都要带歌词行数，UI 靠它区分「没有歌词」和「还没加载」。
const TRACK_COLUMNS: &str = "s.id, s.title, s.artist, COALESCE(s.album,''), s.album_id, \
     COALESCE(s.year,''), COALESCE(s.genre,''), COALESCE(s.audio_path,''), \
     COALESCE(s.cover_path,''), s.duration_sec, \
     (SELECT COUNT(*) FROM utterances u WHERE u.song_id = s.id)";

fn track_from_row(row: &rusqlite::Row) -> rusqlite::Result<Track> {
    Ok(Track {
        id: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        album: row.get(3)?,
        album_id: row.get(4)?,
        year: row.get(5)?,
        genre: row.get(6)?,
        audio_path: row.get(7)?,
        cover_path: row.get(8)?,
        duration_sec: row.get(9)?,
        line_count: row.get(10)?,
    })
}

// ────────────────────────────── 曲目 ──────────────────────────────

pub(crate) fn tracks(conn: &Connection, limit: i64) -> Result<Vec<Track>> {
    let sql = format!("SELECT {TRACK_COLUMNS} FROM songs s ORDER BY s.id LIMIT ?1");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit], track_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn track(conn: &Connection, song_id: &str) -> Result<Option<Track>> {
    let sql = format!("SELECT {TRACK_COLUMNS} FROM songs s WHERE s.id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    Ok(stmt.query_row(params![song_id], track_from_row).optional()?)
}

// ────────────────────────────── 专辑 ──────────────────────────────

pub(crate) fn albums(conn: &Connection, artist_key: Option<&str>, limit: i64) -> Result<Vec<Album>> {
    let (filter, params): (&str, Vec<Box<dyn rusqlite::ToSql>>) = match artist_key {
        Some(key) if !key.is_empty() => (
            "WHERE al.normalized_album_artist = ?1",
            vec![Box::new(key.to_string()), Box::new(limit)],
        ),
        _ => ("", vec![Box::new(limit)]),
    };
    // 专辑自己没有封面时回落到成员曲目的封面。`albums.artwork_path` 要跑一次
    // 「填专辑封面」才会有值，而曲目封面刮削完就有了——不回落的话专辑区会
    // 明明有图却全是占位块。MIN 保证同一张专辑每次挑的是同一张图。
    // **只影响显示，不写库**：真要落到 albums 表还是得走 fill_album_artwork。
    let sql = format!(
        "SELECT al.id, al.title, al.album_artist, al.year, \
                CASE WHEN COALESCE(al.artwork_path,'') <> '' THEN al.artwork_path \
                     ELSE COALESCE(MIN(NULLIF(s.cover_path,'')), '') END, \
                COUNT(s.id), COALESCE(SUM(s.duration_sec), 0) \
         FROM albums al LEFT JOIN songs s ON s.album_id = al.id \
         {filter} GROUP BY al.id ORDER BY al.year DESC, al.title \
         LIMIT ?{}",
        params.len()
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params.iter().map(|p| p.as_ref())), |row| {
        Ok(Album {
            id: row.get(0)?,
            title: row.get(1)?,
            album_artist: row.get(2)?,
            year: row.get(3)?,
            artwork_path: row.get(4)?,
            track_count: row.get(5)?,
            total_duration_sec: row.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn album_tracks(conn: &Connection, album_id: i64) -> Result<Vec<Track>> {
    let sql = format!("SELECT {TRACK_COLUMNS} FROM songs s WHERE s.album_id = ?1 ORDER BY s.id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![album_id], track_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ────────────────────────────── 人物与信用 ──────────────────────────────

pub(crate) fn find_person(conn: &Connection, normalized: &str) -> Result<Option<Person>> {
    let mut stmt =
        conn.prepare("SELECT id, name, normalized_name FROM people WHERE normalized_name = ?1")?;
    Ok(stmt
        .query_row(params![normalized], |row| {
            Ok(Person {
                id: row.get(0)?,
                name: row.get(1)?,
                normalized_name: row.get(2)?,
            })
        })
        .optional()?)
}

pub(crate) fn credits_for_track(conn: &Connection, song_id: &str) -> Result<Vec<Credit>> {
    // 角色按展示顺序排，和 library/credits.py 的 ROLE_ORDER 一致
    let mut stmt = conn.prepare(
        "SELECT c.person_id, p.name, c.role, c.position, c.source \
         FROM track_credits c JOIN people p ON p.id = c.person_id \
         WHERE c.song_id = ?1 \
         ORDER BY CASE c.role \
             WHEN 'lyricist' THEN 0 WHEN 'composer' THEN 1 WHEN 'arranger' THEN 2 \
             WHEN 'translator' THEN 3 WHEN 'performer' THEN 4 ELSE 9 END, \
             c.position, p.name",
    )?;
    let rows = stmt.query_map(params![song_id], |row| {
        Ok(Credit {
            person_id: row.get(0)?,
            name: row.get(1)?,
            role: row.get(2)?,
            position: row.get(3)?,
            source: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 歌手照片那一列的 SQL 表达式。
///
/// `artists` 表是刮削时才建的（`scripts/migrate_db.py`），`check_schema` 并不要求它——
/// 没刮削过的库照样能用。硬 JOIN 会让人物页在这种库上直接报 "no such table"，
/// 所以先看表在不在，不在就给空串。每次查询都看一眼而不是开库时缓存：
/// 用户可能开着程序去刮削，表是中途才出现的。
///
/// 按显示名精确匹配。`artists.name` 存的是刮削时用的歌手名，和 `people.name` 同源；
/// 别名（ZTMY ↔ ずっと真夜中でいいのに。）不去猜——猜错比没图糟。
/// 别名用 `ar`：`collaborators` 里 `a` 已经是 `track_credits` 了。
fn artist_image_expr(conn: &Connection, name_column: &str) -> Result<String> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='artists')",
        [],
        |row| row.get(0),
    )?;
    Ok(if exists {
        format!(
            "COALESCE((SELECT ar.image_path FROM artists ar WHERE ar.name = {name_column}), '')"
        )
    } else {
        "''".to_string()
    })
}

pub(crate) fn people_by_role(conn: &Connection, role: &str, limit: i64) -> Result<Vec<PersonSummary>> {
    let image = artist_image_expr(conn, "p.name")?;
    let sql = format!(
        "SELECT p.id, p.name, COUNT(DISTINCT c.song_id) AS n, {image} \
         FROM track_credits c JOIN people p ON p.id = c.person_id \
         WHERE c.role = ?1 GROUP BY p.id, p.name \
         ORDER BY n DESC, p.name LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![role, limit], |row| {
        Ok(PersonSummary {
            id: row.get(0)?,
            name: row.get(1)?,
            role: role.to_string(),
            track_count: row.get(2)?,
            image_path: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn works_by_person(conn: &Connection, person_id: i64) -> Result<Vec<Track>> {
    let sql = format!(
        "SELECT DISTINCT {TRACK_COLUMNS} FROM track_credits c \
         JOIN songs s ON s.id = c.song_id WHERE c.person_id = ?1 \
         ORDER BY s.year, s.artist, s.title"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![person_id], track_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Collaboration Graph 的边查询。
///
/// 一次自连接就够了——这正是把「人」做成实体而不是留在 `songs.artist`
/// 字符串里的全部意义。
pub(crate) fn collaborators(conn: &Connection, person_id: i64, limit: i64) -> Result<Vec<Collaborator>> {
    let image = artist_image_expr(conn, "p.name")?;
    let sql = format!(
        "SELECT p.id, p.name, COUNT(DISTINCT a.song_id) AS shared, \
                GROUP_CONCAT(DISTINCT b.role), {image} \
         FROM track_credits a \
         JOIN track_credits b ON b.song_id = a.song_id AND b.person_id <> a.person_id \
         JOIN people p ON p.id = b.person_id \
         WHERE a.person_id = ?1 \
         GROUP BY p.id, p.name ORDER BY shared DESC, p.name LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![person_id, limit], |row| {
        let roles: Option<String> = row.get(3)?;
        Ok(Collaborator {
            person_id: row.get(0)?,
            name: row.get(1)?,
            shared_tracks: row.get(2)?,
            image_path: row.get(4)?,
            roles: roles
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 一首歌的全部歌词行，带分词。
///
/// 一次查两条 SQL（行 + 全部 token），不是每行一次——整首歌 60 行的话
/// 逐行查就是 61 次往返。
pub(crate) fn lyrics(conn: &Connection, song_id: &str) -> Result<Vec<LyricLine>> {
    let mut stmt = conn.prepare(
        "SELECT id, line_idx, time_sec, text FROM utterances \
         WHERE song_id = ?1 ORDER BY line_idx",
    )?;
    let mut lines: Vec<LyricLine> = stmt
        .query_map(params![song_id], |row| {
            Ok(LyricLine {
                utterance_id: row.get(0)?,
                line_idx: row.get(1)?,
                time_sec: row.get(2)?,
                text: row.get(3)?,
                tokens: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let ids: Vec<i64> = lines.iter().map(|l| l.utterance_id).collect();
    let mut tokens = tokens_for_lines(conn, &ids)?;
    for line in &mut lines {
        line.tokens = tokens.remove(&line.utterance_id).unwrap_or_default();
    }
    Ok(lines)
}

/// 一行歌词的分词。
pub(crate) fn line_tokens(conn: &Connection, utterance_id: i64) -> Result<Vec<LineToken>> {
    let mut stmt = conn.prepare(
        "SELECT surface, lemma, COALESCE(pos,'') FROM tokens \
         WHERE utterance_id = ?1 ORDER BY token_idx",
    )?;
    let rows = stmt.query_map(params![utterance_id], |row| {
        Ok(LineToken {
            surface: row.get(0)?,
            lemma: row.get(1)?,
            pos: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 批量取多行的分词，一次查询。
pub(crate) fn tokens_for_lines(
    conn: &Connection,
    utterance_ids: &[i64],
) -> Result<HashMap<i64, Vec<LineToken>>> {
    let mut out: HashMap<i64, Vec<LineToken>> = HashMap::new();
    for chunk in utterance_ids.chunks(900) {
        let sql = format!(
            "SELECT utterance_id, surface, lemma, COALESCE(pos,'') FROM tokens \
             WHERE utterance_id IN ({}) ORDER BY utterance_id, token_idx",
            placeholders(chunk.len())
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(chunk.iter()), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                LineToken {
                    surface: row.get(1)?,
                    lemma: row.get(2)?,
                    pos: row.get(3)?,
                },
            ))
        })?;
        for row in rows {
            let (id, token) = row?;
            out.entry(id).or_default().push(token);
        }
    }
    Ok(out)
}

/// 按 id 取一个人，带上他担任过的角色。
///
/// 命令面板点一条人物结果要能直接定位过去；`people_by_role` 只能按角色
/// 列表取，拿不到「这个 id 是谁」。
pub(crate) fn person_by_id(conn: &Connection, person_id: i64) -> Result<Option<PersonSummary>> {
    let image = artist_image_expr(conn, "p.name")?;
    let sql = format!(
        "SELECT p.id, p.name,                 COALESCE((SELECT c.role FROM track_credits c                           WHERE c.person_id = p.id                           GROUP BY c.role ORDER BY COUNT(*) DESC LIMIT 1), ''),                 (SELECT COUNT(DISTINCT c.song_id) FROM track_credits c WHERE c.person_id = p.id), {image}          FROM people p WHERE p.id = ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![person_id], |row| {
        Ok(PersonSummary {
            id: row.get(0)?,
            // 主角色取作品最多的那个——一个人可能既是歌手又是作曲
            name: row.get(1)?,
            role: row.get(2)?,
            track_count: row.get(3)?,
            image_path: row.get(4)?,
        })
    })?;
    Ok(rows.next().transpose()?)
}

// ────────────────────────────── 收藏 ──────────────────────────────

pub(crate) fn is_favorite(conn: &Connection, entity_type: &str, entity_id: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM favorites WHERE entity_type = ?1 AND entity_id = ?2",
        params![entity_type, entity_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// 切换收藏。返回切换后的状态。
///
/// 用「切换」而不是 add/remove 两个接口，是因为 UI 上就是一个心形按钮，
/// 分成两个反而要前端先查状态再决定调哪个，多一次往返还可能竞态。
pub(crate) fn toggle_favorite(
    conn: &Connection,
    entity_type: &str,
    entity_id: &str,
) -> Result<bool> {
    if is_favorite(conn, entity_type, entity_id)? {
        conn.execute(
            "DELETE FROM favorites WHERE entity_type = ?1 AND entity_id = ?2",
            params![entity_type, entity_id],
        )?;
        Ok(false)
    } else {
        conn.execute(
            "INSERT INTO favorites (entity_type, entity_id, created_at)              VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
            params![entity_type, entity_id],
        )?;
        Ok(true)
    }
}

/// 收藏的曲目，最近收藏的在前。
pub(crate) fn favorite_tracks(conn: &Connection, limit: i64) -> Result<Vec<Track>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.title, s.artist, COALESCE(s.album,''), s.album_id,                 COALESCE(s.year,''), COALESCE(s.genre,''), COALESCE(s.audio_path,''),                 COALESCE(s.cover_path,''), s.duration_sec,                 (SELECT COUNT(*) FROM utterances u WHERE u.song_id = s.id)          FROM favorites f JOIN songs s ON s.id = f.entity_id          WHERE f.entity_type = 'song'          ORDER BY f.created_at DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], track_from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ────────────────────────────── 分面 ──────────────────────────────

/// 按流派分组。空流派归到「未分类」而不是丢掉——
/// 库里 17% 的曲目没有流派，丢掉会让总数对不上。
pub(crate) fn genres(conn: &Connection, limit: i64) -> Result<Vec<FacetCount>> {
    facet(
        conn,
        "SELECT CASE WHEN genre IS NULL OR trim(genre) = '' THEN '未分类' ELSE genre END,                 COUNT(*), COALESCE(SUM(duration_sec), 0)          FROM songs GROUP BY 1 ORDER BY 2 DESC, 1 LIMIT ?1",
        limit,
    )
}

/// 按年代分组。年份为空的归到「年代不详」。
pub(crate) fn decades(conn: &Connection, limit: i64) -> Result<Vec<FacetCount>> {
    facet(
        conn,
        "SELECT CASE                   WHEN year IS NULL OR length(trim(year)) < 4 THEN '年代不详'                   ELSE substr(year, 1, 3) || '0s' END,                 COUNT(*), COALESCE(SUM(duration_sec), 0)          FROM songs GROUP BY 1 ORDER BY 1 LIMIT ?1",
        limit,
    )
}

fn facet(conn: &Connection, sql: &str, limit: i64) -> Result<Vec<FacetCount>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map(params![limit], |row| {
            Ok(FacetCount {
                label: row.get(0)?,
                track_count: row.get(1)?,
                total_duration_sec: row.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(with_artists: bool) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL,
                                  normalized_name TEXT NOT NULL DEFAULT '');
             CREATE TABLE track_credits (song_id TEXT, person_id INTEGER, role TEXT,
                                         position INTEGER DEFAULT 0, source TEXT DEFAULT '');
             INSERT INTO people (id, name) VALUES (1, 'ヨルシカ'), (2, 'n-buna'), (3, '月村手毬');
             INSERT INTO track_credits (song_id, person_id, role) VALUES
               ('001', 1, 'performer'), ('001', 2, 'composer'),
               ('002', 1, 'performer'), ('003', 3, 'performer');",
        )
        .unwrap();
        if with_artists {
            conn.execute_batch(
                "CREATE TABLE artists (name TEXT PRIMARY KEY, image_path TEXT, artist_type TEXT,
                                       country TEXT, formed TEXT, updated_at TEXT);
                 INSERT INTO artists (name, image_path) VALUES
                   ('ヨルシカ', 'raw/artists/ヨルシカ.jpg'), ('月村手毬', '');",
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn people_still_list_on_a_library_that_was_never_scraped() {
        // 没有 artists 表是正常状态，人物页不能因此报错
        let conn = fixture(false);
        let people = people_by_role(&conn, "performer", 10).unwrap();
        assert_eq!(people.len(), 2);
        assert!(people.iter().all(|p| p.image_path.is_empty()));

        let one = person_by_id(&conn, 1).unwrap().unwrap();
        assert!(one.image_path.is_empty());

        let peers = collaborators(&conn, 2, 10).unwrap();
        assert_eq!(peers.len(), 1);
        assert!(peers[0].image_path.is_empty());
    }

    #[test]
    fn a_performer_picks_up_their_photo_by_exact_name() {
        let conn = fixture(true);
        let people = people_by_role(&conn, "performer", 10).unwrap();
        let yorushika = people.iter().find(|p| p.name == "ヨルシカ").unwrap();
        assert_eq!(yorushika.image_path, "raw/artists/ヨルシカ.jpg");
        // 有 artists 行但没照片，不能串成别人的
        let temari = people.iter().find(|p| p.name == "月村手毬").unwrap();
        assert!(temari.image_path.is_empty());

        assert_eq!(
            person_by_id(&conn, 1).unwrap().unwrap().image_path,
            "raw/artists/ヨルシカ.jpg"
        );

        // 从作曲者那边点过来的合作者，头像要跟着
        let peers = collaborators(&conn, 2, 10).unwrap();
        assert_eq!(peers[0].name, "ヨルシカ");
        assert_eq!(peers[0].image_path, "raw/artists/ヨルシカ.jpg");

        // 从来没查过照片的人是空串，不是报错
        let peers = collaborators(&conn, 1, 10).unwrap();
        assert_eq!(peers[0].name, "n-buna");
        assert!(peers[0].image_path.is_empty());
    }
}
