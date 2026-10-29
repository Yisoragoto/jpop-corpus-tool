//! 词汇丰富度与文字种：按整个子语料、每位演唱者或每首歌各算一行，放在一起比。
//!
//! ## 为什么不只用 TTR
//!
//! TTR（词种 ÷ 词次）随文本变长单调下降：一首 150 词的歌和一位歌手 8,000 词的全部作品，
//! TTR 没法放在一起比。这里补三个不受长度（或受影响小得多）的指标，都按**词元**算，
//! 和语料报告的「Type 数」一致：
//!
//! - **MATTR**（Covington & McFall 2010）：长度为 W 的窗口每次右移一个词，取所有窗口 TTR 的平均。
//!   默认 W = 50（TAALED 的默认值；一首歌常常只有两三百词，500 的窗口放不下）。词次不到 W 时没有值；
//! - **MTLD**（McCarthy & Jarvis 2010）：从头往后累计 TTR，降到 0.72 记一个「因子」并清零重来，
//!   末尾不满的一段按 (1 − TTR) / (1 − 0.72) 记部分因子；词次 ÷ 因子数。正着、倒着各算一遍取平均。
//!   因子数为 0（全是不同的词）时没有值；
//! - **Yule's K**：10⁴ × (Σ m²·Vₘ − N) / N²，Vₘ 是正好出现 m 次的词种数。越大越重复。
//!
//! 词流的顺序就是读入顺序（歌 id、行号、词序），和 STTR 一样；计数单位选「重复行只算一次」时，
//! 副歌只按第一次算——歌词的「丰富度」一般想问的是写词人写了什么，而不是唱了几遍。
//!
//! ## 文字种
//!
//! 日语歌词的写法本身就是风格：同一个「君」写成汉字、平假名还是片假名，英文混多少。
//! 按词次里的**字符**分五类数比例：汉字（含「々」「〆」）、平假名、片假名（含长音「ー」、半角片假名）、
//! 拉丁字母（含全角）、其他（数字、表情符号等）。分母不含标点和符号，和词次一致。

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::analysis::{self, CountUnit, Line, Provenance, Scope};

