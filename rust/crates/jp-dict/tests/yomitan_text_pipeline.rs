//! 截断 + 逐字缩短 + 预处理变体 + 活用还原，整条管线和 Yomitan 逐行对账。
//!
//! 基准 `fixtures/japanese-text-pipeline.json` 由 `tools/dump-yomitan-text-processing.mjs`
//! 用 Node 直接跑 Yomitan 源码生成，覆盖半角假名、罗马字、合成浊点、兼容字符、部首、
//! 强调符号、长音、全角字母数字等特殊输入。

use jp_dict::deinflect::algorithm_deinflections;
use jp_dict::text::japanese_chinese_korean_only_prefix;
use jp_dict::transformer::LanguageTransformer;

#[test]
fn the_whole_preprocessing_pipeline_matches_yomitan_line_by_line() {
    let cases: Vec<(String, String, Vec<String>)> =
        serde_json::from_str(include_str!("fixtures/japanese-text-pipeline.json")).unwrap();
    assert_eq!(cases.len(), 57, "fixture 条数变了：重新导出过吗？");
    let lt = LanguageTransformer::japanese();

    let mut failures = Vec::new();
    for (input, cjk_only, want) in &cases {
        let prefix = japanese_chinese_korean_only_prefix(input);
        if prefix != cjk_only {
            failures.push(format!("{input:?} 截断: Yomitan {cjk_only:?} / Rust {prefix:?}"));
        }
        let got: Vec<String> = algorithm_deinflections(&lt, input).iter().map(|d| d.parity_line()).collect();
        if &got != want {
            let first_diff = got
                .iter()
                .zip(want)
                .position(|(g, w)| g != w)
                .unwrap_or(got.len().min(want.len()));
            failures.push(format!(
                "{input:?}: Yomitan {} 行 / Rust {} 行，第 {first_diff} 行起不同\n  Yomitan: {:?}\n  Rust:    {:?}",
                want.len(),
                got.len(),
                want.get(first_diff),
                got.get(first_diff)
            ));
        }
    }
    assert!(failures.is_empty(), "{} 条不一致：\n{}", failures.len(), failures.join("\n"));
}
