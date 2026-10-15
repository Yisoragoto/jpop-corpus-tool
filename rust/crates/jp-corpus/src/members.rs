//! 乐队成员。
//!
//! 曲库里原来只有「人」和「署名」：きのこ帝国 和 佐藤千亜妃 是两个平级的人，
//! 界面上就会各占一行、各写「19 首」，看着像重复。这一层补上「谁属于哪个乐队」。
//!
//! **一首歌属于谁，只看演唱署名**——成员关系不改变任何一首歌的归属。
//! 佐藤千亜妃 给 きのこ帝国 写的歌仍然只属于乐队，她的「个人作品」只算她自己演唱署名的。
//! 这一层只回答「这个乐队有谁、这个人在哪个乐队待过、什么时候」。
//!
//! 关系有三个来源，优先级从低到高：
//!
//! * `musicbrainz`——刮削拿到的；
//! * `confirmed`——[`suggestions`] 推测出来、你点了确认的；
//! * `manual`——你手动加的、改的。
//!
//! **自动流程永远不覆盖后两种**，也不会把你否掉的（`status = rejected`）加回来：
//! 否掉的那一行一直留着，就是为了挡住下一次刮削和推测。

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use jp_normalize::matching_key;

/// 一条成员关系，两头的名字和照片都带上，界面不用再查。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Membership {
    pub band_id: i64,
    pub band_name: String,
    pub member_id: i64,
    pub member_name: String,
    /// 在队里做什么：主唱、吉他、作词作曲……
    pub part: String,
    /// YYYY、YYYY-MM 或 YYYY-MM-DD；不知道是空串
    pub joined: String,
    /// 离开（或乐队结束）的日期。空串 = 现在还在
    pub left_at: String,
    /// musicbrainz / confirmed / manual
    pub source: String,
    /// 乐队的照片（看成员时用）
    pub band_image: String,
    /// 成员的照片（看乐队时用）
    pub member_image: String,
}

impl Membership {
    /// 已经离开，或者乐队已经结束
    pub fn is_former(&self) -> bool {
        !self.left_at.is_empty()
    }
}

/// 一条推测：这个人写的歌几乎全是给这个乐队的，可能是成员。**不会自动生效**。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipSuggestion {
    pub band_id: i64,
    pub band_name: String,
    pub member_id: i64,
    pub member_name: String,
    /// 给这个乐队写（作词 / 作曲 / 编曲）的歌数
    pub shared: i64,
    /// 他写的、但不是他自己演唱的歌一共多少首
    pub written: i64,
}

/// 写入一条关系的结果。被挡下来的要说清楚为什么，界面和日志要用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PutOutcome {
    Inserted,
    Updated,
    /// 新来源的优先级比已有的低（比如刮削想改你手动写的）
    KeptExisting,
    /// 你否掉过这一条
    Rejected,
}

const SOURCES: [&str; 3] = ["musicbrainz", "confirmed", "manual"];

fn rank(source: &str) -> usize {
    SOURCES.iter().position(|s| *s == source).unwrap_or(0)
}

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%S','now')";

