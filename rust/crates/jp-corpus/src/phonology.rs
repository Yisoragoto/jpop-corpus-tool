//! 音系层的分析：元音分布、单元音词和单元音行，押韵检索和相邻行押韵。
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

    let mut rules = SongRules::default();
    for line in lines.iter().filter(|l| analysis::counted(l, query.scope.unit)) {
        let pronounced = rules.pronounce(conn, line)?;
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

/// 用户改过的读音（歌词里右键「改读音」，见 `ruby`）优先：本首歌的、全库的，按写法对上就用。
/// 按歌缓存
#[derive(Default)]
struct SongRules(HashMap<String, HashMap<String, String>>);

impl SongRules {
    /// 一行里每个词按发音拆成的拍；读音里有假名以外的字的词是 None
    fn pronounce(&mut self, conn: &Connection, line: &analysis::Line) -> Result<Vec<Option<Vec<Mora>>>> {
        if !self.0.contains_key(&line.song_id) {
            self.0.insert(line.song_id.clone(), crate::ruby::rules_for_song(conn, &line.song_id)?);
        }
        let song_rules = &self.0[&line.song_id];
        let input: Vec<Word> = line
            .tokens
            .iter()
            .map(|t| Word {
                reading: song_rules.get(&t.surface).map_or(t.reading.as_str(), String::as_str),
                surface: &t.surface,
                pos: &t.pos,
                pos_ja: &t.pos_ja,
                cform: &t.cform,
            })
            .collect();
        Ok(pronounce_words(&input))
    }
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

// ────────────────────────────── 押韵：按元音检索、相邻行押韵 ──────────────────────────────
//
// 日语歌词押韵看的是元音（「母音踏み」）：東京 トーキョー 和 どうよ ドーヨ 押 o-o-o 的韵。
// 所以一行先按发音拆成拍，每拍只留一个「音」：元音（长音拍算前一拍的元音）、ン（n）、ッ（q）。
// 默认只比元音，ン、ッ 跳过（ナンダ 和 ナダ 都是 a-a）；勾「ン、ッ 也要对上」才算进去。
//
// 读音拆不了的词（英文）是断点：韵不跨过它，行尾 / 行首是英文的行没有韵脚 / 韵头。

/// 押韵比较用的一个音
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sound {
    V(Vowel),
    /// ン
    N,
    /// ッ
    Q,
}

impl Sound {
    fn of(m: &Mora) -> Option<Sound> {
        match m.kind {
            MoraKind::Sokuon => Some(Sound::Q),
            MoraKind::Hatsuon => Some(Sound::N),
            MoraKind::Plain | MoraKind::Long => m.vowel.map(Sound::V),
        }
    }

    /// 显示用：a i u e o，ン 是 n，ッ 是 q
    pub fn symbol(self) -> char {
        match self {
            Sound::V(v) => v.romaji(),
            Sound::N => 'n',
            Sound::Q => 'q',
        }
    }

    fn kept(self, strict: bool) -> bool {
        strict || matches!(self, Sound::V(_))
    }
}

/// 模式里的一位：一个音、两个音之一，或者任意一个
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Sound(Sound),
    /// 假名模式里光看假名定不了读法的那一拍：おもう 的 う，动词词尾读 u（思う），别的读长音 o（東京）
    Either(Sound, Sound),
    Any,
}

impl Slot {
    /// 显示用：一个音是一个字母，两者之一写成 `(ou)`，任意一个是 `.`
    fn write(self, out: &mut String) {
        match self {
            Slot::Sound(s) => out.push(s.symbol()),
            Slot::Either(a, b) => {
                out.push('(');
                out.push(a.symbol());
                out.push(b.symbol());
                out.push(')');
            }
            Slot::Any => out.push('.'),
        }
    }

    fn matches(self, s: Sound) -> bool {
        match self {
            Slot::Sound(x) => x == s,
            Slot::Either(a, b) => a == s || b == s,
            Slot::Any => true,
        }
    }
}

fn show_slots(pat: &[Slot]) -> String {
    let mut out = String::new();
    for s in pat {
        s.write(&mut out);
    }
    out
}

fn is_kana(c: char) -> bool {
    matches!(c, '\u{3041}'..='\u{3096}' | '\u{30A1}'..='\u{30FA}' | '\u{30FC}')
}

fn to_hiragana(s: &str) -> String {
    s.chars().map(|c| if ('\u{30A1}'..='\u{30F6}').contains(&c) { char::from_u32(c as u32 - 0x60).unwrap_or(c) } else { c }).collect()
}

/// 一串假名按发音转成音。光看假名不知道是不是动词：按非动词（オウ 读长音）和动词（词尾的ウ照读）
/// 各读一遍，两种读法不同的那一拍两者都算。写法也当表层形传进去，「いう」才会读ユー
fn flush_kana(buf: &mut String, out: &mut Vec<Slot>) {
    if buf.is_empty() {
        return;
    }
    let surface = to_hiragana(buf);
    let read = |pos: &str| -> Option<Vec<Option<Sound>>> {
        pronounce(Word { reading: buf, surface: &surface, pos, ..Word::default() }).map(|ms| ms.iter().map(Sound::of).collect())
    };
    match (read(""), read("VERB")) {
        (Some(a), Some(b)) if a.len() == b.len() => {
            for (x, y) in a.into_iter().zip(b) {
                match (x, y) {
                    (Some(x), Some(y)) if x != y => out.push(Slot::Either(x, y)),
                    (Some(x), _) | (None, Some(x)) => out.push(Slot::Sound(x)),
                    (None, None) => {}
                }
            }
        }
        (Some(a), _) => out.extend(a.into_iter().flatten().map(Slot::Sound)),
        _ => {}
    }
    buf.clear();
}

/// 输入的模式 → 一串音。
///
/// - 假名按发音转：えいえん → e e e（ン 默认跳过），ナイフ → a i u，いう → u u；词尾的 オ段 + う
///   两种读法都算（おもう → o o (ou)，思う 的 o o u 和 東京 的 o o o 都对得上）；
/// - 拉丁字母：a i u e o 是元音，n 是ン，q 是ッ；全角也行；
/// - `.`、`?`、`*`（以及全角）是任意一个音；
/// - 别的字（空格、-、辅音字母）跳过。所以 `kimi` 就是 i i。
///
/// 不比 ン、ッ（`strict` 为假）时，模式里的 n、q 也去掉。
pub fn parse_pattern(input: &str, strict: bool) -> Vec<Slot> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for c in input.chars() {
        if is_kana(c) {
            buf.push(c);
            continue;
        }
        flush_kana(&mut buf, &mut out);
        // 全角 ASCII → 半角
        let c = match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        };
        match c.to_ascii_lowercase() {
            'a' => out.push(Slot::Sound(Sound::V(Vowel::A))),
            'i' => out.push(Slot::Sound(Sound::V(Vowel::I))),
            'u' => out.push(Slot::Sound(Sound::V(Vowel::U))),
            'e' => out.push(Slot::Sound(Sound::V(Vowel::E))),
            'o' => out.push(Slot::Sound(Sound::V(Vowel::O))),
            'n' => out.push(Slot::Sound(Sound::N)),
            'q' => out.push(Slot::Sound(Sound::Q)),
            '.' | '?' | '*' => out.push(Slot::Any),
            _ => {}
        }
    }
    flush_kana(&mut buf, &mut out);
    out.retain(|s| match s {
        Slot::Sound(x) => x.kept(strict),
        Slot::Either(a, b) => a.kept(strict) && b.kept(strict),
        Slot::Any => true,
    });
    out
}

