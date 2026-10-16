//! 语料分析：重复感知的词频、搭配、子语料对比，以及结果出处。
//!
//! ## 重复段落
//!
//! 歌词和散文不同：副歌会原样唱好几遍。用户真实语料里，**同一首歌内重复出现的行占 23%**
//! （8,587 行里 1,960 行是同一首歌里前面已经出现过的原句）。直接数词次，副歌里的词会被放大好几倍，
//! 「这个词常用」和「这首歌把这句唱了六遍」混在一起分不开。
//!
//! 所以每个分析都可以选计数单位（[`CountUnit`]）：
//!
//! - `Tokens`：所有词次，唱几遍算几遍——和听到的一样，也和旧版统计页一致；
//! - `DistinctLines`：同一首歌里文本完全相同的行只算第一次——和写词人「写了什么」一致。
//!
//! 词频表两个数都给，外加「重复占比」，一眼看出哪些高频词是副歌堆出来的。
//! 「完全相同」按原文逐字比，和 KWIC 的「去重」是同一个规则。
//!
//! ## 计算方式
//!
//! 整个子语料的 token 流一次读进内存（全库 6 万词，几十毫秒），统计在 Rust 里做。
//! 不在 SQL 里拼窗口函数：搭配要按行内位置开窗，重复行要按首次出现跳过，
//! 写成 SQL 既慢又难核对。
//!
//! 词次的分母不含标点和符号（`PUNCT`、`SYM`），和统计页一致；搭配窗口也跳过它们，
//! 免得「、」占掉一个窗口位置。
//!
//! ## 出处
//!
//! 每个结果都带 [`Provenance`]：库的指纹（行数、词数、最大 id、校正数）、子语料的描述、
//! 计数单位、全部参数、规则版本。指纹和参数相同，结果就相同；分词校正或导入新歌之后指纹会变，
//! 界面据此提示「结果基于旧数据」。

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rusqlite::{Connection, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::subcorpus::Subcorpus;
use crate::MatchField;

/// 规则版本。改了计数或打分的规则就加一，旧结果的出处里能看出是按哪套规则算的
pub const RULES_VERSION: &str = "analysis-1";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CountUnit {
    /// 所有词次，重复的副歌唱几遍算几遍
    #[default]
    Tokens,
    /// 同一首歌里文本相同的行只算第一次
    DistinctLines,
}

/// 分析的范围：哪些歌、怎么数、要不要只看日文
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Scope {
    pub subcorpus: Subcorpus,
    pub unit: CountUnit,
    /// 只算含假名或汉字的词元（去掉英文、数字）
    pub jp_only: bool,
}

// ────────────────────────────── 出处 ──────────────────────────────

/// 库在某一刻的指纹。只要歌词、分词、校正有任何增删改，指纹就会变。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusSnapshot {
    pub schema_version: i64,
    pub songs: i64,
    pub lines: i64,
    pub tokens: i64,
    pub corrections: i64,
    /// 16 位十六进制。不是密码学哈希，只用来判断「是不是同一份数据」
    pub fingerprint: String,
}