/// 按什么分组
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LexicalGroup {
    /// 整个子语料一行
    #[default]
    Whole,
    /// 每位演唱者一行。合唱的歌算进每一位演唱者
    Performer,
    /// 每首歌一行
    Song,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LexicalQuery {
    pub scope: Scope,
    pub group: LexicalGroup,
    /// MATTR 的窗口
    pub window: usize,
    /// 词次少于这个数的组不列出（太短的文本这些指标都不稳）
    pub min_tokens: usize,
    pub limit: Option<usize>,
}

impl Default for LexicalQuery {
    fn default() -> Self {
        Self {
            scope: Scope { unit: CountUnit::DistinctLines, ..Scope::default() },
            group: LexicalGroup::Whole,
            window: 50,
            min_tokens: 100,
            limit: Some(200),
        }
    }
}

/// 五类文字各占多少字符（比例，合计 1）
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptShares {
    pub kanji: f64,
    pub hiragana: f64,
    pub katakana: f64,
    pub latin: f64,
    pub other: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LexicalRow {
    /// 演唱者是 `people.id`，歌是歌 id，整体是空串
    pub key: String,
    /// 演唱者名、「歌手 · 曲名」，整体是空串（界面写「整个范围」）
    pub label: String,
    pub songs: i64,
    pub tokens: i64,
    pub types: i64,
    pub ttr: f64,
    pub mattr: Option<f64>,
    pub mtld: Option<f64>,
    pub yule_k: f64,
    /// 只出现一次的词种占词种的比例
    pub hapax_ratio: f64,
    pub chars: i64,
    pub scripts: ScriptShares,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LexicalResult {
    pub rows: Vec<LexicalRow>,
    pub provenance: Provenance,
}

pub fn lexical_profile(conn: &Connection, query: &LexicalQuery) -> Result<LexicalResult> {
    let lines = analysis::load_lines(conn, &query.scope)?;
    let unit = query.scope.unit;
    let window = query.window.max(1);

    // 每组的词流（词元）和文字计数。BTreeMap：结果顺序稳定
    #[derive(Default)]
    struct Group {
        lemmas: Vec<String>,
        songs: HashSet<String>,
        counts: [i64; 5],
    }
    let performers = match query.group {
        LexicalGroup::Performer => performers_of(conn, &lines)?,
        _ => HashMap::new(),
    };
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for line in lines.iter().filter(|l| analysis::counted(l, unit)) {
        let keys: Vec<String> = match query.group {
            LexicalGroup::Whole => vec![String::new()],
            LexicalGroup::Song => vec![line.song_id.clone()],
            LexicalGroup::Performer => performers
                .get(&line.song_id)
                .map(|ids| ids.iter().map(|id| id.to_string()).collect())
                .unwrap_or_default(),
        };
        for key in keys {
            let g = groups.entry(key).or_default();
            g.songs.insert(line.song_id.clone());
            for tok in &line.tokens {
                g.lemmas.push(tok.lemma.clone());
                for c in tok.surface.chars() {
                    g.counts[script_of(c) as usize] += 1;
                }
            }
        }
    }

    let labels = labels(conn, query.group, groups.keys())?;
    let mut rows: Vec<LexicalRow> = groups
        .into_iter()
        .filter(|(_, g)| g.lemmas.len() >= query.min_tokens.max(1) || query.group == LexicalGroup::Whole)
        .map(|(key, g)| {
            let r = richness(&g.lemmas, window);
            let chars: i64 = g.counts.iter().sum();
            let share = |i: usize| if chars > 0 { g.counts[i] as f64 / chars as f64 } else { 0.0 };
            LexicalRow {
                label: labels.get(&key).cloned().unwrap_or_default(),
                key,
                songs: g.songs.len() as i64,
                tokens: g.lemmas.len() as i64,
                types: r.types,
                ttr: r.ttr,
                mattr: r.mattr,
                mtld: r.mtld,
                yule_k: r.yule_k,
                hapax_ratio: r.hapax_ratio,
                chars,
                scripts: ScriptShares {
                    kanji: share(Script::Kanji as usize),
                    hiragana: share(Script::Hiragana as usize),
                    katakana: share(Script::Katakana as usize),
                    latin: share(Script::Latin as usize),
                    other: share(Script::Other as usize),
                },
            }
        })
        .collect();
    // 词次多的在前（演唱者按作品量），同样多按 key，结果可复现
    rows.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.key.cmp(&b.key)));
    rows.truncate(query.limit.unwrap_or(200));
    let provenance = analysis::provenance(conn, &query.scope, &lines, serde_json::to_value(query)?)?;
    Ok(LexicalResult { rows, provenance })
}

/// 范围里每首歌的演唱者（`people.id`）
fn performers_of(conn: &Connection, lines: &[Line]) -> Result<HashMap<String, Vec<i64>>> {
    let songs: HashSet<&str> = lines.iter().map(|l| l.song_id.as_str()).collect();
    let mut stmt = conn.prepare("SELECT song_id, person_id FROM track_credits WHERE role = 'performer' ORDER BY song_id, person_id")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut out: HashMap<String, Vec<i64>> = HashMap::new();
    for row in rows {
        let (song, person) = row?;
        if songs.contains(song.as_str()) {
            let list = out.entry(song).or_default();
            if !list.contains(&person) {
                list.push(person);
            }
        }
    }
    Ok(out)
}

fn labels<'a>(conn: &Connection, group: LexicalGroup, keys: impl Iterator<Item = &'a String>) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    let sql = match group {
        LexicalGroup::Whole => return Ok(out),
        LexicalGroup::Performer => "SELECT name FROM people WHERE id = ?",
        LexicalGroup::Song => "SELECT artist || ' · ' || title FROM songs WHERE id = ?",
    };
    let mut stmt = conn.prepare(sql)?;
    for key in keys {
        let label: Option<String> = stmt.query_row([key], |r| r.get(0)).ok();
        out.insert(key.clone(), label.unwrap_or_else(|| key.clone()));
    }
    Ok(out)
}

