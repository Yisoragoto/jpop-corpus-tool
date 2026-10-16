//! 可复现的分析：一次统计连同它的条件、范围、分词器、口径和结果一起存下来，
//! 之后能原样重跑，并且说得清新旧两次为什么不一样。
//!
//! 「能生成图表」和「能验证分析」差在这里：同一张词频表，三个月后再看，
//! 得知道当时纳入了哪些歌、每首歌是哪一版歌词、分词用的哪本词典、
//! 「词」是怎么数的。差一样，数字对不上就无从解释。
//!
//! 一次分析存这几样（`analysis_runs`）：
//!
//! | 字段 | 内容 |
//! |---|---|
//! | `filter_json` | 筛选条件（[`StatsFilter`]），重跑时原样用 |
//! | `params_json` | 分析种类和参数（[`AnalysisSpec`]） |
//! | `scope_json` | 当时纳入的每首歌和它的歌词版本 id |
//! | `tokenizers_json` | 这些歌词行用过的分词器（引擎、词典、切分模式、配置指纹） |
//! | `method_json` | 统计口径：词元怎么取、排除哪些词性、各指标的定义 |
//! | `result_json` | 结果本身 |
//!
//! 重跑（[`rerun`]）= 拿同样的条件和参数在**现在的库**上再算一次，另存一条记录，
//! 再和原来那条逐项比（[`compare`]）：范围变了哪些歌、哪些歌换了歌词、分词器变没变、
//! 每个指标差多少。

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::lineage::{Fingerprint, NOW, TokenizerRunRow, tokenizer_runs};
use crate::stats::{self, StatsFilter};

/// 分析种类和参数
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AnalysisSpec {
    /// 语料统计报告：TTR / STTR / Hapax / 词性分布 / 覆盖率
    CorpusReport,
    /// 词频表：词元 × 词性
    #[serde(rename_all = "camelCase")]
    Frequency { pos: Option<String>, limit: usize },
}

impl AnalysisSpec {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::CorpusReport => "corpusReport",
            Self::Frequency { .. } => "frequency",
        }
    }
}

/// 范围里的一首歌：歌和当时的歌词版本
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeSong {
    pub song_id: String,
    /// 老库里没登记版本的歌是 `None`
    pub version_id: Option<i64>,
    pub lines: i64,
}

/// 一次分析的范围
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub songs: Vec<ScopeSong>,
    pub song_count: usize,
    pub line_count: i64,
}

/// 范围里用到的分词器
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenizerUse {
    pub runs: Vec<TokenizerRunRow>,
    /// 不知道是哪个分词器分的行（老库导进来的）
    pub unknown_lines: i64,
}

/// 按条件取范围。和统计用的是**同一个子句**，所以范围就是统计实际看到的那批行。
pub fn scope(conn: &Connection, filter: &StatsFilter) -> Result<Scope> {
    let (clause, values) = filter.clause();
    let mut stmt = conn.prepare(&format!(
        "SELECT u.song_id, u.version_id, COUNT(*) FROM utterances u WHERE 1=1{clause} \
         GROUP BY u.song_id, u.version_id ORDER BY u.song_id, u.version_id"
    ))?;
    let songs: Vec<ScopeSong> = stmt
        .query_map(params_from_iter(values), |r| Ok(ScopeSong { song_id: r.get(0)?, version_id: r.get(1)?, lines: r.get(2)? }))?
        .collect::<rusqlite::Result<_>>()?;
    let song_count = songs.iter().map(|s| s.song_id.as_str()).collect::<BTreeSet<_>>().len();
    let line_count = songs.iter().map(|s| s.lines).sum();
    Ok(Scope { songs, song_count, line_count })
}