/// 一行拆成音：按断点（拆不了的词）分成几段，每个音记下是第几拍、第几个词
struct LineSounds {
    /// 这一行全部的拍（拆不了的词没有拍）
    morae: Vec<Mora>,
    /// 每段：(音, 在 `morae` 里的下标, 第几个词)。第一段从行首开始、最后一段到行尾为止；
    /// 行首 / 行尾是拆不了的词时，那一头的段是空的
    segments: Vec<Vec<(Sound, usize, usize)>>,
    /// 整行按发音的片假名，拆不了的词照原样写
    kana: String,
}

fn line_sounds(line: &analysis::Line, pronounced: Vec<Option<Vec<Mora>>>, strict: bool) -> LineSounds {
    let mut morae = Vec::new();
    let mut segments = vec![Vec::new()];
    let mut kana = String::new();
    for (ti, (tok, ms)) in line.tokens.iter().zip(pronounced).enumerate() {
        match ms {
            None => {
                kana.push_str(&tok.surface);
                segments.push(Vec::new());
            }
            Some(ms) => {
                for m in ms {
                    if let Some(s) = Sound::of(&m).filter(|s| s.kept(strict)) {
                        segments.last_mut().expect("至少一段").push((s, morae.len(), ti));
                    }
                    kana.push_str(&m.kana);
                    morae.push(m);
                }
            }
        }
    }
    LineSounds { morae, segments, kana }
}

/// 检索的位置
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RhymePosition {
    /// 行尾（脚韵）
    #[default]
    End,
    /// 行首（头韵）
    Start,
    /// 行里任何地方
    Any,
}

/// 在一段音里找模式，返回段内的下标范围
fn find_in(seg: &[(Sound, usize, usize)], pat: &[Slot], position: RhymePosition) -> Option<(usize, usize)> {
    let n = pat.len();
    if n == 0 || seg.len() < n {
        return None;
    }
    let at = |i: usize| seg[i..i + n].iter().zip(pat).all(|((s, _, _), p)| p.matches(*s));
    match position {
        RhymePosition::End => at(seg.len() - n).then_some((seg.len() - n, seg.len())),
        RhymePosition::Start => at(0).then_some((0, n)),
        RhymePosition::Any => (0..=seg.len() - n).find(|&i| at(i)).map(|i| (i, i + n)),
    }
}

