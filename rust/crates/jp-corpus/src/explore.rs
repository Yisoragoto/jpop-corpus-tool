//! 从检索结果往下钻的两种分析：按年份 / 演唱者分解频率、搭配网络。
//!
//! ## 频率分解
//!
//! 一个检索（关键词或 CQL）在每一年、每位演唱者的歌里各出现几次，按那一组的词次折成**每万词**——
//! Sketch Engine 的「按文本类型的频率」。不折算的话，歌多的年份、歌多的歌手永远排在前面。
//!
//! - 命中按**全部词次**数（不去重）：一句副歌唱三遍就是三次，分母也是全部词次，两边口径一致；
//! - 分母是这一组的歌在检索范围里的词次，**不含标点和符号**（和词频、搭配一样）；
//! - 合唱的歌算进每一位演唱者；没有年份的歌归在 key 为空的一组；
//! - `ratio` 是这一组的每万词 ÷ 整个范围的每万词：1 是持平，2 是两倍。
//!
//! ## 搭配网络
//!
//! 以节点词为中心：先按搭配表取前 N 个搭配词（和 KWIC 右栏同一个算法、同一组参数），
//! 再在整个范围里数这些搭配词**彼此之间**在同一个窗口里共现几次，按 logDice 给每个词留最强的几条边。
//! 只画节点词周围这一小块（ego network），不画全库的共现网——几千个词的网在歌词这种规模上只是一团毛线。

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use rusqlite::{Connection, params_from_iter, types::Value};
use serde::{Deserialize, Serialize};

use crate::analysis::{self, CollocationQuery, Provenance};
use crate::cql::CqlQuery;
use crate::models::{KwicHit, KwicQuery};

// ────────────────────────────── 频率分解 ──────────────────────────────

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupBy {
    #[default]
    Year,
    Performer,
}

/// 关键词检索和 CQL 二选一（都给时用 CQL）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BreakdownQuery {
    pub by: GroupBy,
    pub kwic: Option<KwicQuery>,
    pub cql: Option<CqlQuery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakdownRow {
    /// 年份（四位）或 `people.id`；没有年份的歌是空串
    pub key: String,
    /// 年份或演唱者名；没有年份的那组是空串（界面写「年份不明」）
    pub label: String,
    pub hits: i64,
    /// 有命中的歌数
    pub hit_songs: i64,
    /// 这一组的词次（分母）
    pub tokens: i64,
    pub songs: i64,
    pub per_10k: f64,
    /// 这一组的每万词 ÷ 整个范围的每万词
    pub ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakdownResult {
    pub by: GroupBy,
    pub rows: Vec<BreakdownRow>,
    pub hits: i64,
    pub tokens: i64,
    pub songs: i64,
    pub per_10k: f64,
}

fn per_10k(hits: i64, tokens: i64) -> f64 {
    if tokens <= 0 { 0.0 } else { hits as f64 * 10_000.0 / tokens as f64 }
}

/// 检索范围里的歌（KWIC 的 `person_ids` 和子语料要同时满足）
fn scope_songs(conn: &Connection, subcorpus: &crate::subcorpus::Subcorpus, person_ids: &[i64]) -> Result<HashSet<String>> {
    let mut songs = subcorpus.song_ids(conn)?;
    if !person_ids.is_empty() {
        let marks = vec!["?"; person_ids.len()].join(",");
        let mut stmt = conn.prepare(&format!("SELECT DISTINCT song_id FROM track_credits WHERE person_id IN ({marks})"))?;
        let with: HashSet<String> = stmt
            .query_map(params_from_iter(person_ids.iter().map(|id| Value::from(*id))), |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        songs.retain(|s| with.contains(s));
    }
    Ok(songs)
}

pub fn breakdown(conn: &Connection, query: &BreakdownQuery) -> Result<BreakdownResult> {
    // 命中：不去重、不截断
    let (hits, songs): (Vec<KwicHit>, HashSet<String>) = match (&query.cql, &query.kwic) {
        (Some(cql), _) => {
            let q = CqlQuery { dedup: false, limit: None, cross_line: false, ..cql.clone() };
            (crate::cql::search(conn, &q)?, scope_songs(conn, &cql.subcorpus, &[])?)
        }
        (None, Some(kwic)) => {
            let q = KwicQuery { dedup: false, limit: None, cross_line: false, ..kwic.clone() };
            (crate::search::kwic(conn, &q)?, scope_songs(conn, &kwic.subcorpus, &kwic.person_ids)?)
        }
        (None, None) => anyhow::bail!("没有给检索条件"),
    };

    // 每首歌的词次（不含标点、符号）
    let mut song_tokens: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT u.song_id, COUNT(*) FROM tokens t JOIN utterances u ON u.id = t.utterance_id \
             WHERE COALESCE(t.pos,'') NOT IN ('PUNCT','SYM') GROUP BY u.song_id",
        )?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (song, n) = row?;
            if songs.contains(&song) {
                song_tokens.insert(song, n);
            }
        }
    }

    // 歌 → 组
    let mut groups_of: HashMap<String, Vec<String>> = HashMap::new();
    let mut labels: HashMap<String, String> = HashMap::new();
    match query.by {
        GroupBy::Year => {
            let mut stmt = conn.prepare("SELECT id, COALESCE(substr(year,1,4),'') FROM songs")?;
            for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (song, year) = row?;
                let year = if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) { year } else { String::new() };
                labels.insert(year.clone(), year.clone());
                groups_of.insert(song, vec![year]);
            }
        }
        GroupBy::Performer => {
            let mut stmt = conn.prepare(
                "SELECT c.song_id, p.id, p.name FROM track_credits c JOIN people p ON p.id = c.person_id \
                 WHERE c.role = 'performer' ORDER BY c.song_id, c.position, p.id",
            )?;
            for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))? {
                let (song, id, name) = row?;
                let list = groups_of.entry(song).or_default();
                if !list.contains(&id.to_string()) {
                    list.push(id.to_string());
                }
                labels.insert(id.to_string(), name);
            }
        }
    }

    #[derive(Default)]
    struct Acc {
        hits: i64,
        hit_songs: HashSet<String>,
        tokens: i64,
        songs: i64,
    }
    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();
    for (song, n) in &song_tokens {
        for g in groups_of.get(song).map(Vec::as_slice).unwrap_or_default() {
            let a = acc.entry(g.clone()).or_default();
            a.tokens += n;
            a.songs += 1;
        }
    }
    for hit in &hits {
        for g in groups_of.get(&hit.song_id).map(Vec::as_slice).unwrap_or_default() {
            let a = acc.entry(g.clone()).or_default();
            a.hits += 1;
            a.hit_songs.insert(hit.song_id.clone());
        }
    }

    let total_tokens: i64 = song_tokens.values().sum();
    let total_hits = hits.len() as i64;
    let overall = per_10k(total_hits, total_tokens);
    let mut rows: Vec<BreakdownRow> = acc
        .into_iter()
        .filter(|(_, a)| a.tokens > 0)
        .map(|(key, a)| {
            let p = per_10k(a.hits, a.tokens);
            BreakdownRow {
                label: labels.get(&key).cloned().unwrap_or_default(),
                key,
                hits: a.hits,
                hit_songs: a.hit_songs.len() as i64,
                tokens: a.tokens,
                songs: a.songs,
                per_10k: p,
                ratio: if overall > 0.0 { p / overall } else { 0.0 },
            }
        })
        .collect();
    match query.by {
        // 年份升序，没有年份的放最后
        GroupBy::Year => rows.sort_by(|a, b| (a.key.is_empty(), &a.key).cmp(&(b.key.is_empty(), &b.key))),
        GroupBy::Performer => rows.sort_by(|a, b| b.hits.cmp(&a.hits).then_with(|| b.tokens.cmp(&a.tokens)).then_with(|| a.key.cmp(&b.key))),
    }
    Ok(BreakdownResult {
        by: query.by,
        rows,
        hits: total_hits,
        tokens: total_tokens,
        songs: song_tokens.len() as i64,
        per_10k: overall,
    })
}