fn tokenizers_in_scope(conn: &Connection, filter: &StatsFilter) -> Result<TokenizerUse> {
    let (clause, values) = filter.clause();
    let mut stmt = conn.prepare(&format!(
        "SELECT u.tokenizer_run_id, COUNT(*) FROM utterances u WHERE 1=1{clause} \
         AND EXISTS (SELECT 1 FROM tokens t WHERE t.utterance_id = u.id) GROUP BY u.tokenizer_run_id"
    ))?;
    let counts: Vec<(Option<i64>, i64)> =
        stmt.query_map(params_from_iter(values), |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    let ids: Vec<i64> = counts.iter().filter_map(|(id, _)| *id).collect();
    let mut runs = tokenizer_runs(conn, &ids)?;
    for run in &mut runs {
        run.line_count = counts.iter().find(|(id, _)| *id == Some(run.id)).map_or(0, |(_, n)| *n);
    }
    let unknown_lines = counts.iter().filter(|(id, _)| id.is_none()).map(|(_, n)| n).sum();
    Ok(TokenizerUse { runs, unknown_lines })
}

fn scope_hash(scope: &Scope) -> String {
    let mut h = Fingerprint::new();
    for s in &scope.songs {
        h.str(&s.song_id).str(&s.version_id.map(|v| v.to_string()).unwrap_or_default()).str(&s.lines.to_string());
    }
    h.hex()
}

/// 统计口径。写成人能读的话，存进记录里——三个月后看的人不一定记得代码是怎么写的
pub fn method(spec: &AnalysisSpec, filter: &StatsFilter) -> serde_json::Value {
    let common = serde_json::json!({
        "tokenUnit": "Sudachi SplitMode C 的词元（dictionary_form），词性是 UniDic 映射到的 UPOS",
        "whitespace": "空白 token 在入库时就不收",
        "jpOnly": if filter.jp_only { "只算词元里有平假名、片假名或汉字的" } else { "全部词元" },
        "lines": if filter.distinct_lines { "同一首歌里文本相同的行只算第一次（副歌只算一遍）" } else { "每一行都算（副歌唱几遍算几遍）" },
        "corrections": "分词校正过的行用校正后的分词",
    });
    let specific = match spec {
        AnalysisSpec::CorpusReport => serde_json::json!({
            "excludedPos": ["PUNCT", "SYM", "SPACE", "X"],
            "tokenCount": "词次：排除上述词性后的 token 数",
            "typeCount": "词种：不同词元数（不分词性）",
            "ttr": "词种 ÷ 词次",
            "sttr": "按歌曲、行、词序排好，词次 ≥ 5000 时每 1000 个一段、≥ 1000 时每 500 个一段，取完整段 TTR 的平均",
            "hapax": "只出现一次的词元",
            "coverage": "前 N 个高频词元占全部词次的比例",
        }),
        AnalysisSpec::Frequency { pos, limit } => serde_json::json!({
            "excludedPos": ["PUNCT", "SYM"],
            "unit": "词元 × 词性 的组合",
            "pos": pos.clone().unwrap_or_else(|| "全部".into()),
            "limit": limit,
            "order": "频次降序，同频次按词元、词性排",
            "songCount": "出现过这个词的歌的数目",
        }),
    };
    let mut out = common;
    if let (Some(a), serde_json::Value::Object(b)) = (out.as_object_mut(), specific) {
        a.extend(b);
    }
    out
}

/// 只算，不存
pub fn execute(conn: &Connection, spec: &AnalysisSpec, filter: &StatsFilter) -> Result<serde_json::Value> {
    Ok(match spec {
        AnalysisSpec::CorpusReport => serde_json::to_value(stats::corpus_report(conn, filter)?)?,
        AnalysisSpec::Frequency { pos, limit } => {
            serde_json::to_value(stats::frequency_table(conn, pos.as_deref(), filter, (*limit).clamp(1, 5000))?)?
        }
    })
}

/// 一次分析的完整记录
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisRun {
    pub id: i64,
    pub label: String,
    pub spec: AnalysisSpec,
    pub filter: StatsFilter,
    pub scope: Scope,
    pub scope_hash: String,
    pub tokenizers: TokenizerUse,
    pub method: serde_json::Value,
    pub result: serde_json::Value,
    pub result_hash: String,
    pub schema_version: i64,
    pub app_version: String,
    pub rerun_of: Option<i64>,
    pub created_at: String,
}

/// 列表里的一行（不带结果本身）
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisSummary {
    pub id: i64,
    pub kind: String,
    pub label: String,
    pub filter: StatsFilter,
    pub song_count: usize,
    pub scope_hash: String,
    pub result_hash: String,
    pub app_version: String,
    pub rerun_of: Option<i64>,
    pub created_at: String,
}

/// 算一次并存下来
pub fn run(
    conn: &Connection,
    spec: &AnalysisSpec,
    filter: &StatsFilter,
    label: &str,
    app_version: &str,
    rerun_of: Option<i64>,
) -> Result<AnalysisRun> {
    // 范围、分词器、结果要是同一个时刻的库：放在一个读事务里，中间有人写也看不见
    let tx = conn.unchecked_transaction()?;
    let scope = scope(&tx, filter)?;
    let tokenizers = tokenizers_in_scope(&tx, filter)?;
    let result = execute(&tx, spec, filter)?;
    let method = method(spec, filter);
    let schema_version = crate::migrations::version(&tx)?;
    let result_hash = Fingerprint::new().str(&result.to_string()).hex();
    let scope_hash = scope_hash(&scope);
    tx.execute(
        &format!(
            "INSERT INTO analysis_runs (kind, label, filter_json, params_json, scope_json, scope_hash, tokenizers_json, \
             method_json, result_json, result_hash, schema_version, app_version, rerun_of, created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,{NOW})"
        ),
        params![
            spec.kind(),
            label.trim(),
            serde_json::to_string(filter)?,
            serde_json::to_string(spec)?,
            serde_json::to_string(&scope)?,
            scope_hash,
            serde_json::to_string(&tokenizers)?,
            method.to_string(),
            result.to_string(),
            result_hash,
            schema_version,
            app_version,
            rerun_of,
        ],
    )?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    get(conn, id)?.context("刚存的分析记录读不回来")
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<AnalysisRun>> {
    let row = conn
        .query_row(
            "SELECT id, label, params_json, filter_json, scope_json, scope_hash, tokenizers_json, method_json, \
             result_json, result_hash, schema_version, app_version, rerun_of, created_at FROM analysis_runs WHERE id=?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    [r.get::<_, String>(2)?, r.get(3)?, r.get(4)?],
                    r.get::<_, String>(5)?,
                    [r.get::<_, String>(6)?, r.get(7)?, r.get(8)?],
                    r.get::<_, String>(9)?,
                    r.get::<_, i64>(10)?,
                    r.get::<_, String>(11)?,
                    r.get::<_, Option<i64>>(12)?,
                    r.get::<_, String>(13)?,
                ))
            },
        )
        .optional()?;
    let Some((id, label, [spec, filter, scope], scope_hash, [tokenizers, method, result], result_hash, schema_version, app_version, rerun_of, created_at)) = row
    else {
        return Ok(None);
    };
    Ok(Some(AnalysisRun {
        id,
        label,
        spec: serde_json::from_str(&spec).context("分析参数读不出来")?,
        filter: serde_json::from_str(&filter).context("筛选条件读不出来")?,
        scope: serde_json::from_str(&scope).unwrap_or_default(),
        scope_hash,
        tokenizers: serde_json::from_str(&tokenizers).unwrap_or_default(),
        method: serde_json::from_str(&method).unwrap_or_default(),
        result: serde_json::from_str(&result).unwrap_or_default(),
        result_hash,
        schema_version,
        app_version,
        rerun_of,
        created_at,
    }))
}