/// 每个词在行文本里的字符范围（Unicode 标量下标，和 `word_timing` 一样）。按顺序往后找表层形，
/// 找不到的是 None
fn token_chars(text: &str, tokens: &[analysis::Tok]) -> Vec<Option<(usize, usize)>> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut cursor = 0;
    for t in tokens {
        match (!t.surface.is_empty()).then(|| text[cursor..].find(&t.surface)).flatten() {
            Some(off) => {
                let b = cursor + off;
                let start = text[..b].chars().count();
                out.push(Some((start, start + t.surface.chars().count())));
                cursor = b + t.surface.len();
            }
            None => out.push(None),
        }
    }
    out
}

fn surfaces(line: &analysis::Line, first: usize, last: usize) -> String {
    line.tokens[first..=last].iter().map(|t| t.surface.as_str()).collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RhymeQuery {
    pub scope: Scope,
    /// 元音模式或假名，见 [`parse_pattern`]
    pub pattern: String,
    pub position: RhymePosition,
    /// ン、ッ 也要对上
    pub strict: bool,
    /// 最多列几行
    pub limit: usize,
}

impl Default for RhymeQuery {
    fn default() -> Self {
        RhymeQuery { scope: Scope::default(), pattern: String::new(), position: RhymePosition::End, strict: false, limit: 200 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RhymeHit {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub utterance_id: i64,
    /// 这一行的时间（没有逐行时间是 None）
    pub time_sec: Option<f64>,
    pub text: String,
    /// 整行按发音的片假名
    pub kana: String,
    /// 对上模式的那几拍
    pub matched: String,
    /// 对上的拍落在哪几个词上，这几个词在 `text` 里的字符范围 `[start, end)`。对不上原文时是 None
    pub char_start: Option<usize>,
    pub char_end: Option<usize>,
}

/// 押这个韵的词：对上的拍落在的那几个词（表层形连起来）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RhymeCarrier {
    pub text: String,
    pub kana: String,
    /// 几行
    pub lines: i64,
    pub songs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RhymeResult {
    /// 解析后的模式（a i u e o n q，`.` 是任意一个）。空串表示输入里没有能比的音
    pub pattern: String,
    /// 对上的行数、曲数
    pub lines: i64,
    pub songs: i64,
    /// 按歌、行的顺序，最多 `limit` 行
    pub hits: Vec<RhymeHit>,
    /// 按行数排，最多 50 个
    pub carriers: Vec<RhymeCarrier>,
    pub provenance: Provenance,
}

/// 按元音模式找行：行尾押这个韵、行首有这个头韵，或者行里任何地方出现
pub fn rhyme(conn: &Connection, query: &RhymeQuery) -> Result<RhymeResult> {
    let pat = parse_pattern(&query.pattern, query.strict);
    let lines = load_with_barriers(conn, &query.scope)?;
    let mut rules = SongRules::default();
    let mut hits: Vec<RhymeHit> = Vec::new();
    let mut count = 0i64;
    let mut songs: HashSet<&str> = HashSet::new();
    #[derive(Default)]
    struct CarrierAcc {
        kana: String,
        lines: i64,
        songs: HashSet<String>,
    }
    let mut carriers: HashMap<String, CarrierAcc> = HashMap::new();
    let limit = query.limit.max(1);

    if !pat.is_empty() {
        for line in lines.iter().filter(|l| analysis::counted(l, query.scope.unit)) {
            let ls = line_sounds(line, pronounce_for_rhyme(&mut rules, conn, line, query.scope.jp_only)?, query.strict);
            let segs: Vec<&Vec<(Sound, usize, usize)>> = match query.position {
                RhymePosition::End => ls.segments.last().into_iter().collect(),
                RhymePosition::Start => ls.segments.first().into_iter().collect(),
                RhymePosition::Any => ls.segments.iter().collect(),
            };
            let Some((seg, (a, b))) = segs.into_iter().find_map(|seg| find_in(seg, &pat, query.position).map(|r| (seg, r))) else {
                continue;
            };
            let (m0, t0) = (seg[a].1, seg[a].2);
            let (m1, t1) = (seg[b - 1].1, seg[b - 1].2);
            let matched: String = ls.morae[m0..=m1].iter().map(|m| m.kana.as_str()).collect();
            count += 1;
            songs.insert(&line.song_id);
            let carrier = surfaces(line, t0, t1);
            let acc = carriers.entry(carrier).or_default();
            if acc.kana.is_empty() {
                acc.kana = matched.clone();
            }
            acc.lines += 1;
            acc.songs.insert(line.song_id.clone());
            if hits.len() < limit {
                let spans = token_chars(&line.text, &line.tokens);
                let (char_start, char_end) = match (spans[t0], spans[t1]) {
                    (Some((s, _)), Some((_, e))) => (Some(s), Some(e)),
                    _ => (None, None),
                };
                hits.push(RhymeHit {
                    song_id: line.song_id.clone(),
                    title: String::new(),
                    artist: String::new(),
                    utterance_id: line.utterance_id,
                    time_sec: None,
                    text: line.text.clone(),
                    kana: ls.kana,
                    matched,
                    char_start,
                    char_end,
                });
            }
        }
    }
    fill_hit_songs(conn, &mut hits)?;

    let mut carriers: Vec<RhymeCarrier> = carriers
        .into_iter()
        .map(|(text, a)| RhymeCarrier { text, kana: a.kana, lines: a.lines, songs: a.songs.len() as i64 })
        .collect();
    carriers.sort_by(|a, b| b.lines.cmp(&a.lines).then_with(|| b.songs.cmp(&a.songs)).then_with(|| a.text.cmp(&b.text)));
    carriers.truncate(50);

    let provenance = analysis::provenance(conn, &query.scope, &lines, serde_json::to_value(query)?)?;
    Ok(RhymeResult {
        pattern: show_slots(&pat),
        lines: count,
        songs: songs.len() as i64,
        hits,
        carriers,
        provenance,
    })
}

/// 押韵要的行：「仅日文」不去掉英文词，而是把它当断点（见 [`pronounce_for_rhyme`]）。
/// 去掉了它，行尾是英文的行就凭空多出一个日文韵脚
fn load_with_barriers(conn: &Connection, scope: &Scope) -> Result<Vec<analysis::Line>> {
    analysis::load_lines(conn, &Scope { jp_only: false, ..scope.clone() })
}

/// 一行按发音拆拍；勾了「仅日文」时，非日文的词（Sudachi 有时给 darling 注ダーリン）不出音、当断点
fn pronounce_for_rhyme(
    rules: &mut SongRules,
    conn: &Connection,
    line: &analysis::Line,
    jp_only: bool,
) -> Result<Vec<Option<Vec<Mora>>>> {
    let mut out = rules.pronounce(conn, line)?;
    if jp_only {
        for (tok, ms) in line.tokens.iter().zip(out.iter_mut()) {
            if !crate::stats::is_japanese(&tok.lemma) {
                *ms = None;
            }
        }
    }
    Ok(out)
}

fn fill_hit_songs(conn: &Connection, hits: &mut [RhymeHit]) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT u.time_sec, COALESCE(s.title,''), COALESCE(s.artist,'') \
         FROM utterances u JOIN songs s ON s.id = u.song_id WHERE u.id = ?1",
    )?;
    for h in hits {
        if let Ok((t, title, artist)) =
            stmt.query_row([h.utterance_id], |r| Ok((r.get::<_, Option<f64>>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
        {
            h.time_sec = t;
            h.title = title;
            h.artist = artist;
        }
    }
    Ok(())
}

// ── 相邻行押韵 ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RhymeStatsQuery {
    pub scope: Scope,
    /// 比行尾（脚韵）还是行首（头韵）。`Any` 按行尾算
    pub position: RhymePosition,
    pub strict: bool,
    /// 韵脚至少几个音（默认 3）
    pub tail: usize,
    /// 歌的排行里，相邻行对至少几对才列
    pub min_pairs: usize,
    pub limit: usize,
}

impl Default for RhymeStatsQuery {
    fn default() -> Self {
        RhymeStatsQuery { scope: Scope::default(), position: RhymePosition::End, strict: false, tail: 3, min_pairs: 8, limit: 50 }
    }
}

/// 韵脚取 k 个音时，相邻行押韵的比例和同一首歌里随便两行押韵的比例
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RhymeLength {
    pub length: usize,
    /// 能比的相邻行对（两行都至少 k 个音、文本不同）
    pub adjacent: i64,
    pub adjacent_rhymed: i64,
    pub adjacent_rate: f64,
    /// 同一首歌里任意两行（文本不同）
    pub baseline: i64,
    pub baseline_rhymed: i64,
    pub baseline_rate: f64,
    /// 相邻行押韵率 ÷ 随便两行押韵率。> 1 表示相邻行押韵比碰巧多
    pub lift: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RhymePairExample {
    pub song_id: String,
    pub first: String,
    pub second: String,
}

/// 一种韵脚：相邻行押它的有几对
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RhymeTail {
    /// 按读的顺序，如 `aiu`
    pub tail: String,
    pub pairs: i64,
    pub songs: i64,
    /// 其中两行押韵的那几个词完全一样（如都以「ないで」结尾）
    pub same_word: i64,
    pub example: RhymePairExample,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongRhyme {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub pairs: i64,
    pub rhymed: i64,
    pub rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RhymeStatsResult {
    pub tail: usize,
    /// 韵脚 1~5 个音各算一遍
    pub lengths: Vec<RhymeLength>,
    /// 按 `tail` 个音：相邻行押韵的对里，押韵的词完全一样的有几对
    pub same_word: i64,
    /// 按 `tail` 个音，最常见的韵脚，最多 `limit` 个
    pub tails: Vec<RhymeTail>,
    /// 相邻行押韵比例最高的歌，最多 `limit` 首
    pub songs: Vec<SongRhyme>,
    pub provenance: Provenance,
}

struct Edge<'a> {
    line: &'a analysis::Line,
    /// 从行尾（或行首）往里数的音和所在的词
    sounds: Vec<(Sound, usize)>,
}

impl Edge<'_> {
    fn key(&self, k: usize) -> Option<&[(Sound, usize)]> {
        (self.sounds.len() >= k).then(|| &self.sounds[..k])
    }

    fn rhymes(&self, other: &Edge, k: usize) -> bool {
        match (self.key(k), other.key(k)) {
            (Some(a), Some(b)) => a.iter().zip(b).all(|(x, y)| x.0 == y.0),
            _ => false,
        }
    }

    /// 前 k 个音落在的词（按原文顺序连起来）
    fn carrier(&self, k: usize) -> String {
        let idx: Vec<usize> = self.sounds[..k].iter().map(|s| s.1).collect();
        let (lo, hi) = (*idx.iter().min().expect("k ≥ 1"), *idx.iter().max().expect("k ≥ 1"));
        surfaces(self.line, lo, hi)
    }

    /// 前 k 个音按读的顺序
    fn tail(&self, k: usize, end: bool) -> String {
        let mut v: Vec<char> = self.sounds[..k].iter().map(|s| s.0.symbol()).collect();
        if end {
            v.reverse();
        }
        v.into_iter().collect()
    }
}

fn pairs(n: i64) -> i64 {
    n * (n - 1) / 2
}

/// 相邻两行押韵比碰巧多多少，最常见的韵脚，押韵最密的歌。
///
/// - 相邻行：同一首歌里挨着的两行（歌词文件里的顺序）。两行文本完全相同的不算（副歌重复一定「押韵」）；
///   按不重复行计时，同一首歌里同样的两行只算一次。
/// - 基线：同一首歌里任意两行（文本不同）押韵的比例。歌自己偏爱某些元音、某些句尾，基线把这一点扣掉；
///   lift = 相邻行押韵率 ÷ 基线。
pub fn rhyme_stats(conn: &Connection, query: &RhymeStatsQuery) -> Result<RhymeStatsResult> {
    let end = query.position != RhymePosition::Start;
    let tail = query.tail.clamp(1, 8);
    let lines = load_with_barriers(conn, &query.scope)?;
    let distinct = query.scope.unit == analysis::CountUnit::DistinctLines;
    let mut rules = SongRules::default();

    // 按歌分好的边
    let mut by_song: Vec<Vec<Edge>> = Vec::new();
    for line in &lines {
        let ls = line_sounds(line, pronounce_for_rhyme(&mut rules, conn, line, query.scope.jp_only)?, query.strict);
        let seg = if end { ls.segments.last() } else { ls.segments.first() }.cloned().unwrap_or_default();
        let mut sounds: Vec<(Sound, usize)> = seg.into_iter().map(|(s, _, t)| (s, t)).collect();
        if end {
            sounds.reverse();
        }
        if by_song.last().and_then(|v| v.first()).map(|e| &e.line.song_id) != Some(&line.song_id) {
            by_song.push(Vec::new());
        }
        by_song.last_mut().expect("刚 push 过").push(Edge { line, sounds });
    }

    let mut lengths = Vec::new();
    for k in 1..=5usize {
        let (mut adj, mut adj_r, mut base, mut base_r) = (0i64, 0i64, 0i64, 0i64);
        for song in &by_song {
            let mut seen: HashSet<(&str, &str)> = HashSet::new();
            for w in song.windows(2) {
                let (a, b) = (&w[0], &w[1]);
                if a.key(k).is_none() || b.key(k).is_none() || a.line.text == b.line.text {
                    continue;
                }
                if distinct && !seen.insert((&a.line.text, &b.line.text)) {
                    continue;
                }
                adj += 1;
                if a.rhymes(b, k) {
                    adj_r += 1;
                }
            }
            // 基线：按韵脚、按文本分组数对
            let mut by_text: HashMap<&str, i64> = HashMap::new();
            let mut by_key: HashMap<Vec<Sound>, HashMap<&str, i64>> = HashMap::new();
            for e in song.iter().filter(|e| !distinct || !e.line.repeat) {
                let Some(key) = e.key(k) else { continue };
                *by_text.entry(&e.line.text).or_default() += 1;
                *by_key.entry(key.iter().map(|s| s.0).collect()).or_default().entry(&e.line.text).or_default() += 1;
            }
            let n: i64 = by_text.values().sum();
            base += pairs(n) - by_text.values().map(|&m| pairs(m)).sum::<i64>();
            for texts in by_key.values() {
                let nk: i64 = texts.values().sum();
                base_r += pairs(nk) - texts.values().map(|&m| pairs(m)).sum::<i64>();
            }
        }
        let rate = |r: i64, n: i64| if n > 0 { r as f64 / n as f64 } else { 0.0 };
        let (ar, br) = (rate(adj_r, adj), rate(base_r, base));
        lengths.push(RhymeLength {
            length: k,
            adjacent: adj,
            adjacent_rhymed: adj_r,
            adjacent_rate: ar,
            baseline: base,
            baseline_rhymed: base_r,
            baseline_rate: br,
            lift: (br > 0.0 && adj > 0).then(|| ar / br),
        });
    }

    // 按 tail 个音：韵脚排行、歌的排行
    #[derive(Default)]
    struct TailAcc {
        pairs: i64,
        songs: HashSet<String>,
        same_word: i64,
        example: Option<RhymePairExample>,
        example_same: bool,
    }
    let mut tails: HashMap<String, TailAcc> = HashMap::new();
    let mut song_rows: Vec<SongRhyme> = Vec::new();
    let mut same_word = 0i64;
    for song in &by_song {
        let mut seen: HashSet<(&str, &str)> = HashSet::new();
        let (mut n, mut r) = (0i64, 0i64);
        for w in song.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if a.key(tail).is_none() || b.key(tail).is_none() || a.line.text == b.line.text {
                continue;
            }
            if distinct && !seen.insert((&a.line.text, &b.line.text)) {
                continue;
            }
            n += 1;
            if !a.rhymes(b, tail) {
                continue;
            }
            r += 1;
            let same = a.carrier(tail) == b.carrier(tail);
            if same {
                same_word += 1;
            }
            let acc = tails.entry(a.tail(tail, end)).or_default();
            acc.pairs += 1;
            acc.songs.insert(a.line.song_id.clone());
            if same {
                acc.same_word += 1;
            }
            // 例子优先挑押韵的词不一样的
            if acc.example.is_none() || (acc.example_same && !same) {
                acc.example_same = same;
                acc.example = Some(RhymePairExample {
                    song_id: a.line.song_id.clone(),
                    first: a.line.text.clone(),
                    second: b.line.text.clone(),
                });
            }
        }
        if let Some(first) = song.first()
            && n >= query.min_pairs.max(1) as i64
        {
            song_rows.push(SongRhyme {
                song_id: first.line.song_id.clone(),
                title: String::new(),
                artist: String::new(),
                pairs: n,
                rhymed: r,
                rate: r as f64 / n as f64,
            });
        }
    }
    let limit = query.limit.max(1);
    let mut tails: Vec<RhymeTail> = tails
        .into_iter()
        .filter_map(|(tail, a)| {
            Some(RhymeTail { tail, pairs: a.pairs, songs: a.songs.len() as i64, same_word: a.same_word, example: a.example? })
        })
        .collect();
    tails.sort_by(|a, b| b.pairs.cmp(&a.pairs).then_with(|| b.songs.cmp(&a.songs)).then_with(|| a.tail.cmp(&b.tail)));
    tails.truncate(limit);
    song_rows.sort_by(|a, b| {
        b.rate.total_cmp(&a.rate).then_with(|| b.rhymed.cmp(&a.rhymed)).then_with(|| a.song_id.cmp(&b.song_id))
    });
    song_rows.truncate(limit);
    {
        let mut stmt = conn.prepare("SELECT COALESCE(title,''), COALESCE(artist,'') FROM songs WHERE id = ?1")?;
        for s in &mut song_rows {
            if let Ok((t, a)) = stmt.query_row([&s.song_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) {
                s.title = t;
                s.artist = a;
            }
        }
    }

    let provenance = analysis::provenance(conn, &query.scope, &lines, serde_json::to_value(query)?)?;
    Ok(RhymeStatsResult { tail, lengths, same_word, tails, songs: song_rows, provenance })
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

    /// 合成库里加一首读音齐全的歌（原有的歌没有读音，全是断点，不影响押韵的数）
    fn rhyme_song() -> crate::Corpus {
        let corpus = crate::fixture::corpus().unwrap();
        let conn = corpus.connection();
        conn.execute("INSERT INTO songs (id, title, artist) VALUES ('900', '押韵', 'テスト')", []).unwrap();
        type Tok3<'a> = (&'a str, &'a str, &'a str);
        let lines: [(&str, &[Tok3]); 7] = [
            ("ナンダ", &[("ナンダ", "ナンダ", "INTJ")]),
            ("君が笑う", &[("君", "キミ", "NOUN"), ("が", "ガ", "ADP"), ("笑う", "ワラウ", "VERB")]),
            ("夢が光る", &[("夢", "ユメ", "NOUN"), ("が", "ガ", "ADP"), ("光る", "ヒカル", "VERB")]),
            ("空を見た", &[("空", "ソラ", "NOUN"), ("を", "ヲ", "ADP"), ("見", "ミ", "VERB"), ("た", "タ", "AUX")]),
            ("海を見た", &[("海", "ウミ", "NOUN"), ("を", "ヲ", "ADP"), ("見", "ミ", "VERB"), ("た", "タ", "AUX")]),
            ("Hello", &[("Hello", "", "INTJ")]),
            ("君が笑う", &[("君", "キミ", "NOUN"), ("が", "ガ", "ADP"), ("笑う", "ワラウ", "VERB")]),
        ];
        for (i, (text, toks)) in lines.iter().enumerate() {
            conn.execute(
                "INSERT INTO utterances (song_id, line_idx, time_sec, text) VALUES ('900', ?1, ?2, ?3)",
                rusqlite::params![i as i64, i as f64 * 5.0, text],
            )
            .unwrap();
            let uid = conn.last_insert_rowid();
            for (j, (surface, reading, pos)) in toks.iter().enumerate() {
                conn.execute(
                    "INSERT INTO tokens (utterance_id, token_idx, surface, lemma, pos, reading) VALUES (?1,?2,?3,?3,?4,?5)",
                    rusqlite::params![uid, j as i64, surface, pos, reading],
                )
                .unwrap();
            }
        }
        corpus
    }

    fn distinct() -> Scope {
        Scope { unit: analysis::CountUnit::DistinctLines, ..Scope::default() }
    }

    #[test]
    fn patterns_parse_kana_by_pronunciation_and_latin_vowels() {
        let p = |s: &str, strict: bool| show_slots(&parse_pattern(s, strict));
        assert_eq!(p("とうきょう", false), "ooo(ou)");
        assert_eq!(p("えいえん", false), "eee");
        assert_eq!(p("ナイフ", false), "aiu");
        assert_eq!(p("kimi", false), "ii");
        assert_eq!(p("ＡＩ", false), "ai");
        assert_eq!(p("a.u", false), "a.u");
        assert_eq!(p("ナンダ", false), "aa");
        assert_eq!(p("ナンダ", true), "ana");
        assert_eq!(p("a n a", false), "aa");
        assert_eq!(p("やっぱ", true), "aqa");
        assert_eq!(p("xyz", false), "");
        // 看不出是不是动词：词尾的う两种读法都算；いう 读ユー
        assert_eq!(p("おもう", false), "oo(ou)");
        assert_eq!(p("いう", false), "uu");
        assert_eq!(p("そういう", false), "oouu");
    }

    #[test]
    fn rhyme_finds_line_ends_starts_and_highlights_the_words() {
        let corpus = rhyme_song();
        let conn = corpus.connection();
        let q = |pattern: &str, position: RhymePosition, strict: bool, scope: Scope| {
            rhyme(conn, &RhymeQuery { scope, pattern: pattern.into(), position, strict, limit: 50 }).unwrap()
        };

        let r = q("oia", RhymePosition::End, false, Scope::default());
        assert_eq!((r.pattern.as_str(), r.lines, r.songs), ("oia", 2, 1));
        assert_eq!(r.hits[0].text, "空を見た");
        assert_eq!(r.hits[0].matched, "オミタ");
        assert_eq!(r.hits[0].kana, "ソラオミタ");
        assert_eq!((r.hits[0].char_start, r.hits[0].char_end), (Some(1), Some(4)));
        assert_eq!((r.hits[0].title.as_str(), r.hits[0].time_sec), ("押韵", Some(15.0)));
        assert_eq!(r.carriers.len(), 1);
        assert_eq!((r.carriers[0].text.as_str(), r.carriers[0].lines), ("を見た", 2));

        // 假名模式按发音转：みた → i a
        assert_eq!(q("みた", RhymePosition::End, false, Scope::default()).lines, 2);
        // 头韵：君が… 开头 i i；重复的那一行按不重复行算只算一次
        assert_eq!(q("kimi", RhymePosition::Start, false, Scope::default()).lines, 2);
        assert_eq!(q("kimi", RhymePosition::Start, false, distinct()).lines, 1);
        // ン 默认跳过；要对上时 ナンダ 是 a n a
        assert_eq!(q("aa", RhymePosition::Start, false, Scope::default()).lines, 1);
        assert_eq!(q("aa", RhymePosition::Start, true, Scope::default()).lines, 0);
        assert_eq!(q("ana", RhymePosition::Start, true, Scope::default()).lines, 1);
        // 行里任何地方：除了 Hello 每行都有 a；a-i 只有 夢が光る 的 ガヒ
        assert_eq!(q("a", RhymePosition::Any, false, Scope::default()).lines, 6);
        let ai = q("ai", RhymePosition::Any, false, Scope::default());
        assert_eq!(ai.hits.iter().map(|h| h.matched.as_str()).collect::<Vec<_>>(), ["ガヒ"]);
        // 英文行没有韵脚
        assert!(q(".", RhymePosition::End, false, Scope::default()).hits.iter().all(|h| h.text != "Hello"));
        // 没有能比的音
        let none = q("xyz", RhymePosition::End, false, Scope::default());
        assert_eq!((none.pattern.as_str(), none.lines), ("", 0));
    }

    #[test]
    fn english_words_stay_breakpoints_even_with_japanese_only() {
        let corpus = rhyme_song();
        let conn = corpus.connection();
        conn.execute("INSERT INTO utterances (song_id, line_idx, time_sec, text) VALUES ('900', 9, 50.0, '見たyeah')", []).unwrap();
        let uid = conn.last_insert_rowid();
        for (j, (surface, reading, pos)) in [("見", "ミ", "VERB"), ("た", "タ", "AUX"), ("yeah", "", "INTJ")].iter().enumerate() {
            conn.execute(
                "INSERT INTO tokens (utterance_id, token_idx, surface, lemma, pos, reading) VALUES (?1,?2,?3,?3,?4,?5)",
                rusqlite::params![uid, j as i64, surface, pos, reading],
            )
            .unwrap();
        }
        // 行尾是 yeah：没有韵脚，勾不勾「仅日文」都一样
        for jp_only in [false, true] {
            let scope = Scope { jp_only, ..Scope::default() };
            let r = rhyme(conn, &RhymeQuery { scope, pattern: "ia".into(), ..RhymeQuery::default() }).unwrap();
            assert!(r.hits.iter().all(|h| h.text != "見たyeah"), "jp_only = {jp_only}");
        }
        // Sudachi 给英文词注了片假名：平时算，勾「仅日文」就是断点
        conn.execute("INSERT INTO utterances (song_id, line_idx, time_sec, text) VALUES ('900', 10, 55.0, 'my darling')", []).unwrap();
        let uid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO tokens (utterance_id, token_idx, surface, lemma, pos, reading) VALUES (?1, 0, 'darling', 'darling', 'NOUN', 'ダーリン')",
            [uid],
        )
        .unwrap();
        let q = |jp_only| {
            let scope = Scope { jp_only, ..Scope::default() };
            rhyme(conn, &RhymeQuery { scope, pattern: "aai".into(), ..RhymeQuery::default() }).unwrap()
        };
        assert!(q(false).hits.iter().any(|h| h.text == "my darling"));
        assert!(q(true).hits.iter().all(|h| h.text != "my darling"));
    }

    #[test]
    fn rhyme_stats_compare_adjacent_lines_with_any_two_lines_of_the_song() {
        let corpus = rhyme_song();
        let conn = corpus.connection();
        let r = rhyme_stats(conn, &RhymeStatsQuery { min_pairs: 1, ..RhymeStatsQuery::default() }).unwrap();
        let len = |k: usize| r.lengths.iter().find(|l| l.length == k).unwrap();
        // 相邻：ナンダ/君が笑う、君が笑う/夢が光る、夢が光る/空を見た、空を見た/海を見た；Hello 两边不算
        assert_eq!((len(1).adjacent, len(1).adjacent_rhymed), (4, 2));
        assert_eq!((len(2).adjacent, len(2).adjacent_rhymed), (4, 2));
        // 三个音：ナンダ 只有两个音，不算
        assert_eq!((len(3).adjacent, len(3).adjacent_rhymed), (3, 1));
        // 基线：五行至少三个音，两行「君が笑う」文本相同不算一对 → 9 对，押 o-i-a 的 1 对
        assert_eq!((len(3).baseline, len(3).baseline_rhymed), (9, 1));
        assert!((len(3).lift.unwrap() - 3.0).abs() < 1e-9);
        assert_eq!(r.tails.len(), 1);
        assert_eq!((r.tails[0].tail.as_str(), r.tails[0].pairs, r.tails[0].same_word), ("oia", 1, 1));
        assert_eq!(r.same_word, 1);
        assert_eq!(r.tails[0].example.first, "空を見た");
        assert_eq!(r.songs.len(), 1);
        assert_eq!((r.songs[0].title.as_str(), r.songs[0].pairs, r.songs[0].rhymed), ("押韵", 3, 1));

        // 不重复行：重复的「君が笑う」不进基线
        let d = rhyme_stats(conn, &RhymeStatsQuery { scope: distinct(), min_pairs: 1, ..RhymeStatsQuery::default() }).unwrap();
        let l3 = d.lengths.iter().find(|l| l.length == 3).unwrap();
        assert_eq!((l3.baseline, l3.baseline_rhymed, l3.adjacent), (6, 1, 3));

        // 头韵：行首两个音 君が i i / 夢が u e / 空を o a / 海を u i：没有相邻的对上
        let h = rhyme_stats(conn, &RhymeStatsQuery { position: RhymePosition::Start, tail: 2, min_pairs: 1, ..RhymeStatsQuery::default() }).unwrap();
        let l2 = h.lengths.iter().find(|l| l.length == 2).unwrap();
        assert_eq!((l2.adjacent, l2.adjacent_rhymed), (4, 0));
        assert!(h.tails.is_empty());
    }
}
