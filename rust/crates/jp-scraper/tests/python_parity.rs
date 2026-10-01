//! 和 Python 侧 `scraper/matching.py` 的打分对账。
//!
//! 两个具体风险，都必须量出来而不是假定：
//!
//! 1. **Jaro-Winkler 的实现差异。** Python 用 rapidfuzz，Rust 用 strsim。
//!    两边都说自己实现的是 Jaro-Winkler，但前缀权重、前缀上限、是否对
//!    低相似度加 boost 这些细节各家不同，差一点就会让阈值附近的候选
//!    落到不同的档里。
//! 2. **舍入。** Python 的 `round()` 是平局取偶，Rust 的 `f64::round()`
//!    是远离零。分数存进库、又拿去和阈值比，差 1e-4 就可能改变判定。
//!
//! 基线由真实曲库 × 一批人造变体（Live / TV size / feat. / 精选集 /
//! 时长偏差 / 字段缺失）生成，覆盖各个分数区间。

use serde::Deserialize;

use jp_scraper::matching::string_similarity;
use jp_scraper::{MatchScorer, NormalizedTrack, ScorerConfig, ScrapeCandidate};

#[derive(Debug, Deserialize)]
struct Baseline {
    pairs: Vec<Pair>,
    /// [a, b, python_similarity]
    similarity: Vec<(String, String, f64)>,
}

#[derive(Debug, Deserialize)]
struct Pair {
    want: Want,
    duration_sec: Option<f64>,
    track_number: Option<i64>,
    cand: Cand,
    default: Expected,
    artwork: Expected,
}

#[derive(Debug, Deserialize)]
struct Want {
    title: String,
    artist: String,
    album: String,
}

#[derive(Debug, Deserialize)]
struct Cand {
    provider: String,
    title: String,
    artist: String,
    album: String,
    year: String,
    duration_sec: Option<f64>,
    track_number: Option<i64>,
    artwork_url: String,
    provider_confidence: f64,
}

#[derive(Debug, Deserialize)]
struct Expected {
    #[serde(rename = "final")]
    final_score: f64,
    weights: std::collections::BTreeMap<String, f64>,
    /// [reason, delta]
    penalties: Vec<(String, f64)>,
}

fn baseline() -> Option<Baseline> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)?
        .join("tests/fixtures/matching_baseline.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return None;
    };
    Some(
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("基线文件读得到但解析不了：{} — {e}", path.display())),
    )
}

fn to_candidate(c: &Cand) -> ScrapeCandidate {
    ScrapeCandidate {
        provider: c.provider.clone(),
        title: c.title.clone(),
        artist: c.artist.clone(),
        album: c.album.clone(),
        year: c.year.clone(),
        duration_sec: c.duration_sec,
        track_number: c.track_number,
        artwork_url: c.artwork_url.clone(),
        provider_confidence: c.provider_confidence,
        ..Default::default()
    }
}