/// 存过的分析，新的在前
pub fn list(conn: &Connection, limit: i64) -> Result<Vec<AnalysisSummary>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, label, filter_json, scope_json, scope_hash, result_hash, app_version, rerun_of, created_at \
         FROM analysis_runs ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, Option<i64>>(8)?,
            r.get::<_, String>(9)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, kind, label, filter, scope, scope_hash, result_hash, app_version, rerun_of, created_at) = row?;
        let scope: Scope = serde_json::from_str(&scope).unwrap_or_default();
        out.push(AnalysisSummary {
            id,
            kind,
            label,
            filter: serde_json::from_str(&filter).unwrap_or_default(),
            song_count: scope.song_count,
            scope_hash,
            result_hash,
            app_version,
            rerun_of,
            created_at,
        });
    }
    Ok(out)
}

pub fn delete(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn.execute("DELETE FROM analysis_runs WHERE id=?1", params![id])? > 0)
}

// ── 重跑与对比 ───────────────────────────────────────────────────

/// 范围上的差别
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// 两次都在、但歌词版本或行数变了的
    pub lyrics_changed: Vec<String>,
}

/// 一个数值指标的前后
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricDiff {
    pub name: String,
    pub before: Option<f64>,
    pub after: Option<f64>,
}

/// 词频表上一个词的前后（只列有变化的）
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordDiff {
    pub lemma: String,
    pub pos: String,
    pub before_rank: Option<usize>,
    pub after_rank: Option<usize>,
    pub before_freq: Option<i64>,
    pub after_freq: Option<i64>,
}