pub fn snapshot(conn: &Connection) -> Result<CorpusSnapshot> {
    let schema_version = crate::migrations::version(conn)?;
    let (songs,): (i64,) = conn.query_row("SELECT COUNT(*) FROM songs", [], |r| Ok((r.get(0)?,)))?;
    let (lines, max_line): (i64, i64) =
        conn.query_row("SELECT COUNT(*), COALESCE(MAX(id),0) FROM utterances", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    // 校正会删掉旧 token 重新插入，所以最大 id 会变——这正是要的
    let (tokens, max_token): (i64, i64) =
        conn.query_row("SELECT COUNT(*), COALESCE(MAX(id),0) FROM tokens", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let has_corrections = conn
        .query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name='token_corrections'", [], |_| Ok(()))
        .is_ok();
    let (corrections, latest): (i64, String) = if has_corrections {
        conn.query_row(
            "SELECT COUNT(*), COALESCE(MAX(updated_at),'') FROM token_corrections",
            [],
            |r| Ok((r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())),
        )
        .or_else(|_| conn.query_row("SELECT COUNT(*), '' FROM token_corrections", [], |r| Ok((r.get(0)?, r.get(1)?))))?
    } else {
        (0, String::new())
    };
    // 歌的元数据（年份、演唱）会改变子语料的范围，也要进指纹
    let meta: String = conn.query_row(
        "SELECT COALESCE(group_concat(id || ':' || COALESCE(year,'') || ':' || COALESCE(album_id,''), ','),'') \
         FROM (SELECT id, year, album_id FROM songs ORDER BY id)",
        [],
        |r| r.get(0),
    )?;
    let credits: i64 = conn
        .query_row("SELECT COUNT(*) FROM track_credits", [], |r| r.get(0))
        .unwrap_or(0);
    let mut h = Fnv64::new();
    for part in [
        schema_version.to_string(),
        songs.to_string(),
        lines.to_string(),
        max_line.to_string(),
        tokens.to_string(),
        max_token.to_string(),
        corrections.to_string(),
        latest,
        credits.to_string(),
        meta,
    ] {
        h.write(part.as_bytes());
        h.write(b"|");
    }
    Ok(CorpusSnapshot {
        schema_version,
        songs,
        lines,
        tokens,
        corrections,
        fingerprint: format!("{:016x}", h.finish()),
    })
}

/// 一个结果是怎么算出来的
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub snapshot: CorpusSnapshot,
    pub rules: String,
    /// 子语料的一句人话描述
    pub scope: String,
    pub unit: CountUnit,
    /// 这个子语料里的歌数、词次（按所选单位）
    pub songs: i64,
    pub tokens: i64,
    /// 原样回显的查询参数。拿它重跑就能复现
    pub params: serde_json::Value,
}

struct Fnv64(u64);
impl Fnv64 {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

// ────────────────────────────── token 流 ──────────────────────────────

#[derive(Debug, Clone)]
struct Tok {
    surface: String,
    lemma: String,
    pos: String,
}

#[derive(Debug, Clone)]
struct Line {
    song_id: String,
    utterance_id: i64,
    /// 同一首歌里前面已经有一行文本完全相同
    repeat: bool,
    /// 已去掉标点、符号（以及 jp_only 时的非日文词）
    tokens: Vec<Tok>,
}

/// 读出一个子语料的全部行和词，按歌、行号排好
fn load_lines(conn: &Connection, scope: &Scope) -> Result<Vec<Line>> {
    let (clause, params) = scope.subcorpus.and_clause("u.song_id");
    let sql = format!(
        "SELECT u.song_id, u.id, u.text, t.surface, t.lemma, COALESCE(t.pos,'') \
         FROM utterances u JOIN tokens t ON t.utterance_id = u.id \
         WHERE 1{clause} \
         ORDER BY u.song_id, u.line_idx, u.id, t.token_idx"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut lines: Vec<Line> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    while let Some(row) = rows.next()? {
        let utterance_id: i64 = row.get(1)?;
        if lines.last().map(|l| l.utterance_id) != Some(utterance_id) {
            let song_id: String = row.get(0)?;
            let text: String = row.get(2)?;
            let repeat = !seen.insert((song_id.clone(), text));
            lines.push(Line { song_id, utterance_id, repeat, tokens: Vec::new() });
        }
        let pos: String = row.get(5)?;
        if pos == "PUNCT" || pos == "SYM" {
            continue;
        }
        let lemma: String = row.get(4)?;
        if scope.jp_only && !crate::stats::is_japanese(&lemma) {
            continue;
        }
        let line = lines.last_mut().expect("刚 push 过");
        line.tokens.push(Tok { surface: row.get(3)?, lemma, pos });
    }
    Ok(lines)
}

fn counted(line: &Line, unit: CountUnit) -> bool {
    unit == CountUnit::Tokens || !line.repeat
}

fn provenance(conn: &Connection, scope: &Scope, lines: &[Line], params: serde_json::Value) -> Result<Provenance> {
    let songs: HashSet<&str> = lines.iter().map(|l| l.song_id.as_str()).collect();
    let tokens = lines.iter().filter(|l| counted(l, scope.unit)).map(|l| l.tokens.len() as i64).sum();
    Ok(Provenance {
        snapshot: snapshot(conn)?,
        rules: RULES_VERSION.into(),
        scope: scope.subcorpus.describe(conn)?,
        unit: scope.unit,
        songs: songs.len() as i64,
        tokens,
        params,
    })
}

fn per_10k(freq: i64, total: i64) -> f64 {
    if total <= 0 { 0.0 } else { freq as f64 * 10_000.0 / total as f64 }
}

// ────────────────────────────── 词频 ──────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FrequencyQuery {
    pub scope: Scope,
    /// UPOS，空是全部
    pub pos: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrequencyItem {
    pub lemma: String,
    pub pos: String,
    /// 按所选单位的频次（排序依据）
    pub freq: i64,
    /// 每万词（按所选单位的分母）
    pub per_10k: f64,
    /// 所有词次
    pub raw_freq: i64,
    /// 重复行只算一次的词次
    pub distinct_freq: i64,
    /// 来自重复行的比例：1 − distinct / raw。越高越是副歌堆出来的
    pub repeat_share: f64,
    /// 出现在几首歌里（分布广度）
    pub songs: i64,
    /// 占子语料歌数的比例
    pub range: f64,
    /// 最常见的几个写法，按频次降序
    pub surfaces: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrequencyResult {
    pub items: Vec<FrequencyItem>,
    pub provenance: Provenance,
}

pub fn frequency(conn: &Connection, query: &FrequencyQuery) -> Result<FrequencyResult> {
    let lines = load_lines(conn, &query.scope)?;
    let pos = query.pos.as_deref().filter(|p| !p.is_empty());

    #[derive(Default)]
    struct Acc {
        raw: i64,
        distinct: i64,
        songs: HashSet<String>,
        surfaces: HashMap<String, i64>,
    }
    let mut acc: HashMap<(String, String), Acc> = HashMap::new();
    for line in &lines {
        for tok in &line.tokens {
            if pos.is_some_and(|p| p != tok.pos) {
                continue;
            }
            let a = acc.entry((tok.lemma.clone(), tok.pos.clone())).or_default();
            a.raw += 1;
            if !line.repeat {
                a.distinct += 1;
            }
            if counted(line, query.scope.unit) {
                a.songs.insert(line.song_id.clone());
            }
            *a.surfaces.entry(tok.surface.clone()).or_default() += 1;
        }
    }
    let total: i64 = lines.iter().filter(|l| counted(l, query.scope.unit)).map(|l| l.tokens.len() as i64).sum();
    let song_total = lines.iter().map(|l| l.song_id.as_str()).collect::<HashSet<_>>().len().max(1) as f64;

    let mut items: Vec<FrequencyItem> = acc
        .into_iter()
        .map(|((lemma, pos), a)| {
            let freq = match query.scope.unit {
                CountUnit::Tokens => a.raw,
                CountUnit::DistinctLines => a.distinct,
            };
            let mut surfaces: Vec<(String, i64)> = a.surfaces.into_iter().collect();
            surfaces.sort_by(|x, y| y.1.cmp(&x.1).then_with(|| x.0.cmp(&y.0)));
            FrequencyItem {
                lemma,
                pos,
                freq,
                per_10k: per_10k(freq, total),
                raw_freq: a.raw,
                distinct_freq: a.distinct,
                repeat_share: if a.raw > 0 { 1.0 - a.distinct as f64 / a.raw as f64 } else { 0.0 },
                songs: a.songs.len() as i64,
                range: a.songs.len() as f64 / song_total,
                surfaces: surfaces.into_iter().take(5).map(|(s, _)| s).collect(),
            }
        })
        .filter(|item| item.freq > 0)
        .collect();
    // 频次相同按词元排，结果才可复现（HashMap 的顺序每次运行都不同）
    items.sort_by(|a, b| b.freq.cmp(&a.freq).then_with(|| a.lemma.cmp(&b.lemma)).then_with(|| a.pos.cmp(&b.pos)));
    items.truncate(query.limit.unwrap_or(500));
    let provenance = provenance(conn, &query.scope, &lines, serde_json::to_value(query)?)?;
    Ok(FrequencyResult { items, provenance })
}

// ────────────────────────────── 搭配 ──────────────────────────────

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CollocationMeasure {
    /// logDice：不受语料大小影响，低频噪音少。默认
    #[default]
    LogDice,
    /// 互信息：偏爱罕见的固定搭配
    Mi,
    /// 对数似然：显著性
    LogLikelihood,
    /// 共现次数
    Frequency,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CollocationQuery {
    pub scope: Scope,
    /// 节点词
    pub node: String,
    pub field: MatchField,
    /// 左右窗口（词数，不含标点）。窗口不跨行——歌词的行就是自然的语义单位
    pub left: usize,
    pub right: usize,
    /// 共现少于这个数的不列出
    pub min_freq: i64,
    /// 只看这种词性的搭配词，空是全部
    pub collocate_pos: Option<String>,
    /// 只看实词搭配：去掉助词、助动词、连词、冠词类（ADP AUX SCONJ CCONJ PART DET）。
    /// 不去的话「君」的前几名全是「の が を に」，研究价值低
    pub content_only: bool,
    pub measure: CollocationMeasure,
    pub limit: Option<usize>,
}

impl Default for CollocationQuery {
    fn default() -> Self {
        Self {
            scope: Scope::default(),
            node: String::new(),
            field: MatchField::Lemma,
            left: 3,
            right: 3,
            min_freq: 2,
            collocate_pos: None,
            content_only: false,
            measure: CollocationMeasure::LogDice,
            limit: Some(100),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collocate {
    pub lemma: String,
    pub pos: String,
    /// 共现次数
    pub freq: i64,
    pub freq_left: i64,
    pub freq_right: i64,
    /// 搭配词在子语料里的总频次
    pub collocate_freq: i64,
    /// 共现发生在几首歌里
    pub songs: i64,
    pub log_dice: f64,
    pub mi: f64,
    pub log_likelihood: f64,
    pub t_score: f64,
    /// 第一次共现的那一行，点了直接去听
    pub example_utterance_id: i64,
    pub example_song_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollocationResult {
    /// 节点词出现了几次（按所选单位）
    pub node_freq: i64,
    pub collocates: Vec<Collocate>,
    pub provenance: Provenance,
}

fn matches(tok: &Tok, field: MatchField, node: &str) -> bool {
    match field {
        MatchField::Lemma => tok.lemma == node,
        MatchField::Surface => tok.surface == node,
    }
}

pub fn collocations(conn: &Connection, query: &CollocationQuery) -> Result<CollocationResult> {
    let lines = load_lines(conn, &query.scope)?;
    let node = query.node.trim();
    let unit = query.scope.unit;

    #[derive(Default)]
    struct Acc {
        left: i64,
        right: i64,
        songs: HashSet<String>,
        example: Option<(i64, String)>,
    }
    let mut acc: HashMap<(String, String), Acc> = HashMap::new();
    let mut corpus_freq: HashMap<(String, String), i64> = HashMap::new();
    let mut total: i64 = 0;
    let mut node_freq: i64 = 0;
    // 所有节点窗口里一共有多少个位置。期望值按它算，比 `节点数 × 窗口宽` 准（行首行尾的窗口是不满的）
    let mut window_slots: i64 = 0;

    for line in lines.iter().filter(|l| counted(l, unit)) {
        total += line.tokens.len() as i64;
        for tok in &line.tokens {
            *corpus_freq.entry((tok.lemma.clone(), tok.pos.clone())).or_default() += 1;
        }
        for (i, tok) in line.tokens.iter().enumerate() {
            if !matches(tok, query.field, node) {
                continue;
            }
            node_freq += 1;
            let from = i.saturating_sub(query.left);
            let to = (i + query.right).min(line.tokens.len().saturating_sub(1));
            for (j, other) in line.tokens.iter().enumerate().take(to + 1).skip(from) {
                if j == i {
                    continue;
                }
                window_slots += 1;
                let a = acc.entry((other.lemma.clone(), other.pos.clone())).or_default();
                if j < i {
                    a.left += 1;
                } else {
                    a.right += 1;
                }
                a.songs.insert(line.song_id.clone());
                if a.example.is_none() {
                    a.example = Some((line.utterance_id, line.song_id.clone()));
                }
            }
        }
    }

    let pos_filter = query.collocate_pos.as_deref().filter(|p| !p.is_empty());
    let n = total.max(1) as f64;
    let mut collocates: Vec<Collocate> = acc
        .into_iter()
        .filter(|((_, pos), _)| pos_filter.is_none_or(|p| p == pos))
        .filter(|((_, pos), _)| !query.content_only || !FUNCTION_POS.contains(&pos.as_str()))
        .filter_map(|((lemma, pos), a)| {
            let o = a.left + a.right;
            if o < query.min_freq.max(1) {
                return None;
            }
            let fc = *corpus_freq.get(&(lemma.clone(), pos.clone())).unwrap_or(&0);
            let ofl = o as f64;
            let expected = window_slots as f64 * fc as f64 / n;
            let mi = if expected > 0.0 { (ofl / expected).log2() } else { 0.0 };
            let t_score = if ofl > 0.0 { (ofl - expected) / ofl.sqrt() } else { 0.0 };
            let log_dice = 14.0 + (2.0 * ofl / (node_freq + fc).max(1) as f64).log2();
            // 2×2 表：行 = 在不在节点窗口里，列 = 是不是这个搭配词
            let a11 = ofl;
            let a12 = (window_slots as f64 - ofl).max(0.0);
            let a21 = (fc as f64 - ofl).max(0.0);
            let a22 = (n - window_slots as f64 - a21).max(0.0);
            let log_likelihood = g2(&[a11, a12, a21, a22]);
            let (example_utterance_id, example_song_id) = a.example.unwrap_or_default();
            Some(Collocate {
                lemma,
                pos,
                freq: o,
                freq_left: a.left,
                freq_right: a.right,
                collocate_freq: fc,
                songs: a.songs.len() as i64,
                log_dice,
                mi,
                log_likelihood,
                t_score,
                example_utterance_id,
                example_song_id,
            })
        })
        .collect();
    let key = |c: &Collocate| match query.measure {
        CollocationMeasure::LogDice => c.log_dice,
        CollocationMeasure::Mi => c.mi,
        CollocationMeasure::LogLikelihood => c.log_likelihood,
        CollocationMeasure::Frequency => c.freq as f64,
    };
    collocates.sort_by(|a, b| {
        key(b)
            .partial_cmp(&key(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.freq.cmp(&a.freq))
            .then_with(|| a.lemma.cmp(&b.lemma))
            .then_with(|| a.pos.cmp(&b.pos))
    });
    collocates.truncate(query.limit.unwrap_or(100));
    let provenance = provenance(conn, &query.scope, &lines, serde_json::to_value(query)?)?;
    Ok(CollocationResult { node_freq, collocates, provenance })
}

/// 功能词的 UPOS。搭配表「只看实词」时去掉
pub const FUNCTION_POS: &[&str] = &["ADP", "AUX", "SCONJ", "CCONJ", "PART", "DET"];

/// 2×2 列联表的 G²（对数似然）。顺序 [a11, a12, a21, a22]
fn g2(cells: &[f64; 4]) -> f64 {
    let [a, b, c, d] = *cells;
    let n = a + b + c + d;
    if n <= 0.0 {
        return 0.0;
    }
    let rows = [a + b, c + d];
    let cols = [a + c, b + d];
    let observed = [[a, b], [c, d]];
    let mut sum = 0.0;
    for (i, row) in observed.iter().enumerate() {
        for (j, o) in row.iter().enumerate() {
            let e = rows[i] * cols[j] / n;
            if *o > 0.0 && e > 0.0 {
                sum += o * (o / e).ln();
            }
        }
    }
    2.0 * sum
}

// ────────────────────────────── 子语料对比 ──────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct KeynessQuery {
    /// 目标子语料
    pub target: Subcorpus,
    /// 参照子语料。None 是「语料库里其余的歌」
    pub reference: Option<Subcorpus>,
    pub unit: CountUnit,
    pub jp_only: bool,
    pub pos: Option<String>,
    /// 两边合计少于这个数的词不列出
    pub min_freq: i64,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyItem {
    pub lemma: String,
    pub pos: String,
    pub target_freq: i64,
    pub reference_freq: i64,
    pub target_per_10k: f64,
    pub reference_per_10k: f64,
    /// 对数似然，**带符号**：正 = 目标里偏多，负 = 偏少
    pub log_likelihood: f64,
    /// log2（目标相对频率 ÷ 参照相对频率），零频按 0.5 平滑。效应大小
    pub log_ratio: f64,
    /// 目标里出现在几首歌
    pub target_songs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeynessResult {
    /// 目标里偏多的词，按对数似然降序
    pub over: Vec<KeyItem>,
    /// 目标里偏少的词
    pub under: Vec<KeyItem>,
    pub target: Provenance,
    pub reference: Provenance,
}

pub fn keyness(conn: &Connection, query: &KeynessQuery) -> Result<KeynessResult> {
    let target_scope = Scope { subcorpus: query.target.clone(), unit: query.unit, jp_only: query.jp_only };
    let target_lines = load_lines(conn, &target_scope)?;
    let (reference_scope, reference_lines) = match &query.reference {
        Some(reference) => {
            let scope = Scope { subcorpus: reference.clone(), unit: query.unit, jp_only: query.jp_only };
            let lines = load_lines(conn, &scope)?;
            (scope, lines)
        }
        None => {
            // 其余的歌 = 全库减去目标
            let target_ids = query.target.song_ids(conn)?;
            let all = Scope { subcorpus: Subcorpus::default(), unit: query.unit, jp_only: query.jp_only };
            let lines: Vec<Line> = load_lines(conn, &all)?
                .into_iter()
                .filter(|l| !target_ids.contains(&l.song_id))
                .collect();
            let rest: Vec<String> = lines.iter().map(|l| l.song_id.clone()).collect::<HashSet<_>>().into_iter().collect();
            let mut rest = rest;
            rest.sort();
            (Scope { subcorpus: Subcorpus { song_ids: rest, ..Default::default() }, ..all }, lines)
        }
    };

    let pos = query.pos.as_deref().filter(|p| !p.is_empty());
    let count = |lines: &[Line]| {
        let mut freq: HashMap<(String, String), (i64, HashSet<String>)> = HashMap::new();
        let mut total = 0i64;
        for line in lines.iter().filter(|l| counted(l, query.unit)) {
            total += line.tokens.len() as i64;
            for tok in &line.tokens {
                if pos.is_some_and(|p| p != tok.pos) {
                    continue;
                }
                let e = freq.entry((tok.lemma.clone(), tok.pos.clone())).or_default();
                e.0 += 1;
                e.1.insert(line.song_id.clone());
            }
        }
        (freq, total)
    };
    let (tf, tn) = count(&target_lines);
    let (rf, rn) = count(&reference_lines);

    let keys: HashSet<&(String, String)> = tf.keys().chain(rf.keys()).collect();
    let mut items: Vec<KeyItem> = keys
        .into_iter()
        .filter_map(|key| {
            let (a, songs) = tf.get(key).map(|(f, s)| (*f, s.len() as i64)).unwrap_or((0, 0));
            let b = rf.get(key).map(|(f, _)| *f).unwrap_or(0);
            if a + b < query.min_freq.max(1) {
                return None;
            }
            let (af, bf, c, d) = (a as f64, b as f64, tn.max(1) as f64, rn.max(1) as f64);
            let e1 = c * (af + bf) / (c + d);
            let e2 = d * (af + bf) / (c + d);
            let mut ll = 0.0;
            if af > 0.0 {
                ll += af * (af / e1).ln();
            }
            if bf > 0.0 {
                ll += bf * (bf / e2).ln();
            }
            ll *= 2.0;
            let log_ratio = (((af.max(0.5)) / c) / ((bf.max(0.5)) / d)).log2();
            let signed = if af / c >= bf / d { ll } else { -ll };
            Some(KeyItem {
                lemma: key.0.clone(),
                pos: key.1.clone(),
                target_freq: a,
                reference_freq: b,
                target_per_10k: per_10k(a, tn),
                reference_per_10k: per_10k(b, rn),
                log_likelihood: signed,
                log_ratio,
                target_songs: songs,
            })
        })
        .collect();
    let by_strength = |x: &KeyItem, y: &KeyItem| {
        y.log_likelihood
            .abs()
            .partial_cmp(&x.log_likelihood.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| x.lemma.cmp(&y.lemma))
            .then_with(|| x.pos.cmp(&y.pos))
    };
    let limit = query.limit.unwrap_or(100);
    let mut under: Vec<KeyItem> = items.iter().filter(|i| i.log_likelihood < 0.0).cloned().collect();
    items.retain(|i| i.log_likelihood > 0.0);
    items.sort_by(by_strength);
    under.sort_by(by_strength);
    items.truncate(limit);
    under.truncate(limit);
    let params = serde_json::to_value(query)?;
    Ok(KeynessResult {
        over: items,
        under,
        target: provenance(conn, &target_scope, &target_lines, params.clone())?,
        reference: provenance(conn, &reference_scope, &reference_lines, params)?,
    })
}

// ────────────────────────────── 一首歌的语料画像 ──────────────────────────────

/// 从一首歌进入语料分析：这首歌里哪些词在全库里也常见、哪些是它独有的，副歌占了多少。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongProfile {
    pub song_id: String,
    pub lines: i64,
    pub repeated_lines: i64,
    pub tokens: i64,
    pub distinct_tokens: i64,
    /// 这首歌相对于其余歌的特征词（偏多的那一侧）
    pub keywords: Vec<KeyItem>,
    pub provenance: Provenance,
}

pub fn song_profile(conn: &Connection, song_id: &str, limit: usize) -> Result<SongProfile> {
    let scope = Scope {
        subcorpus: Subcorpus { song_ids: vec![song_id.to_string()], ..Default::default() },
        unit: CountUnit::Tokens,
        jp_only: true,
    };
    let lines = load_lines(conn, &scope)?;
    let keys = keyness(
        conn,
        &KeynessQuery {
            target: scope.subcorpus.clone(),
            reference: None,
            unit: CountUnit::DistinctLines,
            jp_only: true,
            pos: None,
            min_freq: 2,
            // 先多取一些：功能词（だ、て、の）筛掉之后还要剩够 limit 个
            limit: Some(limit * 4),
        },
    )?;
    // 特征词只要实词。「だ」「て」在某首歌里偏多说明不了这首歌写了什么
    let keywords: Vec<KeyItem> = keys
        .over
        .into_iter()
        .filter(|k| !FUNCTION_POS.contains(&k.pos.as_str()) && k.pos != "PRON" && k.pos != "NUM")
        .take(limit)
        .collect();
    Ok(SongProfile {
        song_id: song_id.to_string(),
        lines: lines.len() as i64,
        repeated_lines: lines.iter().filter(|l| l.repeat).count() as i64,
        tokens: lines.iter().map(|l| l.tokens.len() as i64).sum(),
        distinct_tokens: lines.iter().filter(|l| !l.repeat).map(|l| l.tokens.len() as i64).sum(),
        keywords,
        provenance: provenance(conn, &scope, &lines, serde_json::json!({ "songId": song_id, "limit": limit }))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> crate::Corpus {
        crate::fixture::corpus().unwrap()
    }

    #[test]
    fn distinct_lines_never_count_more_than_tokens_and_repeat_share_is_consistent() {
        let corpus = conn();
        let tokens = frequency(corpus.connection(), &FrequencyQuery::default()).unwrap();
        let distinct = frequency(
            corpus.connection(),
            &FrequencyQuery { scope: Scope { unit: CountUnit::DistinctLines, ..Default::default() }, ..Default::default() },
        )
        .unwrap();
        assert!(distinct.provenance.tokens < tokens.provenance.tokens, "夹具 004 有一行副歌唱了两遍");
        for item in &tokens.items {
            assert!(item.distinct_freq <= item.raw_freq);
            assert_eq!(item.freq, item.raw_freq);
            let share = 1.0 - item.distinct_freq as f64 / item.raw_freq as f64;
            assert!((item.repeat_share - share).abs() < 1e-12);
        }
        // 词次总和就是分母
        let sum: i64 = tokens.items.iter().map(|i| i.freq).sum();
        assert_eq!(sum, tokens.provenance.tokens);
        assert!(!tokens.items.iter().any(|i| i.pos == "PUNCT" || i.pos == "SYM"));
    }

    #[test]
    fn frequency_is_deterministic_and_snapshot_changes_when_tokens_change() {
        let corpus = conn();
        let a = frequency(corpus.connection(), &FrequencyQuery::default()).unwrap();
        let b = frequency(corpus.connection(), &FrequencyQuery::default()).unwrap();
        assert_eq!(
            a.items.iter().map(|i| (&i.lemma, i.freq)).collect::<Vec<_>>(),
            b.items.iter().map(|i| (&i.lemma, i.freq)).collect::<Vec<_>>()
        );
        assert_eq!(a.provenance.snapshot, b.provenance.snapshot);
        corpus
            .connection()
            .execute("DELETE FROM tokens WHERE id = (SELECT MAX(id) FROM tokens)", [])
            .unwrap();
        let after = snapshot(corpus.connection()).unwrap();
        assert_ne!(after.fingerprint, a.provenance.snapshot.fingerprint);
    }

    #[test]
    fn collocates_come_from_the_window_and_are_sorted_by_the_measure() {
        let corpus = conn();
        let top = frequency(corpus.connection(), &FrequencyQuery::default()).unwrap();
        let node = top.items.iter().find(|i| i.freq >= 2).unwrap().lemma.clone();
        let res = collocations(
            corpus.connection(),
            &CollocationQuery { node: node.clone(), min_freq: 1, ..Default::default() },
        )
        .unwrap();
        assert!(res.node_freq >= 2);
        assert!(!res.collocates.is_empty());
        for pair in res.collocates.windows(2) {
            assert!(pair[0].log_dice >= pair[1].log_dice);
        }
        for c in &res.collocates {
            assert_eq!(c.freq, c.freq_left + c.freq_right);
            assert!(c.example_utterance_id > 0 && !c.example_song_id.is_empty());
            assert!(c.log_likelihood >= 0.0);
        }
        // 窗口为 0 就没有搭配
        let none = collocations(
            corpus.connection(),
            &CollocationQuery { node, left: 0, right: 0, min_freq: 1, ..Default::default() },
        )
        .unwrap();
        assert!(none.collocates.is_empty());
    }

    #[test]
    fn keyness_against_the_rest_partitions_the_corpus() {
        let corpus = conn();
        let target = Subcorpus { song_ids: vec!["001".into()], ..Default::default() };
        let res = keyness(corpus.connection(), &KeynessQuery { target, min_freq: 1, ..Default::default() }).unwrap();
        let whole = frequency(corpus.connection(), &FrequencyQuery::default()).unwrap();
        assert_eq!(res.target.tokens + res.reference.tokens, whole.provenance.tokens);
        assert_eq!(res.target.songs, 1);
        assert!(res.over.iter().all(|k| k.log_likelihood > 0.0 && k.target_freq > 0));
        assert!(res.under.iter().all(|k| k.log_likelihood < 0.0));
    }

    #[test]
    fn g2_is_zero_for_independent_tables() {
        assert!(g2(&[10.0, 10.0, 10.0, 10.0]).abs() < 1e-9);
        assert!(g2(&[30.0, 10.0, 10.0, 30.0]) > 0.0);
    }

    #[test]
    fn song_profile_counts_repeated_lines() {
        let corpus = conn();
        let p = song_profile(corpus.connection(), crate::fixture::RICH_SONG, 10).unwrap();
        assert!(p.repeated_lines >= 1);
        assert!(p.distinct_tokens < p.tokens);
        assert!(p.keywords.iter().all(|k| !FUNCTION_POS.contains(&k.pos.as_str())));
        assert!(p.keywords.len() <= 10);
    }
}