/// 按名字找人，没有就建一个。名字归一化后相等就算同一个人（和导入一致）。
pub fn ensure_person(conn: &Connection, name: &str) -> Result<i64> {
    let clean = name.trim();
    let key = matching_key(clean);
    if key.is_empty() {
        bail!("名字是空的");
    }
    if let Some(id) = conn
        .query_row("SELECT id FROM people WHERE normalized_name=?1", params![key], |r| r.get(0))
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        &format!(
            "INSERT INTO people (name, normalized_name, sort_name, created_at) VALUES (?1, ?2, ?1, {NOW})"
        ),
        params![clean, key],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 写一条关系。`source` 是 musicbrainz / confirmed / manual 之一。
///
/// * 已有的行是你否掉的：只有 `manual` 能把它恢复（你自己改主意了），其余一律挡下；
/// * 已有的行来源优先级更高：挡下（刮削不改你确认过、手动写过的）；
/// * 否则写入。同来源重复写是更新（刮削第二次拿到了更全的日期）。
pub fn put(
    conn: &Connection,
    band_id: i64,
    member_id: i64,
    part: &str,
    joined: &str,
    left_at: &str,
    source: &str,
) -> Result<PutOutcome> {
    if band_id == member_id {
        bail!("一个人不能是自己的成员");
    }
    if !SOURCES.contains(&source) {
        bail!("不认识的来源：{source}");
    }
    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT source, status FROM band_members WHERE band_id=?1 AND member_id=?2",
            params![band_id, member_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match &existing {
        Some((_, status)) if status == "rejected" && source != "manual" => return Ok(PutOutcome::Rejected),
        Some((old, status)) if status != "rejected" && rank(old) > rank(source) => {
            return Ok(PutOutcome::KeptExisting);
        }
        _ => {}
    }
    conn.execute(
        &format!(
            "INSERT INTO band_members (band_id, member_id, part, joined, left_at, source, status, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', {NOW})
             ON CONFLICT(band_id, member_id) DO UPDATE SET
                part = excluded.part, joined = excluded.joined, left_at = excluded.left_at,
                source = excluded.source, status = 'active', updated_at = excluded.updated_at"
        ),
        params![band_id, member_id, part.trim(), joined.trim(), left_at.trim(), source],
    )
    .context("写成员关系失败")?;
    Ok(if existing.is_some() { PutOutcome::Updated } else { PutOutcome::Inserted })
}

/// 否掉一条关系（或一条推测）。行留着、标成 rejected，挡住以后的刮削和推测。
pub fn reject(conn: &Connection, band_id: i64, member_id: i64) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT INTO band_members (band_id, member_id, source, status, updated_at)
             VALUES (?1, ?2, 'manual', 'rejected', {NOW})
             ON CONFLICT(band_id, member_id) DO UPDATE SET
                source = 'manual', status = 'rejected', updated_at = excluded.updated_at"
        ),
        params![band_id, member_id],
    )
    .context("否掉成员关系失败")?;
    Ok(())
}

const SELECT: &str = "SELECT m.band_id, b.name, m.member_id, p.name, m.part, m.joined, m.left_at, m.source,
            COALESCE(ab.image_path, ''), COALESCE(ap.image_path, '')
     FROM band_members m
     JOIN people b ON b.id = m.band_id
     JOIN people p ON p.id = m.member_id
     LEFT JOIN artists ab ON ab.name = b.name
     LEFT JOIN artists ap ON ap.name = p.name
     WHERE m.status = 'active'";

fn read(row: &rusqlite::Row) -> rusqlite::Result<Membership> {
    Ok(Membership {
        band_id: row.get(0)?,
        band_name: row.get(1)?,
        member_id: row.get(2)?,
        member_name: row.get(3)?,
        part: row.get(4)?,
        joined: row.get(5)?,
        left_at: row.get(6)?,
        source: row.get(7)?,
        band_image: row.get(8)?,
        member_image: row.get(9)?,
    })
}

/// 一个乐队的成员：现役在前，同一组里按加入时间、再按名字。
pub fn members_of(conn: &Connection, band_id: i64) -> Result<Vec<Membership>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} AND m.band_id = ?1
         ORDER BY m.left_at <> '', m.joined = '', m.joined, p.name"
    ))?;
    let rows = stmt.query_map(params![band_id], read)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 一个人待过的乐队：现在还在的在前。
pub fn bands_of(conn: &Connection, member_id: i64) -> Result<Vec<Membership>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} AND m.member_id = ?1
         ORDER BY m.left_at <> '', m.joined DESC, b.name"
    ))?;
    let rows = stmt.query_map(params![member_id], read)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 全库的成员关系（关系网、左栏分组一次拿全）。