/// 两次分析的对比
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub before: AnalysisSummary,
    pub after: AnalysisSummary,
    pub same_filter: bool,
    pub same_spec: bool,
    pub same_method: bool,
    pub same_scope: bool,
    pub same_tokenizers: bool,
    pub same_schema: bool,
    pub result_identical: bool,
    pub scope: ScopeDiff,
    /// 报告的各项指标（只列变了的）
    pub metrics: Vec<MetricDiff>,
    /// 词频表的变化（最多 200 条）
    pub words: Vec<WordDiff>,
    /// 一句句人话：为什么不一样
    pub reasons: Vec<String>,
}

/// 拿记录里的条件和参数在现在的库上再跑一次，另存一条，并和原来那条比
pub fn rerun(conn: &Connection, id: i64, app_version: &str) -> Result<Comparison> {
    let Some(before) = get(conn, id)? else { bail!("找不到分析记录 {id}") };
    let label = if before.label.is_empty() { format!("重跑 #{id}") } else { format!("{}（重跑）", before.label) };
    let after = run(conn, &before.spec, &before.filter, &label, app_version, Some(id))?;
    Ok(compare(&before, &after))
}

fn summary(run: &AnalysisRun) -> AnalysisSummary {
    AnalysisSummary {
        id: run.id,
        kind: run.spec.kind().into(),
        label: run.label.clone(),
        filter: run.filter.clone(),
        song_count: run.scope.song_count,
        scope_hash: run.scope_hash.clone(),
        result_hash: run.result_hash.clone(),
        app_version: run.app_version.clone(),
        rerun_of: run.rerun_of,
        created_at: run.created_at.clone(),
    }
}

/// 两次分析逐项比。不要求是同一种分析——条件、种类不同也照比，`reasons` 里会说
pub fn compare(before: &AnalysisRun, after: &AnalysisRun) -> Comparison {
    let mut reasons = Vec::new();
    let same_filter = before.filter == after.filter;
    let same_spec = before.spec == after.spec;
    let same_method = before.method == after.method;
    let same_scope = before.scope_hash == after.scope_hash;
    let same_schema = before.schema_version == after.schema_version;
    let fingerprints = |t: &TokenizerUse| (t.runs.iter().map(|r| r.fingerprint.clone()).collect::<BTreeSet<_>>(), t.unknown_lines > 0);
    let same_tokenizers = fingerprints(&before.tokenizers) == fingerprints(&after.tokenizers);
    let result_identical = before.result_hash == after.result_hash;

    if !same_filter {
        reasons.push("筛选条件不同".into());
    }
    if !same_spec {
        reasons.push("分析种类或参数不同".into());
    }
    if !same_method {
        reasons.push("统计口径的说明变了（程序版本更新了统计方法）".into());
    }

    let index = |s: &Scope| -> BTreeMap<String, Vec<(Option<i64>, i64)>> {
        let mut m: BTreeMap<String, Vec<(Option<i64>, i64)>> = BTreeMap::new();
        for song in &s.songs {
            m.entry(song.song_id.clone()).or_default().push((song.version_id, song.lines));
        }
        m
    };
    let (a, b) = (index(&before.scope), index(&after.scope));
    let mut scope = ScopeDiff::default();
    for (song, versions) in &a {
        match b.get(song) {
            None => scope.removed.push(song.clone()),
            Some(other) if other != versions => scope.lyrics_changed.push(song.clone()),
            Some(_) => {}
        }
    }
    scope.added = b.keys().filter(|k| !a.contains_key(*k)).cloned().collect();
    if !scope.added.is_empty() {
        reasons.push(format!("新纳入 {} 首歌（导入了新歌，或者歌的信息改到了条件里）", scope.added.len()));
    }
    if !scope.removed.is_empty() {
        reasons.push(format!("少了 {} 首歌（删了，或者不再符合条件）", scope.removed.len()));
    }
    if !scope.lyrics_changed.is_empty() {
        reasons.push(format!("{} 首歌换了歌词版本", scope.lyrics_changed.len()));
    }
    if !same_tokenizers {
        reasons.push("用到的分词器（引擎、词典或配置）不同".into());
    }
    if !same_schema {
        reasons.push(format!("库结构版本 {} → {}", before.schema_version, after.schema_version));
    }

    let metrics = metric_diffs(&before.result, &after.result);
    let words = word_diffs(&before.result, &after.result);
    if !result_identical && reasons.is_empty() {
        // 范围、分词器、口径都没变，结果却变了：最可能是有人校正了分词
        reasons.push("范围和分词器都没变：多半是有歌词行的分词被校正过（见各歌的修订历史）".into());
    }

    Comparison {
        before: summary(before),
        after: summary(after),
        same_filter,
        same_spec,
        same_method,
        same_scope,
        same_tokenizers,
        same_schema,
        result_identical,
        scope,
        metrics,
        words,
        reasons,
    }
}

