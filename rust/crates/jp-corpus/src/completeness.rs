//! 库的完整度：每一种 metadata 有多少首（张）已经齐了。刮削页一行一行列出来，缺的那行给一个按钮。
//!
//! 封面按「文件真的在盘上」算，不是只看路径有没有填——路径记着、文件被删了，
//! 界面上就是一块占位，对用户来说就是没有。

use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;

/// 一行：齐了几个、一共几个。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Count {
    pub done: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Completeness {
    /// 刮削认出来的（`scrape_state.status = 'success'`）
    pub identified: Count,
    /// 复核时跳过的。不算缺，单独报出来，免得「211 首只认出 205」让人以为有 6 首没管
    pub skipped: i64,
    pub track_covers: Count,
    pub album_covers: Count,
    pub lyrics: Count,
    pub durations: Count,
}

pub(crate) fn completeness(conn: &Connection) -> Result<Completeness> {
    let scalar = |sql: &str| -> Result<i64> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };
    let tracks = scalar("SELECT COUNT(*) FROM songs")?;

    // 新建的库还没刮过时可能连这张表都没有
    let has_scrape_state = scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='scrape_state'",
    )? > 0;
    let (identified, skipped) = if has_scrape_state {
        (
            scalar(
                "SELECT COUNT(DISTINCT song_id) FROM scrape_state \
                 WHERE status = 'success' AND song_id IN (SELECT id FROM songs)",
            )?,
            scalar(
                "SELECT COUNT(DISTINCT song_id) FROM scrape_state \
                 WHERE status = 'skipped' AND song_id IN (SELECT id FROM songs)",
            )?,
        )
    } else {
        (0, 0)
    };

    Ok(Completeness {
        identified: Count {
            done: identified,
            total: tracks,
        },
        skipped,
        track_covers: Count {
            done: existing_files(conn, "SELECT cover_path FROM songs")?,
            total: tracks,
        },
        album_covers: Count {
            done: existing_files(conn, "SELECT artwork_path FROM albums")?,
            total: scalar("SELECT COUNT(*) FROM albums")?,
        },
        lyrics: Count {
            done: scalar(
                "SELECT COUNT(*) FROM songs s WHERE EXISTS (SELECT 1 FROM utterances u WHERE u.song_id = s.id)",
            )?,
            total: tracks,
        },
        durations: Count {
            done: scalar("SELECT COUNT(*) FROM songs WHERE duration_sec > 0")?,
            total: tracks,
        },
    })
}

/// 查询结果里有几个路径真的指向盘上的文件。
fn existing_files(conn: &Connection, sql: &str) -> Result<i64> {
    let mut stmt = conn.prepare(sql)?;
    let paths = stmt
        .query_map([], |r| r.get::<_, Option<String>>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(paths
        .into_iter()
        .flatten()
        .filter(|p| !p.trim().is_empty() && std::path::Path::new(p).is_file())
        .count() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_row_counts_what_is_actually_there() {
        let conn = Connection::open_in_memory().unwrap();
        crate::Corpus::ensure_schema_on(&conn).unwrap();
        let dir = std::env::temp_dir().join(format!("jp-complete-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cover = dir.join("c.jpg");
        std::fs::write(&cover, b"x").unwrap();
        let cover = cover.display().to_string();
        let gone = dir.join("gone.jpg").display().to_string();

        conn.execute_batch(&format!(
            "INSERT INTO songs (id, title, artist, cover_path, duration_sec) VALUES
                ('001', 'a', 'x', '{cover}', 200.0),
                ('002', 'b', 'x', '{gone}', NULL),
                ('003', 'c', 'x', '', 0);
             INSERT INTO albums (id, title, normalized_title, artwork_path) VALUES (1, 'A', 'a', '{cover}'), (2, 'B', 'b', '');
             INSERT INTO utterances (song_id, line_idx, text) VALUES ('001', 0, 'あ'), ('001', 1, 'い');"
        ))
        .unwrap();

        let got = completeness(&conn).unwrap();
        assert_eq!(got.identified, Count { done: 0, total: 3 }, "没刮过");
        assert_eq!(got.track_covers, Count { done: 1, total: 3 }, "文件不在的不算有封面");
        assert_eq!(got.album_covers, Count { done: 1, total: 2 });
        assert_eq!(got.lyrics, Count { done: 1, total: 3 });
        assert_eq!(got.durations, Count { done: 1, total: 3 }, "0 秒不算有时长");
        std::fs::remove_dir_all(&dir).ok();
    }
}
