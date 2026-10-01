//! 注音分配、音拍、音高：和前端同一份 Yomitan 基准对账。
//!
//! 基准由 `tools/dump-yomitan-japanese-util.mjs` 直接跑 Yomitan 源码生成
//! （Yomitan 自带用例 + 从真实词典抽的 1500 个词头）。

use jp_dict::furigana::{
    FuriganaSegment, distribute_furigana, distribute_furigana_inflected, downstep_positions, is_mora_pitch_high,
    kana_mora_count, kana_morae, pitch_category,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Segment {
    text: String,
    reading: String,
}

#[derive(Deserialize)]
struct Furigana {
    term: String,
    reading: String,
    expected: Vec<Segment>,
}

#[derive(Deserialize)]
struct Inflected {
    term: String,
    reading: String,
    source: String,
    expected: Vec<Segment>,
}

#[derive(Deserialize)]
struct Morae {
    text: String,
    morae: Vec<String>,
    count: usize,
}

#[derive(Deserialize)]
struct Pitch {
    index: usize,
    value: Value,
    high: bool,
}

#[derive(Deserialize)]
struct Downstep {
    value: String,
    positions: Vec<i64>,
}

#[derive(Deserialize)]
struct Category {
    text: String,
    value: Value,
    verb: bool,
    category: Option<String>,
}

#[derive(Deserialize)]
struct Fixture {
    furigana: Vec<Furigana>,
    inflected: Vec<Inflected>,
    morae: Vec<Morae>,
    pitch: Vec<Pitch>,
    downsteps: Vec<Downstep>,
    categories: Vec<Category>,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/yomitan-japanese-util.json")).unwrap()
}

fn same(got: &[FuriganaSegment], want: &[Segment]) -> bool {
    got.len() == want.len() && got.iter().zip(want).all(|(g, w)| g.text == w.text && g.reading == w.reading)
}

#[test]
fn furigana_distribution_matches_yomitan() {
    let f = fixture();
    assert!(f.furigana.len() > 1500 && f.inflected.len() > 1600);
    let failures: Vec<String> = f
        .furigana
        .iter()
        .filter(|c| !same(&distribute_furigana(&c.term, &c.reading), &c.expected))
        .map(|c| format!("{}【{}】", c.term, c.reading))
        .chain(
            f.inflected
                .iter()
                .filter(|c| !same(&distribute_furigana_inflected(&c.term, &c.reading, &c.source), &c.expected))
                .map(|c| format!("{}【{}】← {}", c.term, c.reading, c.source)),
        )
        .collect();
    assert!(failures.is_empty(), "{} 条不一致：{:?}", failures.len(), &failures[..failures.len().min(20)]);
}

#[test]
fn morae_and_pitch_helpers_match_yomitan() {
    let f = fixture();
    for c in &f.morae {
        assert_eq!(kana_morae(&c.text), c.morae, "{}", c.text);
        assert_eq!(kana_mora_count(&c.text), c.count, "{}", c.text);
    }
    for c in &f.pitch {
        assert_eq!(is_mora_pitch_high(c.index, &c.value), c.high, "{}@{}", c.value, c.index);
    }
    for c in &f.downsteps {
        assert_eq!(downstep_positions(&c.value), c.positions, "{}", c.value);
    }
    for c in &f.categories {
        assert_eq!(pitch_category(&c.text, &c.value, c.verb).map(str::to_owned), c.category, "{} {}", c.text, c.value);
    }
}