/// 报告里的数值指标。覆盖率按 N 展开成 `coverage@N`
fn metric_diffs(a: &serde_json::Value, b: &serde_json::Value) -> Vec<MetricDiff> {
    let flat = |v: &serde_json::Value| -> BTreeMap<String, f64> {
        let mut out = BTreeMap::new();
        let Some(obj) = v.as_object() else { return out };
        for (k, v) in obj {
            if let Some(x) = v.as_f64() {
                out.insert(k.clone(), x);
            }
        }
        if let Some(cov) = obj.get("coverage").and_then(|c| c.as_array()) {
            for c in cov {
                if let (Some(n), Some(p)) = (c.get("n").and_then(|n| n.as_i64()), c.get("pct").and_then(|p| p.as_f64())) {
                    out.insert(format!("coverage@{n}"), p);
                }
            }
        }
        out
    };
    let (x, y) = (flat(a), flat(b));
    let names: BTreeSet<&String> = x.keys().chain(y.keys()).collect();
    names
        .into_iter()
        .filter_map(|name| {
            let (before, after) = (x.get(name).copied(), y.get(name).copied());
            let same = match (before, after) {
                (Some(p), Some(q)) => (p - q).abs() < 1e-12,
                (None, None) => true,
                _ => false,
            };
            (!same).then(|| MetricDiff { name: name.clone(), before, after })
        })
        .collect()
}

/// 词频表的变化：频次或名次变了的、新进榜的、掉出榜的
fn word_diffs(a: &serde_json::Value, b: &serde_json::Value) -> Vec<WordDiff> {
    let table = |v: &serde_json::Value| -> BTreeMap<(String, String), (usize, i64)> {
        let mut out = BTreeMap::new();
        for (rank, row) in v.as_array().into_iter().flatten().enumerate() {
            let lemma = row.get("lemma").and_then(|x| x.as_str()).unwrap_or_default().to_string();
            let pos = row.get("pos").and_then(|x| x.as_str()).unwrap_or_default().to_string();
            let freq = row.get("freq").and_then(|x| x.as_i64()).unwrap_or_default();
            out.insert((lemma, pos), (rank + 1, freq));
        }
        out
    };
    let (x, y) = (table(a), table(b));
    if x.is_empty() && y.is_empty() {
        return Vec::new();
    }
    let keys: BTreeSet<&(String, String)> = x.keys().chain(y.keys()).collect();
    let mut out: Vec<WordDiff> = keys
        .into_iter()
        .filter_map(|key| {
            let (p, q) = (x.get(key), y.get(key));
            (p != q).then(|| WordDiff {
                lemma: key.0.clone(),
                pos: key.1.clone(),
                before_rank: p.map(|v| v.0),
                after_rank: q.map(|v| v.0),
                before_freq: p.map(|v| v.1),
                after_freq: q.map(|v| v.1),
            })
        })
        .collect();
    // 原来名次靠前的先列
    out.sort_by_key(|d| (d.before_rank.or(d.after_rank).unwrap_or(usize::MAX), d.after_rank.unwrap_or(usize::MAX)));
    out.truncate(200);
    out
}

// ── 子语料 ───────────────────────────────────────────────────────

/// 存起来的子语料
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subcorpus {
    pub id: i64,
    pub name: String,
    pub filter: StatsFilter,
    pub note: String,
    pub created_at: String,
    pub updated_at: String,
    /// 按现在的库算，有几首歌、几行
    pub song_count: usize,
    pub line_count: i64,
}

/// 存一个子语料；同名的覆盖
pub fn save_subcorpus(conn: &Connection, name: &str, filter: &StatsFilter, note: &str) -> Result<Subcorpus> {
    let name = name.trim();
    if name.is_empty() {
        bail!("子语料要有个名字");
    }
    conn.execute(
        &format!(
            "INSERT INTO subcorpora (name, filter_json, note, created_at, updated_at) VALUES (?1,?2,?3,{NOW},{NOW}) \
             ON CONFLICT(name) DO UPDATE SET filter_json=excluded.filter_json, note=excluded.note, updated_at={NOW}"
        ),
        params![name, serde_json::to_string(filter)?, note.trim()],
    )?;
    subcorpora(conn)?.into_iter().find(|s| s.name == name).context("刚存的子语料读不回来")
}

