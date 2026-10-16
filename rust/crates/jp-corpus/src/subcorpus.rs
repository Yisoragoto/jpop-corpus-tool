//! 子语料：按元数据圈出一批歌，检索和分析都在这批歌上做。
//!
//! **为什么单独一层**：以前每个功能各带一套筛选——KWIC 有 `person_ids`，统计页有
//! `StatsFilter.performer_ids`，词云又是别的。要「拿 2010 年代的某几位歌手对比其余」
//! 就得在三个地方各加一遍年份。这里把「哪些歌」定义一次，所有分析都吃同一个结构，
//! 两个客户端（Tauri、macOS）也就共用同一套语义。
//!
//! 条件之间是 **AND**，同一条件里的多个值是 **OR**（选了两位歌手 = 两人中任何一人唱的）。
//! 全空就是整个语料库。
//!
//! 只用实际有的元数据：演唱署名（`track_credits`）、年份（`songs.year` 前四位）、
//! 专辑（`songs.album_id`）、流派（`songs.genre`，多数歌是空的，所以只在有值时有用）、
//! 以及直接点名的歌。

use std::collections::HashSet;

use anyhow::Result;
use rusqlite::{Connection, params_from_iter, types::Value};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Subcorpus {
    /// 演唱者（`people.id`），任何一位唱的都算
    pub performer_ids: Vec<i64>,
    /// 年份下限（含）。按 `songs.year` 的前四位比，没有年份的歌在设了年份条件时被排除
    pub year_from: Option<i32>,
    /// 年份上限（含）
    pub year_to: Option<i32>,
    pub album_ids: Vec<i64>,
    pub genres: Vec<String>,
    /// 直接点名的歌
    pub song_ids: Vec<String>,
    /// 歌词语言（`songs.language`：ja / en / ko / zh / mixed，见 `quality::guess_language`）
    pub languages: Vec<String>,
    /// 当前歌词版本的完整度下限（0~1，见 `lineage`）。没有版本记录的歌在设了这个条件时被排除
    pub min_completeness: Option<f64>,
}

impl Subcorpus {
    /// 没有任何条件 = 整个语料库
    pub fn is_all(&self) -> bool {
        self.performer_ids.is_empty()
            && self.year_from.is_none()
            && self.year_to.is_none()
            && self.album_ids.is_empty()
            && self.genres.is_empty()
            && self.song_ids.is_empty()
            && self.languages.is_empty()
            && self.min_completeness.is_none()
    }

    /// 只看某几位演唱者。兼容老接口（`KwicQuery.person_ids`、`StatsFilter.performer_ids`）
    pub fn performers(ids: &[i64]) -> Self {
        Self {
            performer_ids: ids.to_vec(),
            ..Self::default()
        }
    }

    /// `songs` 表上的 WHERE 条件（不含 WHERE 本身），别名是 `alias`。全空时返回 `1`。
    pub(crate) fn songs_where(&self, alias: &str) -> (String, Vec<Value>) {
        let mut parts: Vec<String> = Vec::new();
        let mut params: Vec<Value> = Vec::new();
        if !self.performer_ids.is_empty() {
            parts.push(format!(
                "EXISTS (SELECT 1 FROM track_credits c WHERE c.song_id = {alias}.id \
                 AND c.role = 'performer' AND c.person_id IN ({}))",
                marks(self.performer_ids.len())
            ));
            params.extend(self.performer_ids.iter().map(|id| Value::from(*id)));
        }
        // 年份存成 TEXT，有「2006」也可能有「2006-05-01」，取前四位当整数比。
        // 刮削补了完整发行日期（release_date）的优先用它——它只在年份对得上或年份为空时才写，
        // 所以只会把「year 为空但知道发行日期」的歌多认出来
        let year = format!(
            "CAST(substr(COALESCE(NULLIF({alias}.release_date,''), NULLIF({alias}.year,''), ''),1,4) AS INTEGER)"
        );
        if let Some(from) = self.year_from {
            parts.push(format!("{year} >= ?"));
            params.push(Value::from(i64::from(from)));
        }
        if let Some(to) = self.year_to {
            parts.push(format!("({year} <= ? AND {year} > 0)"));
            params.push(Value::from(i64::from(to)));
        }
        if !self.album_ids.is_empty() {
            parts.push(format!("{alias}.album_id IN ({})", marks(self.album_ids.len())));
            params.extend(self.album_ids.iter().map(|id| Value::from(*id)));
        }
        if !self.genres.is_empty() {
            // 不分大小写：标签里「J-Pop」「J-POP」都有
            parts.push(format!("LOWER(TRIM(COALESCE({alias}.genre,''))) IN ({})", marks(self.genres.len())));
            params.extend(self.genres.iter().map(|g| Value::from(g.trim().to_lowercase())));
        }
        if !self.languages.is_empty() {
            parts.push(format!("COALESCE({alias}.language,'') IN ({})", marks(self.languages.len())));
            params.extend(self.languages.iter().map(|l| Value::from(l.clone())));
        }
        if let Some(min) = self.min_completeness {
            parts.push(format!(
                "EXISTS (SELECT 1 FROM lyric_versions v WHERE v.song_id = {alias}.id AND v.is_current = 1 \
                 AND v.completeness >= ?)"
            ));
            params.push(Value::from(min));
        }
        if !self.song_ids.is_empty() {
            parts.push(format!("{alias}.id IN ({})", marks(self.song_ids.len())));
            params.extend(self.song_ids.iter().map(|id| Value::from(id.clone())));
        }
        if parts.is_empty() {
            ("1".into(), params)
        } else {
            (parts.join(" AND "), params)
        }
    }

