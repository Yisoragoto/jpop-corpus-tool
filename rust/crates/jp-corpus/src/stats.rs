//! 统计页的词频表和语料统计报告。对应 Python 版 `gui.py` 的 `StatsWorker` 和 `CorpusReportWorker`。
//!
//! 不筛选时和 Python 逐项相同（`jp-corpus/examples/stats_report.rs` 对账）。两处刻意不同：
//!
//! - **按歌手筛选**走演唱署名（`track_credits` 的 performer），不是对 `songs.artist` 做 `LIKE '%/name/%'`。
//!   演唱署名本来就是由歌手串按 `/` 拆出来的，结果一样，但走得了索引。
//! - **「只看日文」**：Python 报告只把**词种**过滤了，词次、词性分布、覆盖率的分母还是全部词次，
//!   于是 TTR = 过滤后的词种 ÷ 未过滤的词次，覆盖率也偏低；统计表是先取前 300 再过滤，结果常常不到 300 条。
//!   这里在**词次**这一层就过滤，所有指标用同一批词，词频表先过滤再取前 N。

use rusqlite::{Connection, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::WordFrequency;

/// Python `_JP_RE`（`[ぁ-んァ-ヶｦ-ｿ` + 两段汉字`]`）：含平假名、片假名、半角片假名或汉字就算日文
pub fn is_japanese(lemma: &str) -> bool {
    lemma.chars().any(|c| {
        matches!(c, 'ぁ'..='ん' | 'ァ'..='ヶ' | 'ｦ'..='ｿ' | '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}')
    })
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StatsFilter {
    /// 只看这些人演唱的歌；空是全部
    pub performer_ids: Vec<i64>,
    pub jp_only: bool,
}

impl StatsFilter {
    /// `AND EXISTS (...)` 子句和它的参数（接在已经 JOIN 了 utterances u 的查询后面）
    fn clause(&self) -> (String, Vec<i64>) {
        if self.performer_ids.is_empty() {
            return (String::new(), Vec::new());
        }
        let marks = vec!["?"; self.performer_ids.len()].join(",");
        (
            format!(
                " AND EXISTS (SELECT 1 FROM track_credits c WHERE c.song_id = u.song_id \
                  AND c.role = 'performer' AND c.person_id IN ({marks}))"
            ),
            self.performer_ids.clone(),
        )
    }
}

/// 统计表的一行：词频加 JLPT 等级（`jlpt_cache` 里的，本地查；没有是空串）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrequencyRow {
    #[serde(flatten)]
    pub word: WordFrequency,
    pub jlpt: String,
}

/// 统计表：词元 × 词性的频次，带 JLPT。Python `StatsWorker`：排除 PUNCT、SYM，按频次降序
pub fn frequency_table(conn: &Connection, pos: Option<&str>, filter: &StatsFilter, limit: usize) -> anyhow::Result<Vec<FrequencyRow>> {
    let words = word_frequency_table(conn, pos, filter, limit)?;
    let has_cache = conn
        .query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name='jlpt_cache'", [], |_| Ok(()))
        .is_ok();
    let mut levels = std::collections::HashMap::new();
    if has_cache && !words.is_empty() {
        let marks = vec!["?"; words.len()].join(",");
        let mut stmt = conn.prepare(&format!("SELECT lemma, level FROM jlpt_cache WHERE lemma IN ({marks})"))?;
        let rows = stmt.query_map(params_from_iter(words.iter().map(|w| w.lemma.as_str())), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (lemma, level) = row?;
            levels.insert(lemma, level.trim_start_matches("JLPT-").to_uppercase());
        }
    }
    Ok(words
        .into_iter()
        .map(|word| {
            let jlpt = levels.get(&word.lemma).cloned().unwrap_or_default();
            FrequencyRow { word, jlpt }
        })
        .collect())
}

/// 统计表：词元 × 词性的频次。Python `StatsWorker`：排除 PUNCT、SYM，按频次降序
pub fn word_frequency_table(conn: &Connection, pos: Option<&str>, filter: &StatsFilter, limit: usize) -> anyhow::Result<Vec<WordFrequency>> {
    let (clause, ids) = filter.clause();
    let pos_clause = if pos.is_some() { " AND t.pos = ?" } else { "" };
    let sql = format!(
        "SELECT t.lemma, t.pos, COUNT(*) AS freq, COUNT(DISTINCT u.song_id), GROUP_CONCAT(DISTINCT t.surface) \
         FROM tokens t JOIN utterances u ON u.id = t.utterance_id \
         WHERE t.pos NOT IN ('PUNCT','SYM'){pos_clause}{clause} \
         GROUP BY t.lemma, t.pos ORDER BY freq DESC"
    );
    let mut values: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(p) = pos {
        values.push(p.to_owned().into());
    }
    values.extend(ids.into_iter().map(rusqlite::types::Value::from));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(values), |row| {
        let surfaces: Option<String> = row.get(4)?;
        Ok(WordFrequency {
            lemma: row.get(0)?,
            pos: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            freq: row.get(2)?,
            song_count: row.get(3)?,
            surfaces: surfaces.unwrap_or_default().split(',').filter(|s| !s.is_empty()).map(str::to_owned).collect(),
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        let row = row?;
        if filter.jp_only && !is_japanese(&row.lemma) {
            continue;
        }
        out.push(row);
        if out.len() >= limit {
            break;
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PosShare {
    pub pos: String,
    pub token_count: i64,
    pub token_pct: f64,
    pub type_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub n: usize,
    pub pct: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusReport {
    pub song_count: i64,
    pub utterance_count: i64,
    /// 实词次（排除 PUNCT / SYM / SPACE / X）
    pub token_count: i64,
    /// 不同词元数
    pub type_count: i64,
    pub ttr: f64,
    /// 标准化 TTR：词次 ≥ 5000 时每 1000 个一段，≥ 1000 时每 500 个一段，取完整段的平均；不够时是 None
    pub sttr: Option<f64>,
    pub sttr_chunk: Option<usize>,
    pub hapax_count: i64,
    pub hapax_ratio: f64,
    pub avg_types_per_song: f64,
    pub avg_tokens_per_line: f64,
    pub pos_dist: Vec<PosShare>,
    pub coverage: Vec<Coverage>,
    /// 高频词元前 20（词元, 次数）
    pub top_words: Vec<(String, i64)>,
}

const COVERAGE_TARGETS: [usize; 4] = [100, 500, 1000, 2000];
const EXCLUDED: &str = "t.pos NOT IN ('PUNCT','SYM','SPACE','X')";

/// CPython 3.12 起 `sum()` 对浮点数做 Neumaier 补偿求和；照做，最后一位也对得上
fn python_fsum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut sum = 0.0f64;
    let mut compensation = 0.0f64;
    for x in values {
        let t = sum + x;
        if sum.abs() >= x.abs() {
            compensation += (sum - t) + x;
        } else {
            compensation += (x - t) + sum;
        }
        sum = t;
    }
    sum + compensation
}

pub fn corpus_report(conn: &Connection, filter: &StatsFilter) -> anyhow::Result<CorpusReport> {
    let (clause, ids) = filter.clause();
    let values = || params_from_iter(ids.iter());

    let song_count: i64 = conn.query_row(
        &format!("SELECT COUNT(DISTINCT u.song_id) FROM utterances u JOIN songs s ON s.id = u.song_id WHERE 1=1{clause}"),
        values(),
        |r| r.get(0),
    )?;
    let utterance_count: i64 = conn.query_row(
        &format!("SELECT COUNT(u.id) FROM utterances u JOIN songs s ON s.id = u.song_id WHERE 1=1{clause}"),
        values(),
        |r| r.get(0),
    )?;

    // 词次序列：统计报告里所有指标都从这一批词算，「只看日文」在这里过滤
    let mut stmt = conn.prepare(&format!(
        "SELECT u.song_id, t.lemma, t.pos FROM tokens t JOIN utterances u ON u.id = t.utterance_id \
         JOIN songs s ON s.id = u.song_id WHERE {EXCLUDED}{clause} ORDER BY s.id, u.line_idx, t.token_idx"
    ))?;
    let tokens: Vec<(String, String, String)> = stmt
        .query_map(values(), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?.unwrap_or_default()))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|(_, lemma, _)| !filter.jp_only || is_japanese(lemma))
        .collect();
    let token_count = tokens.len() as i64;

    // 词性分布：词次和不同词元数，按词次降序（同数时按词性名，结果稳定）
    let mut pos_counts: std::collections::HashMap<&str, (i64, std::collections::HashSet<&str>)> = std::collections::HashMap::new();
    for (_, lemma, pos) in &tokens {
        let entry = pos_counts.entry(pos.as_str()).or_default();
        entry.0 += 1;
        entry.1.insert(lemma.as_str());
    }
    let mut pos_dist: Vec<PosShare> = pos_counts
        .into_iter()
        .map(|(pos, (count, lemmas))| PosShare {
            pos: pos.to_owned(),
            token_count: count,
            token_pct: if token_count > 0 { count as f64 / token_count as f64 * 100.0 } else { 0.0 },
            type_count: lemmas.len() as i64,
        })
        .collect();
    pos_dist.sort_by(|a, b| b.token_count.cmp(&a.token_count).then_with(|| a.pos.cmp(&b.pos)));

    // 词元频次，降序（同数时按词元，结果稳定；前 N 覆盖率不受同数排序影响）
    let mut freq: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    for (_, lemma, _) in &tokens {
        *freq.entry(lemma.as_str()).or_default() += 1;
    }
    let mut freq_rows: Vec<(&str, i64)> = freq.into_iter().collect();
    freq_rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let type_count = freq_rows.len() as i64;
    let ttr = if token_count > 0 { type_count as f64 / token_count as f64 } else { 0.0 };
    let hapax_count = freq_rows.iter().filter(|(_, f)| *f == 1).count() as i64;
    let hapax_ratio = if type_count > 0 { hapax_count as f64 / type_count as f64 } else { 0.0 };

    let mut coverage = Vec::new();
    let mut cumulative = 0i64;
    let mut target = 0;
    let pct = |cum: i64| if token_count > 0 { cum as f64 / token_count as f64 * 100.0 } else { 0.0 };
    for (i, (_, f)) in freq_rows.iter().enumerate() {
        cumulative += f;
        while target < COVERAGE_TARGETS.len() && i + 1 >= COVERAGE_TARGETS[target] {
            coverage.push(Coverage { n: COVERAGE_TARGETS[target], pct: pct(cumulative) });
            target += 1;
        }
    }
    while target < COVERAGE_TARGETS.len() {
        coverage.push(Coverage { n: COVERAGE_TARGETS[target], pct: pct(cumulative) });
        target += 1;
    }

    let sttr_chunk = if token_count >= 5000 {
        Some(1000)
    } else if token_count >= 1000 {
        Some(500)
    } else {
        None
    };
    let sttr = sttr_chunk.and_then(|size| {
        let complete: Vec<f64> = tokens
            .chunks(size)
            .filter(|c| c.len() == size)
            .map(|c| c.iter().map(|(_, lemma, _)| lemma.as_str()).collect::<std::collections::HashSet<_>>().len() as f64 / size as f64)
            .collect();
        (!complete.is_empty()).then(|| python_fsum(complete.iter().copied()) / complete.len() as f64)
    });

    // 每首歌的不同词元数的平均
    let mut per_song: std::collections::BTreeMap<&str, std::collections::HashSet<&str>> = std::collections::BTreeMap::new();
    for (song, lemma, _) in &tokens {
        per_song.entry(song.as_str()).or_default().insert(lemma.as_str());
    }
    let avg_types_per_song = if per_song.is_empty() {
        0.0
    } else {
        per_song.values().map(|s| s.len() as i64).sum::<i64>() as f64 / per_song.len() as f64
    };

    Ok(CorpusReport {
        song_count,
        utterance_count,
        token_count,
        type_count,
        ttr,
        sttr,
        sttr_chunk,
        hapax_count,
        hapax_ratio,
        avg_types_per_song,
        avg_tokens_per_line: if utterance_count > 0 { token_count as f64 / utterance_count as f64 } else { 0.0 },
        pos_dist,
        coverage,
        top_words: freq_rows.iter().take(20).map(|(l, f)| ((*l).to_owned(), *f)).collect(),
    })
}

/// Python `_POS_JA`：报告 TXT 里的「和名」一列
const POS_JA: [(&str, &str); 14] = [
    ("NOUN", "名詞"), ("VERB", "動詞"), ("ADJ", "形容詞"), ("ADV", "副詞"),
    ("AUX", "助動詞"), ("PRON", "代名詞"), ("PROPN", "固有名詞"),
    ("INTJ", "感動詞"), ("ADP", "助詞"), ("CCONJ", "接続詞"),
    ("NUM", "数詞"), ("PART", "接辞"), ("SCONJ", "従属接"), ("DET", "限定詞"),
];

/// Python 的 `f"{n:,}"`
fn grouped(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// 统计报告的 TXT，逐字照 Python `_export_report_txt`（Windows 上文本模式写出的 CRLF、无 BOM）。
///
/// 和 Python 版有两处**有意**不同：
/// - Python 的报告字典里没有 `artist_filter` / `jp_only`，导出的文件永远写「全歌手 / 日本語のみ：なし」，
///   这里写实际的筛选；
/// - 词次不够、没有 STTR 时 Python 写「STTR（None語片段）」，这里写「STTR」。
///
/// 小数位的舍入：Rust 的 `{:.N}` 和 Python 的 `:.Nf` 一样按二进制真值取最近、正好一半时取偶，
/// 拿 42.2 万个值（含整半的）对过，0 处不同。
pub fn report_text(report: &CorpusReport, performers: &[String], jp_only: bool) -> String {
    let filter_desc = if performers.is_empty() { "全歌手".to_owned() } else { performers.join("、") };
    let sttr = report.sttr.map_or_else(|| "—".to_owned(), |v| format!("{v:.4}"));
    let sttr_label = match report.sttr_chunk {
        Some(chunk) => format!("STTR（{chunk}語片段）  "),
        None => "STTR                ".to_owned(),
    };
    let rule = |n: usize| format!("  {}", "-".repeat(n));

    let mut lines = vec![
        "=".repeat(50),
        "  JPOP 語料庫 統計レポート".to_owned(),
        "=".repeat(50),
        format!("フィルター：{filter_desc}"),
        format!("日本語のみ：{}", if jp_only { "あり" } else { "なし" }),
        String::new(),
        "【基本統計】".to_owned(),
        format!("  曲数                : {}", grouped(report.song_count)),
        format!("  歌詞行数            : {}", grouped(report.utterance_count)),
        format!("  トークン数（実質語）: {}", grouped(report.token_count)),
        format!("  タイプ数（ユニーク）: {}", grouped(report.type_count)),
        String::new(),
        "【語彙多様性指標】".to_owned(),
        format!("  TTR                 : {:.4}  (Types/Tokens)", report.ttr),
        format!("  {sttr_label}: {sttr}  (標準化 TTR)"),
        format!(
            "  Hapax 比率          : {:.1}%  ({} 語が1回のみ出現)",
            report.hapax_ratio * 100.0,
            grouped(report.hapax_count)
        ),
        format!("  平均曲語彙量        : {:.0} Types/曲", report.avg_types_per_song),
        format!("  平均行語数          : {:.1} Tokens/行", report.avg_tokens_per_line),
        String::new(),
        "【品詞分布】".to_owned(),
        format!("  {:<10} {:<10} {:>8} {:>7} {:>7}", "品詞", "和名", "トークン", "比率", "タイプ"),
        rule(48),
    ];
    for row in &report.pos_dist {
        let pos_ja = POS_JA.iter().find(|(k, _)| *k == row.pos).map_or(row.pos.as_str(), |(_, v)| v);
        lines.push(format!(
            "  {:<10} {:<10} {:>8} {:>6.1}% {:>7}",
            row.pos,
            pos_ja,
            grouped(row.token_count),
            row.token_pct,
            grouped(row.type_count)
        ));
    }
    lines.push(String::new());
    lines.push("【Top-N 覆蓋率】".to_owned());
    for cov in &report.coverage {
        lines.push(format!("  Top {:>5} 語 : {:.1}%", grouped(cov.n as i64), cov.pct));
    }
    lines.push(String::new());
    lines.push("【高頻語元 Top 20】".to_owned());
    lines.push(format!("  {:>4}  {:<16} {:>6}", "順位", "lemma", "出現数"));
    lines.push(rule(32));
    for (i, (lemma, freq)) in report.top_words.iter().take(20).enumerate() {
        lines.push(format!("  {:>4}.  {:<16} {:>6}", i + 1, lemma, grouped(*freq)));
    }
    lines.push(String::new());
    lines.join("\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE songs (id TEXT PRIMARY KEY, title TEXT, artist TEXT);
             CREATE TABLE utterances (id INTEGER PRIMARY KEY, song_id TEXT, line_idx INTEGER, text TEXT);
             CREATE TABLE tokens (id INTEGER PRIMARY KEY, utterance_id INTEGER, token_idx INTEGER, surface TEXT, lemma TEXT, pos TEXT);
             CREATE TABLE track_credits (song_id TEXT, person_id INTEGER, role TEXT);
             INSERT INTO songs VALUES ('001','夜','A'), ('002','朝','B');
             INSERT INTO utterances VALUES (1,'001',0,'夜 が 来る yeah'), (2,'002',0,'朝 が 来る 。');
             INSERT INTO tokens (utterance_id, token_idx, surface, lemma, pos) VALUES
               (1,0,'夜','夜','NOUN'), (1,1,'が','が','ADP'), (1,2,'来る','来る','VERB'), (1,3,'yeah','yeah','X'),
               (2,0,'朝','朝','NOUN'), (2,1,'が','が','ADP'), (2,2,'来','来る','VERB'), (2,3,'。','。','PUNCT');
             INSERT INTO track_credits VALUES ('001', 1, 'performer'), ('002', 2, 'performer'), ('002', 1, 'composer');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn japanese_detection_matches_the_python_pattern() {
        assert!(is_japanese("夜") && is_japanese("ら") && is_japanese("ラブ") && is_japanese("ｱｲ"));
        assert!(!is_japanese("yeah") && !is_japanese("123") && !is_japanese("ー"));
    }

    #[test]
    fn frequency_table_filters_by_performer_and_japanese_before_the_limit() {
        let conn = db();
        let all = word_frequency_table(&conn, None, &StatsFilter::default(), 10).unwrap();
        assert_eq!(all[0].lemma, "が");
        assert!(all.iter().any(|w| w.lemma == "yeah"), "统计表只排除标点和符号");
        assert!(all.iter().all(|w| w.lemma != "。"));
        let kuru = all.iter().find(|w| w.lemma == "来る").unwrap();
        assert_eq!((kuru.freq, kuru.song_count), (2, 2));

        let jp = word_frequency_table(&conn, None, &StatsFilter { jp_only: true, ..Default::default() }, 10).unwrap();
        assert!(jp.iter().all(|w| w.lemma != "yeah"));

        // 作曲不算：002 里 1 号是作曲者，不是演唱者
        let a = word_frequency_table(&conn, None, &StatsFilter { performer_ids: vec![1], jp_only: false }, 10).unwrap();
        assert!(a.iter().all(|w| w.lemma != "朝"));
        assert_eq!(word_frequency_table(&conn, Some("NOUN"), &StatsFilter::default(), 1).unwrap().len(), 1);
    }

    #[test]
    fn report_counts_content_tokens_and_filters_japanese_consistently() {
        let conn = db();
        let report = corpus_report(&conn, &StatsFilter::default()).unwrap();
        assert_eq!((report.song_count, report.utterance_count), (2, 2));
        // 排除 PUNCT 和 X：6 个词次，4 个词元（夜 が 来る 朝）
        assert_eq!((report.token_count, report.type_count), (6, 4));
        assert_eq!(report.hapax_count, 2);
        assert_eq!(report.sttr, None);
        assert_eq!(report.coverage.iter().map(|c| c.n).collect::<Vec<_>>(), [100, 500, 1000, 2000]);
        assert!((report.coverage[0].pct - 100.0).abs() < 1e-9);
        assert_eq!(report.pos_dist.iter().map(|p| (p.pos.as_str(), p.token_count)).collect::<Vec<_>>(), [("ADP", 2), ("NOUN", 2), ("VERB", 2)]);
        assert!((report.avg_types_per_song - 3.0).abs() < 1e-9);

        let one = corpus_report(&conn, &StatsFilter { performer_ids: vec![2], jp_only: true }).unwrap();
        assert_eq!((one.song_count, one.token_count, one.type_count), (1, 3, 3));
    }

    #[test]
    fn report_text_follows_the_python_layout_and_writes_the_real_filter() {
        let report = CorpusReport {
            song_count: 1234,
            utterance_count: 56789,
            token_count: 1_234_567,
            type_count: 8901,
            ttr: 0.012_345,
            sttr: Some(0.5),
            sttr_chunk: Some(1000),
            hapax_count: 4321,
            hapax_ratio: 0.4855,
            avg_types_per_song: 150.5,
            avg_tokens_per_line: 12.25,
            pos_dist: vec![PosShare { pos: "NOUN".into(), token_count: 300_000, token_pct: 24.25, type_count: 5000 }],
            coverage: vec![Coverage { n: 100, pct: 45.25 }, Coverage { n: 1000, pct: 80.0 }],
            top_words: vec![("夜".into(), 1234)],
        };
        // 夹具是 gui.py 原样的 _export_report_txt 对同一份报告写出的文件（Windows 文本模式，CRLF、无 BOM）。
        // 里面有几处正好一半的舍入：150.5 → "150"、12.25 → "12.2"、24.25 → "24.2"、45.25 → "45.2"
        let python = include_str!("../testdata/report_synthetic_py.txt");
        assert_eq!(report_text(&report, &[], false), python);

        let filtered = report_text(&report, &["ヨルシカ".into(), "YOASOBI".into()], true);
        assert_eq!(
            filtered,
            python.replacen("フィルター：全歌手\r\n日本語のみ：なし", "フィルター：ヨルシカ、YOASOBI\r\n日本語のみ：あり", 1)
        );

        let none = CorpusReport { sttr: None, sttr_chunk: None, ..report };
        assert!(report_text(&none, &[], false).contains("\r\n  STTR                : —  (標準化 TTR)\r\n"));
    }

    #[test]
    fn fsum_matches_python_compensated_sum() {
        // Python 3.12: sum([0.1] * 10) == 1.0，朴素累加是 0.9999999999999999
        assert_eq!(python_fsum(vec![0.1; 10]), 1.0);
        assert_eq!(python_fsum([1e16, 1.0, -1e16]), 1.0);
    }
}