pub fn all(conn: &Connection) -> Result<Vec<Membership>> {
    let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY b.name, m.joined, p.name"))?;
    let rows = stmt.query_map([], read)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 推测成员：一个人写的歌（作词 / 作曲 / 编曲，**不算他自己演唱的**）里，
/// 至少 `min_share` 比例、至少 `min_songs` 首是给同一个演唱者的，就提示一下。
///
/// 只是提示，**不写库**。已经有关系（不管是哪个来源、还是被你否掉了）的组合不再提。
/// 演唱者确认是个人（`artists.artist_type = 'Person'`）的不提——那是长期合作，不是成员。
///
/// 为什么排除「他自己演唱的歌」：佐藤千亜妃 在 きのこ帝国 之后自己发歌，
/// 那些歌的词曲也是她写的。算进分母的话她给乐队写的比例就被稀释了，推不出来。
pub fn suggestions(conn: &Connection, min_songs: i64, min_share: f64) -> Result<Vec<MembershipSuggestion>> {
    let mut stmt = conn.prepare(
        "WITH written AS (
             -- 每个人写过的歌（去掉他自己演唱的那些）
             SELECT DISTINCT c.person_id, c.song_id
             FROM track_credits c
             WHERE c.role <> 'performer'
               AND NOT EXISTS (SELECT 1 FROM track_credits s
                               WHERE s.song_id = c.song_id AND s.person_id = c.person_id AND s.role = 'performer')
         ),
         totals AS (SELECT person_id, COUNT(*) AS n FROM written GROUP BY person_id),
         pairs AS (
             SELECT w.person_id AS member_id, perf.person_id AS band_id, COUNT(DISTINCT w.song_id) AS shared
             FROM written w
             JOIN track_credits perf ON perf.song_id = w.song_id AND perf.role = 'performer'
             WHERE perf.person_id <> w.person_id
             GROUP BY w.person_id, perf.person_id
         )
         SELECT pr.band_id, b.name, pr.member_id, p.name, pr.shared, t.n
         FROM pairs pr
         JOIN totals t ON t.person_id = pr.member_id
         JOIN people b ON b.id = pr.band_id
         JOIN people p ON p.id = pr.member_id
         LEFT JOIN artists a ON a.name = b.name
         WHERE pr.shared >= ?1
           AND CAST(pr.shared AS REAL) / t.n >= ?2
           AND COALESCE(a.artist_type, '') <> 'Person'
           AND NOT EXISTS (SELECT 1 FROM band_members m WHERE m.band_id = pr.band_id AND m.member_id = pr.member_id)
         ORDER BY pr.shared DESC, b.name, p.name",
    )?;
    let rows = stmt.query_map(params![min_songs, min_share], |r| {
        Ok(MembershipSuggestion {
            band_id: r.get(0)?,
            band_name: r.get(1)?,
            member_id: r.get(2)?,
            member_name: r.get(3)?,
            shared: r.get(4)?,
            written: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// きのこ帝国（乐队）：佐藤千亜妃 词曲 3 首；乐队结束后她自己唱 2 首（词曲也是她）。
    /// ヨルシカ：n-buna 词曲 4 首，另外给 suis 写过 1 首。
    /// Vaundy 是个人歌手，编曲人 X 给他编了 3 首——长期合作，不是成员。
    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::Corpus::ensure_schema_on(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO people (id, name, normalized_name) VALUES
                (1, 'きのこ帝国', 'きのこ帝国'), (2, '佐藤千亜妃', '佐藤千亜妃'),
                (3, 'ヨルシカ', 'ヨルシカ'), (4, 'n-buna', 'nbuna'), (5, 'suis', 'suis'),
                (6, 'Vaundy', 'vaundy'), (7, '编曲人X', '编曲人x');
             INSERT INTO artists (name, artist_type) VALUES ('きのこ帝国', 'Group'), ('Vaundy', 'Person');
             INSERT INTO track_credits (song_id, person_id, role) VALUES
                ('k1', 1, 'performer'), ('k1', 2, 'lyricist'), ('k1', 2, 'composer'),
                ('k2', 1, 'performer'), ('k2', 2, 'lyricist'),
                ('k3', 1, 'performer'), ('k3', 2, 'composer'),
                ('s1', 2, 'performer'), ('s1', 2, 'lyricist'),
                ('s2', 2, 'performer'), ('s2', 2, 'composer'),
                ('y1', 3, 'performer'), ('y1', 4, 'lyricist'),
                ('y2', 3, 'performer'), ('y2', 4, 'lyricist'),
                ('y3', 3, 'performer'), ('y3', 4, 'composer'),
                ('y4', 3, 'performer'), ('y4', 4, 'composer'),
                ('x1', 5, 'performer'), ('x1', 4, 'composer'),
                ('v1', 6, 'performer'), ('v1', 7, 'arranger'),
                ('v2', 6, 'performer'), ('v2', 7, 'arranger'),
                ('v3', 6, 'performer'), ('v3', 7, 'arranger');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn a_member_profile_gets_the_band_songs_without_owning_them() {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO songs (id, title, artist) VALUES
                ('k1', 'k1', 'きのこ帝国'), ('k2', 'k2', 'きのこ帝国'), ('k3', 'k3', 'きのこ帝国'),
                ('k4', 'k4', 'きのこ帝国'), ('s1', 's1', '佐藤千亜妃'), ('s2', 's2', '佐藤千亜妃');
             INSERT INTO track_credits (song_id, person_id, role) VALUES ('k4', 1, 'performer');",
        )
        .unwrap();
        put(&conn, 1, 2, "主唱", "2007", "2019", "musicbrainz").unwrap();
        let works = crate::library::person_works(&conn, 2).unwrap();
        let ids: Vec<&str> = works.iter().map(|w| w.track.id.as_str()).collect();
        // k4 她没署名，但是乐队的歌，也要拿到
        assert_eq!(works.len(), 6, "{ids:?}");
        let k4 = works.iter().find(|w| w.track.id == "k4").unwrap();
        assert!(k4.roles.is_empty());
        assert_eq!(k4.performer_ids, vec![1]);
        let s1 = works.iter().find(|w| w.track.id == "s1").unwrap();
        assert_eq!(s1.performer_ids, vec![2]);
        assert!(s1.roles.contains(&"performer".to_string()));

        // 否掉之后乐队的歌不再跟过来
        reject(&conn, 1, 2).unwrap();
        assert_eq!(crate::library::person_works(&conn, 2).unwrap().len(), 5);
    }

    #[test]
    fn a_band_lists_its_members_and_a_member_lists_the_band() {
        let conn = db();
        assert_eq!(put(&conn, 1, 2, "主唱 · 作词作曲", "2007", "2019", "musicbrainz").unwrap(), PutOutcome::Inserted);
        let members = members_of(&conn, 1).unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].member_name, "佐藤千亜妃");
        assert!(members[0].is_former());
        let bands = bands_of(&conn, 2).unwrap();
        assert_eq!(bands[0].band_name, "きのこ帝国");
    }

    #[test]
    fn current_members_come_before_former_ones() {
        let conn = db();
        put(&conn, 3, 5, "主唱", "2017", "2020", "manual").unwrap();
        put(&conn, 3, 4, "作词作曲", "2017", "", "manual").unwrap();
        let names: Vec<_> = members_of(&conn, 3).unwrap().into_iter().map(|m| m.member_name).collect();
        assert_eq!(names, ["n-buna", "suis"]);
    }

    #[test]
    fn scraping_never_overwrites_what_you_confirmed_or_wrote() {
        let conn = db();
        put(&conn, 1, 2, "主唱", "2007", "2019", "manual").unwrap();
        assert_eq!(put(&conn, 1, 2, "Vocals", "2008", "", "musicbrainz").unwrap(), PutOutcome::KeptExisting);
        let m = &members_of(&conn, 1).unwrap()[0];
        assert_eq!((m.part.as_str(), m.joined.as_str(), m.left_at.as_str()), ("主唱", "2007", "2019"));

        put(&conn, 3, 4, "", "", "", "confirmed").unwrap();
        assert_eq!(put(&conn, 3, 4, "Guitar", "2017", "", "musicbrainz").unwrap(), PutOutcome::KeptExisting);
    }

    #[test]
    fn a_rejected_membership_stays_rejected_until_you_add_it_yourself() {
        let conn = db();
        put(&conn, 3, 5, "主唱", "", "", "musicbrainz").unwrap();
        reject(&conn, 3, 5).unwrap();
        assert!(members_of(&conn, 3).unwrap().is_empty(), "否掉之后不再显示");
        assert_eq!(put(&conn, 3, 5, "主唱", "", "", "musicbrainz").unwrap(), PutOutcome::Rejected);
        assert_eq!(put(&conn, 3, 5, "主唱", "", "", "confirmed").unwrap(), PutOutcome::Rejected);
        // 你自己手动加回来是可以的
        assert_eq!(put(&conn, 3, 5, "主唱", "", "", "manual").unwrap(), PutOutcome::Updated);
        assert_eq!(members_of(&conn, 3).unwrap().len(), 1);
    }

    #[test]
    fn the_same_source_can_refresh_its_own_row() {
        let conn = db();
        put(&conn, 1, 2, "Vocals", "2007", "", "musicbrainz").unwrap();
        assert_eq!(put(&conn, 1, 2, "Vocals", "2007", "2019", "musicbrainz").unwrap(), PutOutcome::Updated);
        assert_eq!(members_of(&conn, 1).unwrap()[0].left_at, "2019");
    }

    #[test]
    fn nobody_is_their_own_member_and_unknown_sources_are_refused() {
        let conn = db();
        assert!(put(&conn, 1, 1, "", "", "", "manual").is_err());
        assert!(put(&conn, 1, 2, "", "", "", "guess").is_err());
    }

    /// 佐藤千亜妃 自己唱的 2 首不算分母，她写的 3 首全是 きのこ帝国 的 → 推测出来。
    /// n-buna 写的 5 首里 4 首是 ヨルシカ 的（80%）→ 推测出来；suis 那 1 首不够。
    /// 编曲人X 全是给 Vaundy 的，但 Vaundy 是个人 → 不推测。
    #[test]
    fn suggestions_find_the_writers_who_only_write_for_one_band() {
        let conn = db();
        let got: Vec<_> = suggestions(&conn, 3, 0.8)
            .unwrap()
            .into_iter()
            .map(|s| (s.band_name, s.member_name, s.shared, s.written))
            .collect();
        assert_eq!(
            got,
            [
                ("ヨルシカ".to_string(), "n-buna".to_string(), 4, 5),
                ("きのこ帝国".to_string(), "佐藤千亜妃".to_string(), 3, 3),
            ]
        );
    }

    #[test]
    fn suggestions_skip_pairs_you_already_decided_on() {
        let conn = db();
        reject(&conn, 3, 4).unwrap();
        put(&conn, 1, 2, "", "", "", "confirmed").unwrap();
        assert!(suggestions(&conn, 3, 0.8).unwrap().is_empty());
    }

    #[test]
    fn people_are_matched_by_normalized_name() {
        let conn = db();
        assert_eq!(ensure_person(&conn, "N-Buna").unwrap(), 4);
        let new = ensure_person(&conn, "ドラマー").unwrap();
        assert!(new > 7);
        assert_eq!(ensure_person(&conn, " ドラマー ").unwrap(), new);
        assert!(ensure_person(&conn, "  ").is_err());
    }
}