pub fn subcorpora(conn: &Connection) -> Result<Vec<Subcorpus>> {
    let rows: Vec<(i64, String, String, String, String, String)> = {
        let mut stmt = conn.prepare("SELECT id, name, filter_json, note, created_at, updated_at FROM subcorpora ORDER BY name")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut out = Vec::new();
    for (id, name, filter, note, created_at, updated_at) in rows {
        let filter: StatsFilter = serde_json::from_str(&filter).unwrap_or_default();
        let s = scope(conn, &filter)?;
        out.push(Subcorpus { id, name, filter, note, created_at, updated_at, song_count: s.song_count, line_count: s.line_count });
    }
    Ok(out)
}

pub fn delete_subcorpus(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn.execute("DELETE FROM subcorpora WHERE id=?1", params![id])? > 0)
}

/// 建子语料时可选的值：库里出现过的年份范围、流派、语言（各带首数）
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterFacets {
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
    pub songs_without_year: i64,
    pub genres: Vec<(String, i64)>,
    pub languages: Vec<(String, i64)>,
}

pub fn facets(conn: &Connection) -> Result<FilterFacets> {
    let year = "CAST(substr(COALESCE(NULLIF(release_date,''), NULLIF(year,''), ''), 1, 4) AS INTEGER)";
    let (year_min, year_max, songs_without_year): (Option<i32>, Option<i32>, i64) = conn.query_row(
        &format!(
            "SELECT MIN(CASE WHEN {year} > 0 THEN {year} END), MAX(CASE WHEN {year} > 0 THEN {year} END), \
             SUM(CASE WHEN {year} > 0 THEN 0 ELSE 1 END) FROM songs"
        ),
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<i64>>(2)?.unwrap_or(0))),
    )?;
    let pairs = |sql: &str| -> Result<Vec<(String, i64)>> {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    };
    Ok(FilterFacets {
        year_min,
        year_max,
        songs_without_year,
        genres: pairs(
            "SELECT TRIM(genre), COUNT(*) FROM songs WHERE TRIM(COALESCE(genre,'')) <> '' GROUP BY LOWER(TRIM(genre)) ORDER BY 2 DESC, 1",
        )?,
        languages: pairs(
            "SELECT language, COUNT(*) FROM songs WHERE COALESCE(language,'') <> '' GROUP BY language ORDER BY 2 DESC, 1",
        )?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lineage::{self, LyricSource, TokenizerRun};
    use crate::quality::LineInput;

    /// 两首歌：001（2019，J-Pop，副歌重复）、002（2023，Rock）
    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::Corpus::ensure_schema_on(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO songs (id, title, artist, year, genre, duration_sec) VALUES
               ('001','夜に駆ける','YOASOBI','2019','J-Pop',30.0),
               ('002','怪獣','サカナクション','2023-01-01','Rock',30.0);
             INSERT INTO utterances (id, song_id, line_idx, time_sec, text) VALUES
               (1,'001',0,1.0,'夜に駆ける'),(2,'001',1,2.0,'夜に駆ける'),(3,'002',0,1.0,'怪獣の夜');
             INSERT INTO tokens (utterance_id, token_idx, surface, lemma, pos) VALUES
               (1,0,'夜','夜','NOUN'),(1,1,'に','に','ADP'),(1,2,'駆ける','駆ける','VERB'),
               (2,0,'夜','夜','NOUN'),(2,1,'に','に','ADP'),(2,2,'駆ける','駆ける','VERB'),
               (3,0,'怪獣','怪獣','NOUN'),(3,1,'の','の','ADP'),(3,2,'夜','夜','NOUN');",
        )
        .unwrap();
        lineage::backfill_legacy(&conn).unwrap();
        conn
    }

    fn freq() -> AnalysisSpec {
        AnalysisSpec::Frequency { pos: None, limit: 50 }
    }

    #[test]
    fn subcorpus_filters_cut_the_scope() {
        let conn = db();
        let all = scope(&conn, &StatsFilter::default()).unwrap();
        assert_eq!(all.song_count, 2);
        let early = StatsFilter { year_to: Some(2020), ..Default::default() };
        assert_eq!(scope(&conn, &early).unwrap().songs[0].song_id, "001");
        let late = StatsFilter { year_from: Some(2021), ..Default::default() };
        assert_eq!(scope(&conn, &late).unwrap().songs[0].song_id, "002", "年份取 release_date 的前四位");
        let rock = StatsFilter { genres: vec!["rock".into()], ..Default::default() };
        assert_eq!(scope(&conn, &rock).unwrap().song_count, 1, "流派不分大小写");
        let ja = StatsFilter { languages: vec!["ja".into()], ..Default::default() };
        assert_eq!(scope(&conn, &ja).unwrap().song_count, 2);
        let distinct = StatsFilter { distinct_lines: true, ..Default::default() };
        assert_eq!(scope(&conn, &distinct).unwrap().line_count, 2, "副歌只算一遍");
    }

    #[test]
    fn unknown_years_are_left_out_once_a_year_bound_is_set() {
        let conn = db();
        conn.execute("UPDATE songs SET year='' WHERE id='001'", []).unwrap();
        let s = scope(&conn, &StatsFilter { year_to: Some(2030), ..Default::default() }).unwrap();
        assert_eq!(s.song_count, 1);
    }

    #[test]
    fn distinct_lines_changes_counts_but_not_the_vocabulary() {
        let conn = db();
        let all = stats::word_frequency_table(&conn, None, &StatsFilter::default(), 10).unwrap();
        let distinct = stats::word_frequency_table(&conn, None, &StatsFilter { distinct_lines: true, ..Default::default() }, 10).unwrap();
        let f = |rows: &[crate::WordFrequency], l: &str| rows.iter().find(|r| r.lemma == l).unwrap().freq;
        assert_eq!(f(&all, "駆ける"), 2);
        assert_eq!(f(&distinct, "駆ける"), 1);
        assert_eq!(all.len(), distinct.len());
    }

    #[test]
    fn a_saved_run_records_scope_tokenizers_and_method() {
        let conn = db();
        let r = run(&conn, &AnalysisSpec::CorpusReport, &StatsFilter::default(), "全库", "0.2.10", None).unwrap();
        assert_eq!(r.scope.song_count, 2);
        assert!(r.scope.songs.iter().all(|s| s.version_id.is_some()), "补登记过，每首都有版本");
        assert_eq!(r.tokenizers.unknown_lines, 3, "老库的行不知道是哪个分词器分的，照实记");
        assert!(r.method["ttr"].as_str().unwrap().contains("词次"));
        assert_eq!(r.result["songCount"], 2);
        assert_eq!(r.schema_version, crate::migrations::CURRENT);
        assert_eq!(list(&conn, 10).unwrap()[0].id, r.id);
    }

    #[test]
    fn rerunning_an_unchanged_library_gives_the_identical_result() {
        let conn = db();
        let first = run(&conn, &freq(), &StatsFilter::default(), "", "t", None).unwrap();
        let cmp = rerun(&conn, first.id, "t").unwrap();
        assert!(cmp.result_identical, "{:?}", cmp.reasons);
        assert!(cmp.same_scope && cmp.same_tokenizers && cmp.same_method);
        assert!(cmp.reasons.is_empty());
        assert!(cmp.words.is_empty());
        assert_eq!(cmp.after.rerun_of, Some(first.id));
    }

    #[test]
    fn a_rerun_explains_new_songs_and_replaced_lyrics() {
        let conn = db();
        let first = run(&conn, &AnalysisSpec::CorpusReport, &StatsFilter::default(), "", "t", None).unwrap();

        // 002 换一版歌词，再加一首 003
        let lines = [LineInput { time_sec: Some(1.0), text: "怪獣" }];
        let v = lineage::record_version(&conn, "002", &LyricSource::new("online", "netease", "", "user"), Some("[00:01]怪獣"), &lines).unwrap();
        conn.execute_batch(&format!(
            "DELETE FROM tokens WHERE utterance_id=3; DELETE FROM utterances WHERE id=3;
             INSERT INTO utterances (id, song_id, line_idx, time_sec, text, version_id) VALUES (4,'002',0,1.0,'怪獣',{});
             INSERT INTO tokens (utterance_id, token_idx, surface, lemma, pos) VALUES (4,0,'怪獣','怪獣','NOUN');
             INSERT INTO songs (id, title, artist) VALUES ('003','新曲','X');
             INSERT INTO utterances (id, song_id, line_idx, text) VALUES (5,'003',0,'光');
             INSERT INTO tokens (utterance_id, token_idx, surface, lemma, pos) VALUES (5,0,'光','光','NOUN');",
            v.id
        ))
        .unwrap();
        let run_id = lineage::ensure_tokenizer_run(&conn, &TokenizerRun { engine: "sudachi.rs".into(), ..Default::default() }).unwrap();
        conn.execute("UPDATE utterances SET tokenizer_run_id=?1 WHERE id IN (4,5)", [run_id]).unwrap();

        let cmp = rerun(&conn, first.id, "t").unwrap();
        assert!(!cmp.result_identical);
        assert_eq!(cmp.scope.added, ["003"]);
        assert_eq!(cmp.scope.lyrics_changed, ["002"]);
        assert!(!cmp.same_tokenizers);
        assert!(cmp.reasons.iter().any(|r| r.contains("换了歌词版本")), "{:?}", cmp.reasons);
        assert!(cmp.metrics.iter().any(|m| m.name == "songCount" && m.before == Some(2.0) && m.after == Some(3.0)));
    }

    #[test]
    fn a_token_correction_alone_is_named_as_the_likely_cause() {
        let conn = db();
        let first = run(&conn, &freq(), &StatsFilter::default(), "", "t", None).unwrap();
        crate::corrections::save(
            &conn,
            3,
            &[crate::TokenEdit { surface: "怪獣の夜".into(), lemma: "怪獣の夜".into(), pos: "NOUN".into() }],
        )
        .unwrap();
        let cmp = rerun(&conn, first.id, "t").unwrap();
        assert!(!cmp.result_identical);
        assert!(cmp.same_scope);
        assert!(cmp.reasons[0].contains("校正"), "{:?}", cmp.reasons);
        assert!(cmp.words.iter().any(|w| w.lemma == "怪獣の夜" && w.before_rank.is_none()));
    }

    #[test]
    fn subcorpora_are_saved_by_name_and_counted_live() {
        let conn = db();
        let f = StatsFilter { year_to: Some(2020), ..Default::default() };
        let s = save_subcorpus(&conn, " 2010 年代 ", &f, "").unwrap();
        assert_eq!(s.name, "2010 年代");
        assert_eq!(s.song_count, 1);
        let again = save_subcorpus(&conn, "2010 年代", &StatsFilter::default(), "全部").unwrap();
        assert_eq!(again.id, s.id, "同名覆盖");
        assert_eq!(again.song_count, 2);
        assert!(save_subcorpus(&conn, "  ", &f, "").is_err());
        assert!(delete_subcorpus(&conn, s.id).unwrap());
        assert!(subcorpora(&conn).unwrap().is_empty());
    }

    #[test]
    fn old_filters_without_the_new_fields_still_load() {
        let f: StatsFilter = serde_json::from_str(r#"{"performerIds":[1],"jpOnly":true}"#).unwrap();
        assert_eq!(f.performer_ids, [1]);
        assert!(f.year_from.is_none() && !f.distinct_lines);
    }

    #[test]
    fn kwic_and_word_examples_stay_inside_the_subcorpus() {
        let conn = db();
        // 「夜」在 001（2019）和 002（2023）里都有
        let query = |scope: Option<StatsFilter>| crate::KwicQuery {
            keywords: vec!["夜".into()],
            field: crate::MatchField::Lemma,
            dedup: false,
            scope,
            ..Default::default()
        };
        let all = crate::search::kwic(&conn, &query(None)).unwrap();
        assert_eq!(all.iter().map(|h| h.song_id.as_str()).collect::<BTreeSet<_>>().len(), 2);
        // distinct_lines 是口径，KWIC 不吃它（KWIC 有自己的 dedup）
        let late = StatsFilter { year_from: Some(2021), distinct_lines: true, ..Default::default() };
        let hits = crate::search::kwic(&conn, &query(Some(late.clone()))).unwrap();
        assert!(!hits.is_empty() && hits.iter().all(|h| h.song_id == "002"));

        let word = crate::corpus::word_in_corpus(&conn, "夜", None, 10, Some(&late)).unwrap();
        assert_eq!(word.song_count, 1);
        assert_eq!(word.occurrences, 1);
        assert!(word.examples.iter().all(|e| e.song_id == "002"));
        let whole = crate::corpus::word_in_corpus(&conn, "夜", Some("002"), 10, None).unwrap();
        assert_eq!(whole.occurrences, 3);
        assert_eq!(whole.examples[0].song_id, "002", "当前这首的例句排最前");
    }

    #[test]
    fn facets_list_years_genres_and_languages() {
        let conn = db();
        let f = facets(&conn).unwrap();
        assert_eq!((f.year_min, f.year_max), (Some(2019), Some(2023)));
        assert_eq!(f.genres.len(), 2);
        assert_eq!(f.languages, [("ja".to_string(), 2)]);
    }
}