// ────────────────────────────── 指标 ──────────────────────────────

/// 一段词流的丰富度指标
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Richness {
    pub types: i64,
    pub ttr: f64,
    pub mattr: Option<f64>,
    pub mtld: Option<f64>,
    pub yule_k: f64,
    pub hapax_ratio: f64,
}

pub fn richness<S: AsRef<str>>(tokens: &[S], window: usize) -> Richness {
    let n = tokens.len();
    let mut freq: HashMap<&str, i64> = HashMap::new();
    for t in tokens {
        *freq.entry(t.as_ref()).or_default() += 1;
    }
    let types = freq.len() as i64;
    let hapax = freq.values().filter(|&&f| f == 1).count() as f64;
    let sum_sq: f64 = freq.values().map(|&f| (f * f) as f64).sum();
    let nf = n as f64;
    Richness {
        types,
        ttr: if n > 0 { types as f64 / nf } else { 0.0 },
        mattr: mattr(tokens, window),
        mtld: mtld(tokens, MTLD_THRESHOLD),
        yule_k: if n > 0 { 10_000.0 * (sum_sq - nf) / (nf * nf) } else { 0.0 },
        hapax_ratio: if types > 0 { hapax / types as f64 } else { 0.0 },
    }
}

pub const MTLD_THRESHOLD: f64 = 0.72;

/// 移动平均 TTR。窗口每次右移一个词，增量维护窗口里的词种数
pub fn mattr<S: AsRef<str>>(tokens: &[S], window: usize) -> Option<f64> {
    let w = window.max(1);
    if tokens.len() < w {
        return None;
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for t in &tokens[..w] {
        *counts.entry(t.as_ref()).or_default() += 1;
    }
    let mut sum = counts.len() as f64;
    let windows = tokens.len() - w + 1;
    for i in 1..windows {
        let out = tokens[i - 1].as_ref();
        if let Some(c) = counts.get_mut(out) {
            *c -= 1;
            if *c == 0 {
                counts.remove(out);
            }
        }
        *counts.entry(tokens[i + w - 1].as_ref()).or_default() += 1;
        sum += counts.len() as f64;
    }
    Some(sum / (windows as f64 * w as f64))
}

/// MTLD：正反两遍的平均
pub fn mtld<S: AsRef<str>>(tokens: &[S], threshold: f64) -> Option<f64> {
    let forward = mtld_pass(tokens.iter().map(AsRef::as_ref), tokens.len(), threshold)?;
    let backward = mtld_pass(tokens.iter().rev().map(AsRef::as_ref), tokens.len(), threshold)?;
    Some((forward + backward) / 2.0)
}

fn mtld_pass<'a>(tokens: impl Iterator<Item = &'a str>, n: usize, threshold: f64) -> Option<f64> {
    let mut factors = 0.0;
    let mut types: HashSet<&str> = HashSet::new();
    let mut count = 0usize;
    for t in tokens {
        count += 1;
        types.insert(t);
        if types.len() as f64 / count as f64 <= threshold {
            factors += 1.0;
            types.clear();
            count = 0;
        }
    }
    if count > 0 {
        let ttr = types.len() as f64 / count as f64;
        factors += (1.0 - ttr) / (1.0 - threshold);
    }
    (factors > 0.0).then(|| n as f64 / factors)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Kanji = 0,
    Hiragana = 1,
    Katakana = 2,
    Latin = 3,
    Other = 4,
}