// ────────────────────────────── 搭配网络 ──────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetworkQuery {
    /// 节点词和搭配参数（窗口、计数单位、指标、只看实词……），和搭配表同一组
    pub collocation: CollocationQuery,
    /// 画几个搭配词
    pub size: usize,
    /// 搭配词之间至少共现几次才连线
    pub min_pair: i64,
    /// 每个搭配词最多连几条（按 logDice 取最强的）
    pub per_node: usize,
}

impl Default for NetworkQuery {
    fn default() -> Self {
        Self { collocation: CollocationQuery::default(), size: 20, min_pair: 2, per_node: 3 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkNode {
    pub lemma: String,
    pub pos: String,
    /// 在范围里的总频次
    pub freq: i64,
    /// 和节点词的共现次数
    pub cofreq: i64,
    pub log_dice: f64,
    /// 按所选指标的值（决定离中心多近）
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEdge {
    /// 两端在 `nodes` 里的下标，a < b
    pub a: usize,
    pub b: usize,
    pub freq: i64,
    pub log_dice: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkResult {
    pub node: String,
    pub node_freq: i64,
    pub nodes: Vec<NetworkNode>,
    pub edges: Vec<NetworkEdge>,
    pub provenance: Provenance,
}

pub fn network(conn: &Connection, query: &NetworkQuery) -> Result<NetworkResult> {
    let base = CollocationQuery { limit: Some(query.size.max(1)), ..query.collocation.clone() };
    let colloc = analysis::collocations(conn, &base)?;
    let nodes: Vec<NetworkNode> = colloc
        .collocates
        .iter()
        .map(|c| NetworkNode {
            lemma: c.lemma.clone(),
            pos: c.pos.clone(),
            freq: c.collocate_freq,
            cofreq: c.freq,
            log_dice: c.log_dice,
            score: match base.measure {
                analysis::CollocationMeasure::LogDice => c.log_dice,
                analysis::CollocationMeasure::Mi => c.mi,
                analysis::CollocationMeasure::Mi3 => c.mi3,
                analysis::CollocationMeasure::LogLikelihood => c.log_likelihood,
                analysis::CollocationMeasure::DeltaP => c.delta_p,
                analysis::CollocationMeasure::Frequency => c.freq as f64,
            },
        })
        .collect();
    let index: HashMap<(String, String), usize> =
        nodes.iter().enumerate().map(|(i, n)| ((n.lemma.clone(), n.pos.clone()), i)).collect();

    // 搭配词两两在同一窗口里共现几次。窗口和搭配表一样宽（取左右较大的那边），不跨行，跳过标点
    let window = base.left.max(base.right).max(1);
    let lines = analysis::load_lines(conn, &base.scope)?;
    let mut pairs: HashMap<(usize, usize), i64> = HashMap::new();
    for line in lines.iter().filter(|l| analysis::counted(l, base.scope.unit)) {
        let ids: Vec<Option<usize>> =
            line.tokens.iter().map(|t| index.get(&(t.lemma.clone(), t.pos.clone())).copied()).collect();
        for (i, a) in ids.iter().enumerate() {
            let Some(a) = a else { continue };
            for b in ids.iter().skip(i + 1).take(window).flatten() {
                if a != b {
                    *pairs.entry(((*a).min(*b), (*a).max(*b))).or_default() += 1;
                }
            }
        }
    }
    let mut edges: Vec<NetworkEdge> = pairs
        .into_iter()
        .filter(|(_, n)| *n >= query.min_pair.max(1))
        .map(|((a, b), n)| NetworkEdge {
            a,
            b,
            freq: n,
            log_dice: 14.0 + (2.0 * n as f64 / (nodes[a].freq + nodes[b].freq).max(1) as f64).log2(),
        })
        .collect();
    edges.sort_by(|x, y| y.log_dice.partial_cmp(&x.log_dice).unwrap_or(std::cmp::Ordering::Equal).then_with(|| (x.a, x.b).cmp(&(y.a, y.b))));
    // 每个点只留最强的 per_node 条：一条边只要是任一端的前几名就留
    let mut kept_per: HashMap<usize, usize> = HashMap::new();
    let mut keep = vec![false; edges.len()];
    for (k, e) in edges.iter().enumerate() {
        for end in [e.a, e.b] {
            let n = kept_per.entry(end).or_default();
            if *n < query.per_node {
                *n += 1;
                keep[k] = true;
            }
        }
    }
    let edges: Vec<NetworkEdge> = edges.into_iter().zip(keep).filter_map(|(e, k)| k.then_some(e)).collect();
    let provenance = analysis::provenance(conn, &base.scope, &lines, serde_json::to_value(query)?)?;
    Ok(NetworkResult { node: base.node.clone(), node_freq: colloc.node_freq, nodes, edges, provenance })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MatchField;

    #[test]
    fn breakdown_groups_add_up_to_the_whole() {
        let corpus = crate::fixture::corpus().unwrap();
        let conn = corpus.connection();
        let kwic = KwicQuery { keywords: vec!["夜".into()], field: MatchField::Lemma, ..Default::default() };
        let by_year = breakdown(conn, &BreakdownQuery { by: GroupBy::Year, kwic: Some(kwic.clone()), cql: None }).unwrap();
        // 每首歌只有一个年份组，所以分组之和就是总数
        assert_eq!(by_year.rows.iter().map(|r| r.hits).sum::<i64>(), by_year.hits);
        assert_eq!(by_year.rows.iter().map(|r| r.tokens).sum::<i64>(), by_year.tokens);
        assert_eq!(by_year.hits, 4, "「夜」不去重是 4 次");
        for r in &by_year.rows {
            assert!((r.per_10k - r.hits as f64 * 1e4 / r.tokens as f64).abs() < 1e-9);
        }
        // CQL 和关键词检索给同样的数
        let cql = CqlQuery { query: r#"[lemma="夜"]"#.into(), ..Default::default() };
        let same = breakdown(conn, &BreakdownQuery { by: GroupBy::Year, kwic: None, cql: Some(cql) }).unwrap();
        assert_eq!(same.hits, by_year.hits);
        let people = breakdown(conn, &BreakdownQuery { by: GroupBy::Performer, kwic: Some(kwic), cql: None }).unwrap();
        assert!(people.rows.iter().all(|r| !r.label.is_empty()));
    }

    #[test]
    fn network_nodes_are_the_collocates_and_edges_point_inside() {
        let corpus = crate::fixture::corpus().unwrap();
        let conn = corpus.connection();
        let top = analysis::frequency(conn, &analysis::FrequencyQuery::default()).unwrap();
        let node = top.items.iter().find(|i| i.freq >= 2).unwrap().lemma.clone();
        let q = NetworkQuery {
            collocation: CollocationQuery { node, min_freq: 1, ..Default::default() },
            size: 10,
            min_pair: 1,
            per_node: 2,
        };
        let net = network(conn, &q).unwrap();
        assert!(!net.nodes.is_empty() && net.nodes.len() <= 10);
        assert!(net.edges.iter().all(|e| e.a < e.b && e.b < net.nodes.len() && e.freq >= 1));
    }
}