/// strsim 和 rapidfuzz 算的是不是同一个数。
///
/// 这一条单独测，因为它是打分差异最可能的来源——分数对不上时
/// 先看这里，能立刻区分「相似度算法不同」和「打分逻辑写错了」。
#[test]
fn jaro_winkler_agrees_with_rapidfuzz() {
    let Some(data) = baseline() else {
        eprintln!("跳过：找不到基线");
        return;
    };
    let mut worst = 0.0f64;
    let mut diffs = Vec::new();
    for (a, b, py) in &data.similarity {
        let rs = string_similarity(a, b);
        let delta = (rs - py).abs();
        if delta > worst {
            worst = delta;
        }
        if delta > 1e-9 {
            diffs.push(format!("  {a:?} vs {b:?}: py={py} rs={rs} Δ={delta:.3e}"));
        }
    }
    println!(
        "相似度样本 {}，最大偏差 {worst:.3e}，不一致 {}",
        data.similarity.len(),
        diffs.len()
    );
    assert!(
        diffs.is_empty(),
        "strsim 和 rapidfuzz 算得不一样（{} / {}）：\n{}",
        diffs.len(),
        data.similarity.len(),
        diffs
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn compare(label: &str, config: ScorerConfig, pick: fn(&Pair) -> &Expected) {
    let Some(data) = baseline() else {
        eprintln!("跳过：找不到基线");
        return;
    };
    let scorer = MatchScorer::new(config);
    let mut diffs: Vec<String> = Vec::new();
    let mut worst = 0.0f64;

    for pair in &data.pairs {
        let want = NormalizedTrack::of(&pair.want.title, &pair.want.artist, &pair.want.album);
        let got = scorer.score_normalized(
            &want,
            &to_candidate(&pair.cand),
            pair.duration_sec,
            pair.track_number,
        );
        let expected = pick(pair);

        let delta = (got.final_score - expected.final_score).abs();
        if delta > worst {
            worst = delta;
        }
        let mut problems = Vec::new();
        if delta > 1e-9 {
            problems.push(format!(
                "final py={} rs={} Δ={delta:.3e}",
                expected.final_score, got.final_score
            ));
        }
        if got.weights != expected.weights {
            problems.push(format!(
                "weights py={:?} rs={:?}",
                expected.weights, got.weights
            ));
        }
        let got_pen: Vec<(String, f64)> = got
            .penalties
            .iter()
            .map(|p| (p.reason.clone(), p.delta))
            .collect();
        if got_pen.len() != expected.penalties.len()
            || got_pen
                .iter()
                .zip(&expected.penalties)
                .any(|(a, b)| a.0 != b.0 || (a.1 - b.1).abs() > 1e-9)
        {
            problems.push(format!(
                "penalties py={:?} rs={:?}",
                expected.penalties, got_pen
            ));
        }
        if !problems.is_empty() {
            diffs.push(format!(
                "  want={:?}/{:?} cand={:?}/{:?}\n    {}",
                pair.want.title,
                pair.want.artist,
                pair.cand.title,
                pair.cand.artist,
                problems.join("\n    ")
            ));
        }
    }

    println!(
        "{label}: {} 个配对，不一致 {}，final 最大偏差 {worst:.3e}",
        data.pairs.len(),
        diffs.len()
    );
    assert!(
        diffs.is_empty(),
        "{label}: {} / {} 个配对和 Python 不一致\n{}",
        diffs.len(),
        data.pairs.len(),
        diffs
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn default_scoring_matches_python() {
    compare("default", ScorerConfig::default(), |p| &p.default);
}

#[test]
fn artwork_scoring_matches_python() {
    compare("artwork", ScorerConfig::for_artwork(), |p| &p.artwork);
}

// ────────────────────────── 查询阶梯 ──────────────────────────

#[derive(Debug, Deserialize)]
struct QueryCase {
    input: QueryInput,
    queries: Vec<ExpectedQuery>,
}

#[derive(Debug, Deserialize)]
struct QueryInput {
    title: String,
    artist: String,
    album: String,
    duration_sec: Option<f64>,
    track_number: Option<i64>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ExpectedQuery {
    kind: String,
    title: String,
    artist: String,
    album: String,
    term: String,
}

/// 查询阶梯必须**逐条且同序**地和 Python 一致。
///
/// 顺序不是装饰：resolver 拿到足够好的结果就停，顺序变了就是
/// 「用哪个写法去搜」变了，能不能搜到、搜到哪一条都会跟着变。
#[test]
fn query_ladder_matches_python() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .join("tests/fixtures/query_baseline.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("跳过：找不到 {}", path.display());
        return;
    };
    let cases: Vec<QueryCase> = serde_json::from_str(&text).expect("基线解析失败");

    let builder = jp_scraper::QueryBuilder::default();
    let mut diffs = Vec::new();
    let mut total_queries = 0usize;

    for case in &cases {
        let track = jp_scraper::TrackFile {
            path: "D:/a/1.flac".into(),
            embedded_title: case.input.title.clone(),
            embedded_artist: case.input.artist.clone(),
            embedded_album: case.input.album.clone(),
            duration_sec: case.input.duration_sec,
            track_number: case.input.track_number,
            ..Default::default()
        };
        let got: Vec<ExpectedQuery> = builder
            .build(&track)
            .into_iter()
            .map(|q| ExpectedQuery {
                kind: q.kind.clone(),
                title: q.title.clone(),
                artist: q.artist.clone(),
                album: q.album.clone(),
                term: q.term(),
            })
            .collect();
        total_queries += got.len();
        if got != case.queries {
            diffs.push(format!(
                "  输入 {:?}/{:?}\n    py={:?}\n    rs={:?}",
                case.input.title,
                case.input.artist,
                case.queries
                    .iter()
                    .map(|q| (&q.kind, &q.term))
                    .collect::<Vec<_>>(),
                got.iter().map(|q| (&q.kind, &q.term)).collect::<Vec<_>>()
            ));
        }
    }

    println!(
        "查询阶梯：{} 个用例、{total_queries} 条查询，不一致 {}",
        cases.len(),
        diffs.len()
    );
    assert!(
        diffs.is_empty(),
        "{} / {} 个用例和 Python 不一致\n{}",
        diffs.len(),
        cases.len(),
        diffs.iter().take(8).cloned().collect::<Vec<_>>().join("\n")
    );
}