    /// 接在一条已经有 `song_col`（比如 `u.song_id`）的查询后面的 ` AND ...` 子句。全空时是空串。
    pub(crate) fn and_clause(&self, song_col: &str) -> (String, Vec<Value>) {
        if self.is_all() {
            return (String::new(), Vec::new());
        }
        let (cond, params) = self.songs_where("sc");
        (
            format!(" AND {song_col} IN (SELECT sc.id FROM songs sc WHERE {cond})"),
            params,
        )
    }

    /// 这个子语料里有哪些歌
    pub fn song_ids(&self, conn: &Connection) -> Result<HashSet<String>> {
        let (cond, params) = self.songs_where("s");
        let mut stmt = conn.prepare(&format!("SELECT s.id FROM songs s WHERE {cond}"))?;
        let rows = stmt.query_map(params_from_iter(params), |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<HashSet<_>>>()?)
    }

    /// 一句人话，结果的出处里写这个。演唱者名字要查库，所以要连接。
    pub fn describe(&self, conn: &Connection) -> Result<String> {
        if self.is_all() {
            return Ok("全部歌曲".into());
        }
        let mut parts = Vec::new();
        if !self.performer_ids.is_empty() {
            let mut stmt = conn.prepare(&format!(
                "SELECT name FROM people WHERE id IN ({}) ORDER BY name",
                marks(self.performer_ids.len())
            ))?;
            let names = stmt
                .query_map(params_from_iter(self.performer_ids.iter()), |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            parts.push(format!("演唱：{}", names.join(" / ")));
        }
        match (self.year_from, self.year_to) {
            (Some(a), Some(b)) => parts.push(format!("{a}–{b} 年")),
            (Some(a), None) => parts.push(format!("{a} 年起")),
            (None, Some(b)) => parts.push(format!("{b} 年以前")),
            (None, None) => {}
        }
        if !self.album_ids.is_empty() {
            parts.push(format!("{} 张专辑", self.album_ids.len()));
        }
        if !self.genres.is_empty() {
            parts.push(format!("流派：{}", self.genres.join(" / ")));
        }
        if !self.song_ids.is_empty() {
            parts.push(format!("{} 首指定歌曲", self.song_ids.len()));
        }
        if !self.languages.is_empty() {
            parts.push(format!("语言：{}", self.languages.join(" / ")));
        }
        if let Some(min) = self.min_completeness {
            parts.push(format!("歌词完整度 ≥ {:.0}%", min * 100.0));
        }
        Ok(parts.join("，"))
    }
}

fn marks(n: usize) -> String {
    std::iter::repeat_n("?", n).collect::<Vec<_>>().join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_subcorpus_is_the_whole_corpus() {
        let corpus = crate::fixture::corpus().unwrap();
        let all = Subcorpus::default().song_ids(corpus.connection()).unwrap();
        assert_eq!(all.len() as i64, crate::fixture::TRACKS);
        assert_eq!(Subcorpus::default().and_clause("u.song_id").0, "");
    }

    #[test]
    fn conditions_are_anded_and_values_are_ored() {
        let corpus = crate::fixture::corpus().unwrap();
        let conn = corpus.connection();
        let years: Vec<i32> = {
            let mut stmt = conn.prepare("SELECT DISTINCT CAST(substr(year,1,4) AS INTEGER) FROM songs WHERE year <> '' ORDER BY 1").unwrap();
            stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
        };
        assert!(!years.is_empty(), "夹具里要有年份");
        let first = years[0];
        let only_first = Subcorpus { year_from: Some(first), year_to: Some(first), ..Default::default() };
        let ids = only_first.song_ids(conn).unwrap();
        let expected: i64 = conn
            .query_row("SELECT COUNT(*) FROM songs WHERE substr(year,1,4) = ?", [first.to_string()], |r| r.get(0))
            .unwrap();
        assert_eq!(ids.len() as i64, expected);

        // 再加一个不可能满足的条件 → 空
        let none = Subcorpus { song_ids: vec!["no-such-song".into()], ..only_first.clone() };
        assert!(none.song_ids(conn).unwrap().is_empty());
        assert!(only_first.describe(conn).unwrap().contains(&first.to_string()));
    }
}
