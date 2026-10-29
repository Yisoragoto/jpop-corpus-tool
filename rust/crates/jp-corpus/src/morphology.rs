//! 用 tokens 的形态信息（20261029 起）做的两种分析：活用形分布、表记变体。
//!
//! ## 活用形
//!
//! 有活用的词（动词、形容词、助动词……，即 `ctype` 不为空）按活用形数：命令形、意志推量形、仮定形、
//! 連用形-促音便……歌词里命令形（「走れ」「信じて」之外的「行け」）、意志形（「行こう」）比书面语多，
//! 这一张表能直接看出来。活用形按 Sudachi（UniDic）的写法，`大类-细类`；`coarse` 是 `-` 前面那段。
//!
//! ## 表记变体
//!
//! 同一个词在歌词里常常故意换写法：君 / きみ / キミ、僕 / ボク、会う / 逢う。按 Sudachi 的正规化表记
//! （`normalized`）归组，组里按**辞书形的写法**（lemma）分开数。
//!
//! 组的键还带上活用型：Sudachi 把可能动词也正规化成原动词（行ける → 行く、言える → 言う），
//! 那是另一个词而不是另一种写法。两者活用型不同（下一段 / 五段），按活用型一分就开了。
//!
//! 正规化表记只合并了一部分：キミ → 君，但 きみ 还是 きみ。所以再加一条：**全是假名**的组，
//! 如果读音和词性都只对得上一个带汉字的组，就并过去。只对不活用的词这么做——活用词的读音是
//! 变形后的（「会い」读アイ），拿它比辞书形不可靠。
//!
//! 两种分析都只看标过形态信息的词（`pos_ja` 不为空），计数单位和其余分析一样可选。

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::analysis::{self, Provenance, Scope};

