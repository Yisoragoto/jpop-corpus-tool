//! 首页要的几样东西：上次听到哪、今天听了多少、听过的歌碰到了多少词、最近加了什么。
//!
//! 「今天」按用户本地日期算。库里的 `played_at` 是 UTC（`…Z`），
//! 所以调用方要把本地时区相对 UTC 的分钟数传进来（东八区是 +480）。
//! 不在后端读系统时区：前端和后端各自判断「今天」的话，跨午夜那几分钟两边会对不上。

use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

/// 「实词」：名词、动词、形容词、副词。助词、助动词这类每首歌都有，
/// 算进「碰到过多少词」只会把比例抬高、没有信息量。
const CONTENT_POS: &str = "('NOUN','VERB','ADJ','ADV')";

/// 上次听的那首，以及停在哪。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinueTrack {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub cover_path: String,
    pub duration_sec: Option<f64>,
    /// 最后一次播放停下的位置（秒）
    pub position_sec: f64,
    /// 最后一次是不是听完了。听完了界面给「再听一遍」，没听完给「从 x:xx 继续」
    pub completed: bool,
    pub last_played: String,
    pub play_count: i64,
    /// 停下的位置正在唱的那一行（时间戳不晚于停下位置的最后一行）。
    /// 没有带时间戳的歌词、或者停在第一行之前时是空串
    pub stopped_line: String,
}

/// 某一天（本地日期）听了多少。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayListening {
    pub listened_sec: f64,
    /// 听了几首（去重）
    pub songs: i64,
    /// 听到的歌词行数。每次播放算 `[停下位置 - 听了多久, 停下位置]` 这段时间里唱到的行；
    /// 同一首听两遍算两遍——这是「听了多少」，不是「听过多少种」
    pub lines_heard: i64,
    /// 连续有收听的天数。今天还没听的话从昨天往回数（今天还没过完，不算断）
    pub streak_days: i64,
}

/// 听过的歌覆盖了语料里多少实词。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reach {
    /// 听满 [`HEARD_MIN_SEC`] 秒的歌有几首
    pub heard_songs: i64,
    /// 这些歌里出现过的实词词元数（去重）
    pub heard_lemmas: i64,
    /// 全语料的实词词元数（去重）
    pub total_lemmas: i64,
}

/// 「听过」的门槛，和播放排行（`most_played(30.0, …)`）用同一个数。
/// 点开五秒就切走的不算听过。
pub const HEARD_MIN_SEC: f64 = 30.0;

/// 最近添加的一首。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedTrack {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub cover_path: String,
    pub added_at: String,
}

/// 本地时区偏移 → SQLite 的日期修饰符（`+480 minutes`）。
fn offset_modifier(utc_offset_minutes: i32) -> Result<String> {
    // 世界上的时区在 UTC-12 到 UTC+14 之间
    if !(-720..=840).contains(&utc_offset_minutes) {
        bail!("时区偏移 {utc_offset_minutes} 分钟不合理");
    }
    Ok(format!("{utc_offset_minutes:+} minutes"))
}

