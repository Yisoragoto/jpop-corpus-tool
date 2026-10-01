//! 和 Python 侧 `scraper/normalize.py` 的逐字段对账。
//!
//! 基线由 `scripts/export_normalize_baseline.py` 从真实曲库 + 一批人造的
//! 版本标记 / feat 变体导出。真实数据里没有 Live / TV size / feat.，
//! 而刮削天天遇到——只拿真实曲名对账会漏掉整个版本判定逻辑。
//!
//! 为什么必须逐字段相同而不是「差不多」：迁移期两边共用一个 `corpus.db`，
//! 归一化键决定「是不是同一首歌」。算得不一样就会一边认为重复、
//! 另一边认为是新的，库会被写脏。

use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Expected {
    normalized: String,
    base: String,
    base_display: String,
    tags: Vec<String>,
    featured: Vec<String>,
    parts: Vec<String>,
}

fn baseline() -> Option<HashMap<String, HashMap<String, Expected>>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)?
        .join("tests/fixtures/normalize_baseline.json");
    // 文件不在就跳过（克隆仓库的人可能没有），但**文件在却读不动**
    // 必须报错——静默跳过的对账测试比没有测试更糟，它会一直显示绿色。
    let Ok(text) = std::fs::read_to_string(&path) else {
        return None;
    };
    Some(serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!("基线文件读得到但解析不了：{} — {e}", path.display())
    }))
}

/// 把差异全部收集起来一次报出来，而不是第一条就 panic——
/// 一次看到全部分歧才知道是「一类规则错了」还是「一个字符特殊」。
fn compare(kind: &str, f: impl Fn(&str) -> jp_normalize::NormalizedText) {
    let Some(data) = baseline() else {
        eprintln!("跳过：找不到 tests/fixtures/normalize_baseline.json");
        return;
    };
    let cases = data.get(kind).expect("基线里没有这一类");
    let mut diffs: Vec<String> = Vec::new();

    for (input, want) in cases {
        let got = f(input);
        let got_tags: Vec<String> = got.tags.iter().cloned().collect();
        let mut fields: Vec<String> = Vec::new();
        if got.normalized != want.normalized {
            fields.push(format!(
                "normalized py={:?} rs={:?}",
                want.normalized, got.normalized
            ));
        }
        if got.base != want.base {
            fields.push(format!("base py={:?} rs={:?}", want.base, got.base));
        }
        if got.base_display != want.base_display {
            fields.push(format!(
                "base_display py={:?} rs={:?}",
                want.base_display, got.base_display
            ));
        }
        if got_tags != want.tags {
            fields.push(format!("tags py={:?} rs={:?}", want.tags, got_tags));
        }
        if got.featured != want.featured {
            fields.push(format!(
                "featured py={:?} rs={:?}",
                want.featured, got.featured
            ));
        }
        if got.parts != want.parts {
            fields.push(format!("parts py={:?} rs={:?}", want.parts, got.parts));
        }
        if !fields.is_empty() {
            diffs.push(format!("  {input:?}\n    {}", fields.join("\n    ")));
        }
    }

    assert!(
        diffs.is_empty(),
        "{kind}: {} / {} 条和 Python 不一致\n{}",
        diffs.len(),
        cases.len(),
        diffs
            .iter()
            .take(15)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    println!("{kind}: {} 条全部一致", cases.len());
}

#[test]
fn titles_normalize_exactly_like_python() {
    compare("title", jp_normalize::normalize_title);
}

#[test]
fn artists_normalize_exactly_like_python() {
    compare("artist", jp_normalize::normalize_artist);
}

#[test]
fn albums_normalize_exactly_like_python() {
    compare("album", jp_normalize::normalize_album);
}
