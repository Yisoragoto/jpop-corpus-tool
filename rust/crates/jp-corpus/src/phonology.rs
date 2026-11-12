//! 音系层的分析：元音分布、单元音词和单元音行。
//!
//! 读音怎么转成按发音的拍，规则和与 UniDic 的对账在 `jp_normalize::phonology`，这里重新导出。
//! 读音先看用户改过的（`reading_rules`，见 `ruby`），没有再用 Sudachi 的。

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

pub use jp_normalize::phonology::*;

use crate::analysis::{self, Provenance, Scope};

// ────────────────────────────── 分析：元音分布、单元音词 ──────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VowelQuery {
    pub scope: Scope,
    /// 单元音词至少几个带元音的拍（默认 2：一拍的词全是单元音，没意思）。ッ、ン 不算：なっ、何（ナン）只有一拍
    pub min_word_morae: usize,
    /// 单元音行至少几拍（默认 6，ッ、ン 也算）
    pub min_line_morae: usize,
    /// 每个元音最多列几个词、几行
    pub limit: usize,
}

impl Default for VowelQuery {
    fn default() -> Self {
        VowelQuery { scope: Scope::default(), min_word_morae: 2, min_line_morae: 6, limit: 300 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VowelShare {
    pub vowel: Vowel,
    /// 带这个元音的拍数（长音拍算前一拍的元音）
    pub morae: i64,
    /// 占所有带元音的拍的比例
    pub share: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnivocalicWord {
    /// 按发音的片假名
    pub kana: String,
    pub lemma: String,
    /// 最常见的几种写法（表层形, 次数）
    pub surfaces: Vec<(String, i64)>,
    pub morae: usize,
    pub freq: i64,
    pub songs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnivocalicLine {
    pub song_id: String,
    pub utterance_id: i64,
    pub text: String,
    pub kana: String,
    pub morae: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnivocalicGroup {
    pub vowel: Vowel,
    /// 不同的词有几个（按读音 + 原形算一个）
    pub distinct_words: i64,
    /// 这些词一共出现几次
    pub tokens: i64,
    /// 按次数排，最多 `limit` 个
    pub words: Vec<UnivocalicWord>,
    /// 整行只有这一个元音的行数
    pub line_count: i64,
    pub lines: Vec<UnivocalicLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VowelResult {
    /// 带元音的拍一共多少（比例的分母）
    pub vowel_morae: i64,
    /// 按 a i u e o 的顺序
    pub vowels: Vec<VowelShare>,
    pub sokuon: i64,
    pub hatsuon: i64,
    /// 长音拍（ー 和读成长音的エイ、オウ……）。已经算进了 `vowels`
    pub long: i64,
    /// 拆过的词数
    pub words: i64,
    /// 读音里有假名以外的字、拆不了的词数（多是英文），不算在上面
    pub skipped: i64,
    /// 拆不了的词里最常见的几个（表层形, 次数）
    pub skipped_examples: Vec<(String, i64)>,
    /// a i u e o 各一组
    pub univocalic: Vec<UnivocalicGroup>,
    pub provenance: Provenance,
}

pub fn vowels(conn: &Connection, query: &VowelQuery) -> Result<VowelResult> {
    let lines = analysis::load_lines(conn, &query.scope)?;

    #[derive(Default)]
    struct WordAcc {
        morae: usize,
        freq: i64,
        songs: HashSet<String>,
        surfaces: HashMap<String, i64>,
    }
    let mut counts: HashMap<Vowel, i64> = HashMap::new();
    let (mut sokuon, mut hatsuon, mut long_morae, mut words, mut skipped) = (0, 0, 0, 0, 0);
    let mut skipped_surfaces: HashMap<String, i64> = HashMap::new();
    let mut uni_words: HashMap<Vowel, HashMap<(String, String), WordAcc>> = HashMap::new();
    // (元音, 行) —— 行文本最后一起查
    let mut uni_lines: Vec<(Vowel, String, i64, String, usize)> = Vec::new();

    // 用户改过的读音（歌词里右键「改读音」）优先：本首歌的、全库的，按写法对上就用
    let mut rules: HashMap<String, HashMap<String, String>> = HashMap::new();
    for line in lines.iter().filter(|l| analysis::counted(l, query.scope.unit)) {
        if !rules.contains_key(&line.song_id) {
            rules.insert(line.song_id.clone(), crate::ruby::rules_for_song(conn, &line.song_id)?);
        }
        let song_rules = &rules[&line.song_id];
        let input: Vec<Word> = line
            .tokens
            .iter()
            .map(|t| Word {
                reading: song_rules.get(&t.surface).map_or(t.reading.as_str(), String::as_str),
                surface: &t.surface,
                pos: &t.pos,
                pos_ja: "",
                cform: &t.cform,
            })
            .collect();
        let pronounced = pronounce_words(&input);
        let mut line_ok = !pronounced.is_empty();
        let mut line_morae: Vec<&Mora> = Vec::new();
        for (tok, ms) in line.tokens.iter().zip(&pronounced) {
            let Some(ms) = ms else {
                skipped += 1;
                *skipped_surfaces.entry(tok.surface.clone()).or_default() += 1;
                line_ok = false;
                continue;
            };
            words += 1;
            for m in ms {
                match m.kind {
                    MoraKind::Sokuon => sokuon += 1,
                    MoraKind::Hatsuon => hatsuon += 1,
                    MoraKind::Long => long_morae += 1,
                    MoraKind::Plain => {}
                }
                if let Some(v) = m.vowel {
                    *counts.entry(v).or_default() += 1;
                }
            }
            line_morae.extend(ms.iter());
            if vowels_of(ms).len() >= query.min_word_morae.max(1)
                && let Some(v) = single_vowel(ms)
            {
                let acc = uni_words.entry(v).or_default().entry((kana_of(ms), tok.lemma.clone())).or_default();
                acc.morae = ms.len();
                acc.freq += 1;
                acc.songs.insert(line.song_id.clone());
                *acc.surfaces.entry(tok.surface.clone()).or_default() += 1;
            }
        }
        if line_ok
            && line_morae.len() >= query.min_line_morae.max(1)
            && let Some(v) = single_vowel_refs(&line_morae)
        {
            let kana: String = line_morae.iter().map(|m| m.kana.as_str()).collect();
            uni_lines.push((v, line.song_id.clone(), line.utterance_id, kana, line_morae.len()));
        }
    }

    let vowel_morae: i64 = counts.values().sum();
    let vowel_shares = Vowel::ALL
        .iter()
        .map(|&v| {
            let morae = counts.get(&v).copied().unwrap_or(0);
            VowelShare { vowel: v, morae, share: if vowel_morae > 0 { morae as f64 / vowel_morae as f64 } else { 0.0 } }
        })
        .collect();

    let mut texts: HashMap<i64, String> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT text FROM utterances WHERE id = ?1")?;
        for (_, _, id, _, _) in &uni_lines {
            if let Ok(t) = stmt.query_row([id], |r| r.get::<_, String>(0)) {
                texts.insert(*id, t);
            }
        }
    }

    let limit = query.limit.max(1);
    let univocalic = Vowel::ALL
        .iter()
        .map(|&v| {
            let accs = uni_words.remove(&v).unwrap_or_default();
            let distinct_words = accs.len() as i64;
            let tokens = accs.values().map(|a| a.freq).sum();
            let mut ws: Vec<UnivocalicWord> = accs
                .into_iter()
                .map(|((kana, lemma), a)| UnivocalicWord {
                    kana,
                    lemma,
                    morae: a.morae,
                    freq: a.freq,
                    songs: a.songs.len() as i64,
                    surfaces: top(a.surfaces, 3),
                })
                .collect();
            ws.sort_by(|a, b| b.freq.cmp(&a.freq).then_with(|| b.morae.cmp(&a.morae)).then_with(|| a.kana.cmp(&b.kana)));
            ws.truncate(limit);
            let mine: Vec<&(Vowel, String, i64, String, usize)> = uni_lines.iter().filter(|l| l.0 == v).collect();
            let line_count = mine.len() as i64;
            let mut ls: Vec<UnivocalicLine> = mine
                .into_iter()
                .map(|(_, song_id, id, kana, morae)| UnivocalicLine {
                    song_id: song_id.clone(),
                    utterance_id: *id,
                    text: texts.get(id).cloned().unwrap_or_default(),
                    kana: kana.clone(),
                    morae: *morae,
                })
                .collect();
            ls.sort_by(|a, b| b.morae.cmp(&a.morae).then_with(|| a.utterance_id.cmp(&b.utterance_id)));
            ls.truncate(limit);
            UnivocalicGroup { vowel: v, distinct_words, tokens, words: ws, line_count, lines: ls }
        })
        .collect();

    let provenance = analysis::provenance(conn, &query.scope, &lines, serde_json::to_value(query)?)?;
    Ok(VowelResult {
        vowel_morae,
        vowels: vowel_shares,
        sokuon,
        hatsuon,
        long: long_morae,
        words,
        skipped,
        skipped_examples: top(skipped_surfaces, 10),
        univocalic,
        provenance,
    })
}

fn single_vowel_refs(ms: &[&Mora]) -> Option<Vowel> {
    let mut vs = ms.iter().filter_map(|m| m.vowel);
    let first = vs.next()?;
    vs.all(|v| v == first).then_some(first)
}

fn top(map: HashMap<String, i64>, n: usize) -> Vec<(String, i64)> {
    let mut v: Vec<(String, i64)> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vowel_shares_sum_to_one_and_univocalic_words_have_one_vowel() {
        let corpus = crate::fixture::corpus().unwrap();
        // 合成库的词没有读音：给几个词填上，其余的算拆不了
        corpus
            .connection()
            .execute_batch(
                "UPDATE tokens SET reading = 'アナタ', surface = 'あなた', lemma = 'あなた', pos = 'PRON'
                   WHERE id IN (SELECT id FROM tokens ORDER BY id LIMIT 3);
                 UPDATE tokens SET reading = 'エイエン', surface = '永遠', lemma = '永遠', pos = 'NOUN'
                   WHERE id IN (SELECT id FROM tokens ORDER BY id LIMIT 2 OFFSET 3);
                 UPDATE tokens SET reading = 'ソラ', surface = '空', lemma = '空', pos = 'NOUN'
                   WHERE id IN (SELECT id FROM tokens ORDER BY id LIMIT 1 OFFSET 5);",
            )
            .unwrap();
        let r = vowels(corpus.connection(), &VowelQuery { min_line_morae: 1, ..VowelQuery::default() }).unwrap();
        assert_eq!(r.words, 6);
        assert!(r.skipped > 0);
        // あなた×3 + エーエン×2 + ソラ
        assert_eq!(r.vowel_morae, 9 + 6 + 2);
        let a = &r.univocalic[0];
        assert_eq!((a.vowel, a.distinct_words, a.tokens), (Vowel::A, 1, 3));
        assert_eq!(a.words[0].kana, "アナタ");
        assert_eq!(r.univocalic[3].words[0].kana, "エーエン");

        // 全库把「永遠」改读トワ：不再是单元音词
        crate::ruby::set_rule(corpus.connection(), "", "永遠", "とわ", true).unwrap();
        let r = vowels(corpus.connection(), &VowelQuery { min_line_morae: 1, ..VowelQuery::default() }).unwrap();
        assert!(r.univocalic[3].words.is_empty());
        assert_eq!(r.vowel_morae, 9 + 4 + 2);
        let sum: f64 = r.vowels.iter().map(|v| v.share).sum();
        assert!((sum - 1.0).abs() < 1e-9);
        assert_eq!(r.vowels.iter().map(|v| v.morae).sum::<i64>(), r.vowel_morae);
        assert_eq!(r.univocalic.len(), 5);
        for g in &r.univocalic {
            for w in &g.words {
                let ms = morae(&w.kana).unwrap();
                assert_eq!(single_vowel(&ms), Some(g.vowel), "{}", w.kana);
                assert!(vowels_of(&ms).len() >= 2);
            }
            assert!(g.lines.iter().all(|l| single_vowel(&morae(&l.kana).unwrap()) == Some(g.vowel)));
        }
    }
}