pub(crate) fn continue_track(conn: &Connection) -> Result<Option<ContinueTrack>> {
    let row = conn
        .query_row(
            "SELECT h.song_id, s.title, s.artist, COALESCE(s.cover_path,''), s.duration_sec, \
                    h.position_sec, h.completed, h.played_at, \
                    (SELECT COUNT(*) FROM play_history c WHERE c.song_id = h.song_id) \
             FROM play_history h JOIN songs s ON s.id = h.song_id \
             ORDER BY h.played_at DESC, h.id DESC LIMIT 1",
            [],
            |r| {
                Ok(ContinueTrack {
                    song_id: r.get(0)?,
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    cover_path: r.get(3)?,
                    duration_sec: r.get(4)?,
                    position_sec: r.get(5)?,
                    completed: r.get::<_, i64>(6)? != 0,
                    last_played: r.get(7)?,
                    play_count: r.get(8)?,
                    stopped_line: String::new(),
                })
            },
        )
        .optional()?;
    let Some(mut track) = row else { return Ok(None) };
    track.stopped_line = conn
        .query_row(
            "SELECT text FROM utterances \
             WHERE song_id = ?1 AND time_sec IS NOT NULL AND time_sec <= ?2 AND trim(text) <> '' \
             ORDER BY time_sec DESC, line_idx DESC LIMIT 1",
            params![track.song_id, track.position_sec],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_default();
    Ok(Some(track))
}

pub(crate) fn today(conn: &Connection, utc_offset_minutes: i32) -> Result<DayListening> {
    let offset = offset_modifier(utc_offset_minutes)?;
    let (listened_sec, songs, lines_heard): (f64, i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(h.listened_sec), 0), COUNT(DISTINCT h.song_id), \
                COALESCE(SUM((SELECT COUNT(*) FROM utterances u \
                              WHERE u.song_id = h.song_id AND u.time_sec IS NOT NULL \
                                AND u.time_sec <= h.position_sec \
                                AND u.time_sec >= h.position_sec - h.listened_sec)), 0) \
         FROM play_history h \
         WHERE date(h.played_at, ?1) = date('now', ?1)",
        params![offset],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;

    // 本地日期换成连续的整数（儒略日从正午起算，+0.5 再取整就是按午夜换日）
    let today: i64 = conn.query_row(
        "SELECT CAST(julianday('now', ?1) + 0.5 AS INTEGER)",
        params![offset],
        |r| r.get(0),
    )?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT CAST(julianday(played_at, ?1) + 0.5 AS INTEGER) AS d \
         FROM play_history ORDER BY d DESC",
    )?;
    let days = stmt
        .query_map(params![offset], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(DayListening {
        listened_sec,
        songs,
        lines_heard,
        streak_days: streak(&days, today),
    })
}

/// `days` 是降序去重的日编号。今天没有记录时从昨天起数。
fn streak(days: &[i64], today: i64) -> i64 {
    let start = if days.first() == Some(&today) { today } else { today - 1 };
    let mut expect = start;
    let mut count = 0;
    for &day in days.iter().skip_while(|&&d| d > start) {
        if day != expect {
            break;
        }
        count += 1;
        expect -= 1;
    }
    count
}

pub(crate) fn reach(conn: &Connection) -> Result<Reach> {
    let (heard_songs, heard_lemmas): (i64, i64) = conn.query_row(
        &format!(
            "WITH heard AS (SELECT DISTINCT song_id FROM play_history WHERE listened_sec >= ?1) \
             SELECT (SELECT COUNT(*) FROM heard), \
                    (SELECT COUNT(DISTINCT t.lemma) FROM tokens t \
                     JOIN utterances u ON u.id = t.utterance_id \
                     WHERE u.song_id IN (SELECT song_id FROM heard) AND t.pos IN {CONTENT_POS})"
        ),
        params![HEARD_MIN_SEC],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let total_lemmas: i64 = conn.query_row(
        &format!("SELECT COUNT(DISTINCT lemma) FROM tokens WHERE pos IN {CONTENT_POS}"),
        [],
        |r| r.get(0),
    )?;
    Ok(Reach {
        heard_songs,
        heard_lemmas,
        total_lemmas,
    })
}

pub(crate) fn recently_added(conn: &Connection, limit: i64) -> Result<Vec<AddedTrack>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, artist, COALESCE(cover_path,''), added_at FROM songs \
         WHERE added_at IS NOT NULL ORDER BY added_at DESC, id DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok(AddedTrack {
                song_id: r.get(0)?,
                title: r.get(1)?,
                artist: r.get(2)?,
                cover_path: r.get(3)?,
                added_at: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 给没有 `added_at` 的歌补上：取音频文件的创建时间，取不到就用修改时间。
///
/// 只补空的，所以每次启动都调也只在第一次真的干活。文件找不到的歌留空——
/// 编一个时间进去，它就会冒充「最近添加」排在前面。
pub(crate) fn backfill_added_at(conn: &mut Connection) -> Result<usize> {
    let todo: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, audio_path FROM songs \
             WHERE added_at IS NULL AND audio_path IS NOT NULL AND trim(audio_path) <> ''",
        )?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    if todo.is_empty() {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    let mut written = 0;
    {
        let mut stmt = tx.prepare(
            "UPDATE songs SET added_at = strftime('%Y-%m-%dT%H:%M:%SZ', ?2, 'unixepoch') \
             WHERE id = ?1 AND added_at IS NULL",
        )?;
        for (song_id, path) in todo {
            let Some(secs) = file_birth_unix(std::path::Path::new(&path)) else { continue };
            written += stmt.execute(params![song_id, secs])?;
        }
    }
    tx.commit()?;
    Ok(written)
}

fn file_birth_unix(path: &std::path::Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let when = meta.created().or_else(|_| meta.modified()).ok()?;
    let secs = when.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::Corpus::ensure_schema_on(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO songs (id, title, artist, cover_path, duration_sec) VALUES
                ('001', '夜行', 'ヨルシカ', 'c1.jpg', 200.0),
                ('002', '春泥棒', 'ヨルシカ', '', 300.0);
             INSERT INTO utterances (id, song_id, line_idx, time_sec, text) VALUES
                (1, '001', 0, 10.0, 'いっそ'),
                (2, '001', 1, 20.0, '夜を'),
                (3, '001', 2, 30.0, ''),
                (4, '001', 3, 40.0, '歩く'),
                (5, '002', 0, 5.0, '風');
             INSERT INTO tokens (utterance_id, token_idx, surface, lemma, pos) VALUES
                (1, 0, 'いっそ', 'いっそ', 'ADV'),
                (2, 0, '夜', '夜', 'NOUN'),
                (2, 1, 'を', 'を', 'ADP'),
                (4, 0, '歩く', '歩く', 'VERB'),
                (5, 0, '風', '風', 'NOUN');",
        )
        .unwrap();
        conn
    }

    fn play(conn: &Connection, song: &str, at: &str, listened: f64, position: f64, completed: bool) {
        conn.execute(
            "INSERT INTO play_history (song_id, played_at, listened_sec, position_sec, completed) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![song, at, listened, position, completed as i64],
        )
        .unwrap();
    }

    #[test]
    fn continue_is_the_latest_play_and_names_the_line_it_stopped_on() {
        let conn = db();
        assert_eq!(continue_track(&conn).unwrap(), None, "没听过就没有");
        play(&conn, "002", "2026-10-01T10:00:00Z", 300.0, 300.0, true);
        play(&conn, "001", "2026-10-02T10:00:00Z", 35.0, 35.0, false);

        let t = continue_track(&conn).unwrap().unwrap();
        assert_eq!(t.song_id, "001");
        assert_eq!(t.position_sec, 35.0);
        assert!(!t.completed);
        assert_eq!(t.play_count, 1);
        assert_eq!(t.cover_path, "c1.jpg");
        // 35 秒处：30 秒那行是空行，跳过，落在 20 秒那行
        assert_eq!(t.stopped_line, "夜を");
    }

    #[test]
    fn stopping_before_the_first_line_names_no_line() {
        let conn = db();
        play(&conn, "001", "2026-10-02T10:00:00Z", 5.0, 5.0, false);
        assert_eq!(continue_track(&conn).unwrap().unwrap().stopped_line, "");
    }

    #[test]
    fn today_counts_only_plays_from_today() {
        let conn = db();
        // 拿 SQLite 的「现在」造数据，免得测试跨午夜时失败
        let now: String = conn
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')", [], |r| r.get(0))
            .unwrap();
        play(&conn, "001", &now, 25.0, 45.0, false);
        play(&conn, "001", &now, 200.0, 200.0, true);
        play(&conn, "002", "2000-01-01T00:00:00Z", 300.0, 300.0, true);

        let day = today(&conn, 0).unwrap();
        assert_eq!(day.listened_sec, 225.0);
        assert_eq!(day.songs, 1);
        // 第一次：20–45 秒唱到 20、30、40 三行；第二次：整首四行
        assert_eq!(day.lines_heard, 7);
        assert_eq!(day.streak_days, 1);
    }

    #[test]
    fn a_bad_offset_is_refused() {
        let conn = db();
        assert!(today(&conn, 2000).is_err());
    }

    #[test]
    fn a_streak_survives_until_today_is_over() {
        assert_eq!(streak(&[100, 99, 98, 96], 100), 3);
        assert_eq!(streak(&[99, 98, 96], 100), 2, "今天还没听，从昨天数");
        assert_eq!(streak(&[98, 97], 100), 0, "昨天也没听就断了");
        assert_eq!(streak(&[], 100), 0);
    }

    #[test]
    fn reach_counts_content_words_of_songs_heard_long_enough() {
        let conn = db();
        play(&conn, "001", "2026-10-02T10:00:00Z", 60.0, 60.0, false);
        play(&conn, "002", "2026-10-02T11:00:00Z", 10.0, 10.0, false);
        let r = reach(&conn).unwrap();
        assert_eq!(r.heard_songs, 1, "002 只听了 10 秒，不算听过");
        assert_eq!(r.heard_lemmas, 3, "いっそ 夜 歩く；を 是助词不算");
        assert_eq!(r.total_lemmas, 4);
    }

    #[test]
    fn backfill_fills_only_empty_rows_from_existing_files() {
        let mut conn = db();
        let dir = std::env::temp_dir().join(format!("jp-home-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.flac");
        std::fs::write(&file, b"x").unwrap();
        conn.execute_batch(&format!(
            "UPDATE songs SET audio_path = '{}' WHERE id = '001';
             UPDATE songs SET audio_path = '{}' WHERE id = '002';
             INSERT INTO songs (id, title, artist, audio_path, added_at)
                 VALUES ('003', 'x', 'y', '{}', '2020-01-01T00:00:00Z');",
            file.display(),
            dir.join("missing.flac").display(),
            file.display(),
        ))
        .unwrap();

        assert_eq!(backfill_added_at(&mut conn).unwrap(), 1);
        let got: Vec<(String, Option<String>)> = conn
            .prepare("SELECT id, added_at FROM songs ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(got[0].1.as_deref().is_some_and(|s| s.ends_with('Z') && s.len() == 20), "{got:?}");
        assert_eq!(got[1].1, None, "文件不在就留空，不编时间");
        assert_eq!(got[2].1.as_deref(), Some("2020-01-01T00:00:00Z"), "已有的不动");
        assert_eq!(backfill_added_at(&mut conn).unwrap(), 0, "第二次没有要补的");

        let added = recently_added(&conn, 5).unwrap();
        assert_eq!(added.len(), 2);
        assert_eq!(added[0].song_id, "001", "新的在前");
        std::fs::remove_dir_all(&dir).ok();
    }
}