pub fn script_of(c: char) -> Script {
    match c {
        // 々 〆 〇 和 CJK 统一汉字（含扩展 A、兼容汉字）
        '\u{3005}' | '\u{3006}' | '\u{3007}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' => Script::Kanji,
        '\u{20000}'..='\u{2FFFF}' => Script::Kanji,
        '\u{3041}'..='\u{309F}' => Script::Hiragana,
        // 片假名、长音 ー、片假名扩展、半角片假名
        '\u{30A0}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}' => Script::Katakana,
        // 带附加符号的拉丁字母（é ü ø…），去掉夹在中间的 × ÷
        '\u{00D7}' | '\u{00F7}' => Script::Other,
        'A'..='Z' | 'a'..='z' | '\u{FF21}'..='\u{FF3A}' | '\u{FF41}'..='\u{FF5A}' | '\u{00C0}'..='\u{024F}' => Script::Latin,
        _ => Script::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mattr_equals_ttr_when_the_window_is_the_whole_text_and_is_none_when_too_short() {
        let toks = ["a", "b", "a", "c"];
        assert_eq!(mattr(&toks, 4), Some(3.0 / 4.0));
        assert_eq!(mattr(&toks, 5), None);
        // 窗口 2：ab ba ac → 全是 2 个词种
        assert_eq!(mattr(&toks, 2), Some(1.0));
        // 窗口 3：aba(2) bac(3) → 5/6
        assert!((mattr(&toks, 3).unwrap() - 5.0 / 6.0).abs() < 1e-12);
    }

    #[test]
    fn mtld_counts_full_and_partial_factors() {
        // 全是同一个词：第 2 个词 TTR = 0.5 ≤ 0.72 就是一个因子；10 个词正好 5 个因子
        let same = ["x"; 10];
        assert_eq!(mtld(&same, MTLD_THRESHOLD), Some(2.0));
        // 全不同：TTR 一直是 1，部分因子是 0，没有值
        let distinct: Vec<String> = (0..20).map(|i| i.to_string()).collect();
        assert_eq!(mtld(&distinct, MTLD_THRESHOLD), None);
    }

    #[test]
    fn yule_k_and_hapax() {
        // N = 4，频次 2,1,1：Σm²Vm = 4 + 1 + 1 = 6 → K = 1e4 × 2 / 16
        let r = richness(&["a", "a", "b", "c"], 50);
        assert!((r.yule_k - 1250.0).abs() < 1e-9);
        assert_eq!(r.types, 3);
        assert!((r.hapax_ratio - 2.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn scripts_cover_the_japanese_writing_system() {
        let kinds: Vec<Script> = "君きみキミｷﾐ々ーLove１!".chars().map(script_of).collect();
        use Script::*;
        assert_eq!(
            kinds,
            vec![Kanji, Hiragana, Hiragana, Katakana, Katakana, Katakana, Katakana, Kanji, Katakana, Latin, Latin, Latin, Latin, Other, Other]
        );
    }

    #[test]
    fn profile_by_song_and_performer_partitions_the_tokens() {
        let corpus = crate::fixture::corpus().unwrap();
        let conn = corpus.connection();
        let scope = Scope { unit: CountUnit::Tokens, ..Scope::default() };
        let whole = lexical_profile(conn, &LexicalQuery { scope: scope.clone(), min_tokens: 1, ..Default::default() }).unwrap();
        assert_eq!(whole.rows.len(), 1);
        assert_eq!(whole.rows[0].tokens, whole.provenance.tokens);
        let sum: f64 = [whole.rows[0].scripts.kanji, whole.rows[0].scripts.hiragana, whole.rows[0].scripts.katakana, whole.rows[0].scripts.latin, whole.rows[0].scripts.other]
            .iter()
            .sum();
        assert!((sum - 1.0).abs() < 1e-9);

        let songs = lexical_profile(conn, &LexicalQuery { scope: scope.clone(), group: LexicalGroup::Song, min_tokens: 1, ..Default::default() }).unwrap();
        assert_eq!(songs.rows.iter().map(|r| r.tokens).sum::<i64>(), whole.rows[0].tokens);
        assert!(songs.rows.iter().all(|r| r.songs == 1 && !r.label.is_empty()));

        let people = lexical_profile(conn, &LexicalQuery { scope, group: LexicalGroup::Performer, min_tokens: 1, ..Default::default() }).unwrap();
        assert!(!people.rows.is_empty());
        // 合唱的歌算进每一位，所以各演唱者词次之和不少于整体（没有署名的歌不进任何一组）
        assert!(people.rows.iter().all(|r| r.key.parse::<i64>().is_ok()));
    }
}