// ────────────────────────────── 活用形 ──────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InflectionQuery {
    pub scope: Scope,
    /// 只看这种 UPOS（VERB / ADJ / AUX），空是全部有活用的词
    pub pos: Option<String>,
    /// 每种活用形列几个例词
    pub examples: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InflectionForm {
    /// 活用形全名：`連用形-促音便`
    pub cform: String,
    /// 大类：`連用形`
    pub coarse: String,
    pub freq: i64,
    /// 占有活用的词的比例
    pub share: f64,
    pub songs: i64,
    /// 最常见的几个词（辞书形, 次数）
    pub lemmas: Vec<(String, i64)>,
    /// 最常见的几种实际写法（表层形, 次数）
    pub surfaces: Vec<(String, i64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InflectionResult {
    /// 有活用的词一共多少（分母）
    pub total: i64,
    /// 还没标形态信息的词数。不为 0 时界面提示「还在补」
    pub unannotated: i64,
    pub forms: Vec<InflectionForm>,
    pub provenance: Provenance,
}

fn top(map: HashMap<String, i64>, n: usize) -> Vec<(String, i64)> {
    let mut v: Vec<(String, i64)> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

fn unannotated(conn: &Connection, scope: &Scope) -> Result<i64> {
    let (clause, params) = scope.subcorpus.and_clause("u.song_id");
    let sql = format!(
        "SELECT COUNT(*) FROM tokens t JOIN utterances u ON u.id = t.utterance_id WHERE t.pos_ja IS NULL{clause}"
    );
    Ok(conn.query_row(&sql, rusqlite::params_from_iter(params), |r| r.get(0)).unwrap_or(0))
}

pub fn inflection(conn: &Connection, query: &InflectionQuery) -> Result<InflectionResult> {
    let lines = analysis::load_lines(conn, &query.scope)?;
    let pos = query.pos.as_deref().filter(|p| !p.is_empty());
    let examples = query.examples.unwrap_or(6);

    #[derive(Default)]
    struct Acc {
        freq: i64,
        songs: HashSet<String>,
        lemmas: HashMap<String, i64>,
        surfaces: HashMap<String, i64>,
    }
    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();
    let mut total = 0;
    for line in lines.iter().filter(|l| analysis::counted(l, query.scope.unit)) {
        for tok in &line.tokens {
            if tok.ctype.is_empty() || tok.cform.is_empty() || pos.is_some_and(|p| p != tok.pos) {
                continue;
            }
            total += 1;
            let a = acc.entry(tok.cform.clone()).or_default();
            a.freq += 1;
            a.songs.insert(line.song_id.clone());
            *a.lemmas.entry(tok.lemma.clone()).or_default() += 1;
            *a.surfaces.entry(tok.surface.clone()).or_default() += 1;
        }
    }
    let mut forms: Vec<InflectionForm> = acc
        .into_iter()
        .map(|(cform, a)| InflectionForm {
            coarse: cform.split('-').next().unwrap_or(&cform).to_string(),
            share: if total > 0 { a.freq as f64 / total as f64 } else { 0.0 },
            freq: a.freq,
            songs: a.songs.len() as i64,
            lemmas: top(a.lemmas, examples),
            surfaces: top(a.surfaces, examples),
            cform,
        })
        .collect();
    forms.sort_by(|a, b| b.freq.cmp(&a.freq).then_with(|| a.cform.cmp(&b.cform)));
    let provenance = analysis::provenance(conn, &query.scope, &lines, serde_json::to_value(query)?)?;
    Ok(InflectionResult { total, unannotated: unannotated(conn, &query.scope)?, forms, provenance })
}

// ────────────────────────────── 表记变体 ──────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OrthographyQuery {
    pub scope: Scope,
    /// 整组少于这个数的不列出
    pub min_freq: i64,
    /// 每种写法至少出现几次才算一种写法（去掉一两次的误写、误分）
    pub min_form_freq: i64,
    /// 只看实词：去掉助词、助动词、连词类（`analysis::FUNCTION_POS`）。
    /// 它们的「变体」是语法上的异形（て / で、た / だ），不是写法的选择
    pub content_only: bool,
    pub limit: Option<usize>,
}

impl Default for OrthographyQuery {
    fn default() -> Self {
        Self { scope: Scope::default(), min_freq: 5, min_form_freq: 1, content_only: true, limit: Some(200) }
    }
}

/// 写法用的是哪种文字
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Writing {
    /// 有汉字（含送假名）
    Kanji,
    Hiragana,
    Katakana,
    /// 假名混写（平假名 + 片假名）
    Kana,
    /// 拉丁字母等其他
    Other,
}

pub fn writing_of(text: &str) -> Writing {
    use crate::lexical::{Script, script_of};
    let (mut kanji, mut hira, mut kata, mut other) = (false, false, false, false);
    for c in text.chars() {
        match script_of(c) {
            Script::Kanji => kanji = true,
            Script::Hiragana => hira = true,
            Script::Katakana => kata = true,
            _ => other = true,
        }
    }
    match (kanji, hira, kata) {
        (true, _, _) => Writing::Kanji,
        (false, true, false) if !other => Writing::Hiragana,
        (false, false, true) if !other => Writing::Katakana,
        (false, true, true) if !other => Writing::Kana,
        _ => Writing::Other,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WritingForm {
    /// 辞书形的写法
    pub lemma: String,
    pub writing: Writing,
    pub freq: i64,
    pub songs: i64,
    pub example_utterance_id: i64,
    pub example_song_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariantGroup {
    /// 正规化表记（组名）
    pub normalized: String,
    pub reading: String,
    pub pos: String,
    pub freq: i64,
    /// 至少两种写法，按次数降序
    pub forms: Vec<WritingForm>,
    /// 这组是靠读音把假名写法并进来的（正规化表记本身没合并）
    pub merged_by_reading: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrthographyResult {
    pub groups: Vec<VariantGroup>,
    /// 一共几组有多种写法（截断前）
    pub total_groups: usize,
    pub unannotated: i64,
    pub provenance: Provenance,
}

pub fn orthography(conn: &Connection, query: &OrthographyQuery) -> Result<OrthographyResult> {
    let lines = analysis::load_lines(conn, &query.scope)?;

    #[derive(Default)]
    struct Form {
        freq: i64,
        songs: HashSet<String>,
        example: Option<(i64, String)>,
    }
    #[derive(Default)]
    struct Group {
        forms: HashMap<String, Form>,
        readings: HashMap<String, i64>,
        inflecting: bool,
    }
    // 键：（正规化表记，UPOS）。同一个写法当名词和当动词不是一回事。
    // 活用型拼在 UPOS 后面（`VERB\u{1f}下一段-カ行`）：行ける 不进 行く 那组；输出时再拆开
    let mut groups: HashMap<(String, String), Group> = HashMap::new();
    for line in lines.iter().filter(|l| analysis::counted(l, query.scope.unit)) {
        for tok in &line.tokens {
            if tok.normalized.is_empty() || (query.content_only && analysis::FUNCTION_POS.contains(&tok.pos.as_str())) {
                continue;
            }
            let key_pos = if tok.ctype.is_empty() { tok.pos.clone() } else { format!("{}\u{1f}{}", tok.pos, tok.ctype) };
            let g = groups.entry((tok.normalized.clone(), key_pos)).or_default();
            g.inflecting |= !tok.ctype.is_empty();
            *g.readings.entry(tok.reading.clone()).or_default() += 1;
            let f = g.forms.entry(tok.lemma.clone()).or_default();
            f.freq += 1;
            f.songs.insert(line.song_id.clone());
            if f.example.is_none() {
                f.example = Some((line.utterance_id, line.song_id.clone()));
            }
        }
    }
    let main_reading = |g: &Group| {
        g.readings.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))).map(|(r, _)| r.clone()).unwrap_or_default()
    };

    // 全假名、不活用的组：读音和词性只对得上唯一一个带汉字的组时并过去
    let mut kanji_by_reading: HashMap<(String, String), Vec<(String, String)>> = HashMap::new();
    for (key, g) in &groups {
        if !g.inflecting && writing_of(&key.0) == Writing::Kanji {
            kanji_by_reading.entry((main_reading(g), key.1.clone())).or_default().push(key.clone());
        }
    }
    let mut merged_into: HashSet<(String, String)> = HashSet::new();
    let kana_keys: Vec<(String, String)> = groups
        .iter()
        .filter(|(key, g)| !g.inflecting && matches!(writing_of(&key.0), Writing::Hiragana | Writing::Katakana | Writing::Kana))
        .map(|(key, _)| key.clone())
        .collect();
    for key in kana_keys {
        let reading = main_reading(&groups[&key]);
        let Some(targets) = kanji_by_reading.get(&(reading, key.1.clone())) else { continue };
        let [target] = targets.as_slice() else { continue };
        let Some(source) = groups.remove(&key) else { continue };
        let dest = groups.get_mut(target).expect("目标组还在");
        for (lemma, f) in source.forms {
            let d = dest.forms.entry(lemma).or_default();
            d.freq += f.freq;
            d.songs.extend(f.songs);
            if d.example.is_none() {
                d.example = f.example;
            }
        }
        for (r, n) in source.readings {
            *dest.readings.entry(r).or_default() += n;
        }
        merged_into.insert(target.clone());
    }

    let mut out: Vec<VariantGroup> = groups
        .into_iter()
        .filter_map(|((normalized, pos), g)| {
            let reading = main_reading(&g);
            let mut forms: Vec<WritingForm> = g
                .forms
                .into_iter()
                .filter(|(_, f)| f.freq >= query.min_form_freq.max(1))
                .map(|(lemma, f)| {
                    let (example_utterance_id, example_song_id) = f.example.unwrap_or_default();
                    WritingForm {
                        writing: writing_of(&lemma),
                        lemma,
                        freq: f.freq,
                        songs: f.songs.len() as i64,
                        example_utterance_id,
                        example_song_id,
                    }
                })
                .collect();
            if forms.len() < 2 {
                return None;
            }
            forms.sort_by(|a, b| b.freq.cmp(&a.freq).then_with(|| a.lemma.cmp(&b.lemma)));
            let freq: i64 = forms.iter().map(|f| f.freq).sum();
            if freq < query.min_freq.max(1) {
                return None;
            }
            let merged_by_reading = merged_into.contains(&(normalized.clone(), pos.clone()));
            let pos = pos.split('\u{1f}').next().unwrap_or_default().to_string();
            Some(VariantGroup { normalized, reading, pos, freq, forms, merged_by_reading })
        })
        .collect();
    out.sort_by(|a, b| b.freq.cmp(&a.freq).then_with(|| a.normalized.cmp(&b.normalized)).then_with(|| a.pos.cmp(&b.pos)));
    let total_groups = out.len();
    out.truncate(query.limit.unwrap_or(200));
    let provenance = analysis::provenance(conn, &query.scope, &lines, serde_json::to_value(query)?)?;
    Ok(OrthographyResult { groups: out, total_groups, unannotated: unannotated(conn, &query.scope)?, provenance })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writing_classes() {
        assert_eq!(writing_of("君"), Writing::Kanji);
        assert_eq!(writing_of("会う"), Writing::Kanji);
        assert_eq!(writing_of("きみ"), Writing::Hiragana);
        assert_eq!(writing_of("キミ"), Writing::Katakana);
        assert_eq!(writing_of("ボクら"), Writing::Kana);
        assert_eq!(writing_of("love"), Writing::Other);
    }

    fn annotated_corpus() -> crate::Corpus {
        let corpus = crate::fixture::corpus().unwrap();
        let conn = corpus.connection();
        // 合成库没有形态信息：按写法手工标几个（读音、正规化表记、活用），够两种分析用
        conn.execute_batch(
            "UPDATE tokens SET pos_ja = 'x', ctype = '', cform = '', reading = surface, normalized = lemma;
             UPDATE tokens SET ctype = '五段', cform = '連用形-一般' WHERE pos = 'VERB';",
        )
        .unwrap();
        corpus
    }

    #[test]
    fn inflection_counts_only_inflecting_words_and_shares_sum_to_one() {
        let corpus = annotated_corpus();
        let res = inflection(corpus.connection(), &InflectionQuery::default()).unwrap();
        let verbs: i64 = corpus
            .connection()
            .query_row("SELECT COUNT(*) FROM tokens WHERE pos = 'VERB'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(res.total, verbs);
        assert_eq!(res.unannotated, 0);
        let share: f64 = res.forms.iter().map(|f| f.share).sum();
        assert!((share - 1.0).abs() < 1e-9);
        assert!(res.forms.iter().all(|f| f.coarse == "連用形"));
    }

    #[test]
    fn kana_spellings_join_the_kanji_group_with_the_same_reading() {
        let corpus = annotated_corpus();
        let conn = corpus.connection();
        // 造一组：君（キミ）、キミ（正规化成君）、きみ（正规化不变，读音キミ）
        conn.execute_batch(
            "UPDATE tokens SET reading = 'キミ', normalized = '君', pos = 'PRON', lemma = '君', surface = '君', ctype = '', cform = ''
               WHERE id IN (SELECT id FROM tokens ORDER BY id LIMIT 3);
             UPDATE tokens SET reading = 'キミ', normalized = '君', pos = 'PRON', lemma = 'キミ', surface = 'キミ', ctype = '', cform = ''
               WHERE id IN (SELECT id FROM tokens ORDER BY id LIMIT 2 OFFSET 3);
             UPDATE tokens SET reading = 'キミ', normalized = 'きみ', pos = 'PRON', lemma = 'きみ', surface = 'きみ', ctype = '', cform = ''
               WHERE id IN (SELECT id FROM tokens ORDER BY id LIMIT 2 OFFSET 5);",
        )
        .unwrap();
        let res = orthography(conn, &OrthographyQuery { min_freq: 1, ..Default::default() }).unwrap();
        assert!(res.groups.iter().all(|g| !analysis::FUNCTION_POS.contains(&g.pos.as_str())));
        let kimi = res.groups.iter().find(|g| g.normalized == "君" && g.pos == "PRON").expect("君 这一组");
        let forms: Vec<(&str, i64)> = kimi.forms.iter().map(|f| (f.lemma.as_str(), f.freq)).collect();
        // 合成库里本来就有一个「君」，所以是 4；同样两次的按写法排
        assert_eq!(forms, vec![("君", 4), ("きみ", 2), ("キミ", 2)]);
        assert!(kimi.merged_by_reading);
        assert!(!res.groups.iter().any(|g| g.normalized == "きみ"), "きみ 并进去了，不再单列");
    }
}
