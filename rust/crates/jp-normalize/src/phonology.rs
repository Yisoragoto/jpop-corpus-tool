//! 读音 → 拍（モーラ）→ 辅音和元音。语料的音系层（元音分布、单元音词）建在这上面。
//!
//! ## 读音从哪来
//!
//! tokens 的 `reading` 是 Sudachi 的读音，**按写法**记的片假名：助词「は」记ハ、「を」记ヲ，
//! 「東京」记トウキョウ、「永遠」记エイエン。唱出来是ワ、オ、トーキョー、エーエン。
//! 直接拿它数元音，ハ/ワ、长音都会数错，所以先转成按发音的写法（[`pronounce`]）：
//!
//! 1. 助词（Sudachi 词性为助詞，没标时看 UPOS 的 ADP / SCONJ / PART）的ハ、ヘ读ワ、エ；ヲ 一律读オ。
//!    ヅ、ヂ 和 ズ、ジ 同音，写成后者。
//! 2. 「いう」（言う、そういう）读ユー。
//! 3. 长音：同一个元音连着（イイ、オオ、ネェ）、エ段 + イ、オ段 + ウ 读成长音拍
//!    （イー、エーエン、トーキョー）。动词词尾オ段后面的ウ是活用词尾（思う オモウ、問う トウ），
//!    意志形（言おう イオー）除外。Sudachi 把「行こう」「だろ…」切开时，接在オ段后面的助动词「う」
//!    也读长音（[`pronounce_words`]）。
//! 4. ー 是长音拍，元音跟前一拍。
//!
//! 只是近似东京音的规则，不查发音词典。在真实库（8,443 行）上和 UniDic 的发音字段（`pron`）
//! 逐行对过账：两边都能读的 7,591 行里，拍的写法完全相同的 7,345 行，元音序列相同的 7,464 行；
//! 五个元音的占比差不到千分之一。剩下的差异主要是：
//! - 复合词内部的词界：生き急ぐ（イキイソグ）、ため息（タメイキ）——不看词素切分，规则会读成长音；
//! - 两边选的读音不同（君 キミ / クン、力 チカラ / リョク）；
//! - 外来语：UniDic 把ヴ写成ブ，这里保留 v。
//!
//! ## 拍
//!
//! 一拍是一个假名，或者一个假名加后面的小字（キャ、ティ、ファ）。ッ（促音）、ン（拨音）、ー（长音）
//! 各自也是一拍：ッ、ン 没有元音，ー 的元音是前一拍的。读音里有假名以外的字（英文、数字没读音的）
//! 整个词不拆，返回 None。

use serde::{Deserialize, Serialize};

/// 五个元音。按段：ア段 = A……
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Vowel {
    A,
    I,
    U,
    E,
    O,
}

impl Vowel {
    pub const ALL: [Vowel; 5] = [Vowel::A, Vowel::I, Vowel::U, Vowel::E, Vowel::O];

    pub fn romaji(self) -> char {
        match self {
            Vowel::A => 'a',
            Vowel::I => 'i',
            Vowel::U => 'u',
            Vowel::E => 'e',
            Vowel::O => 'o',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MoraKind {
    /// 普通的拍：（辅音 +）元音
    Plain,
    /// ッ
    Sokuon,
    /// ン
    Hatsuon,
    /// 长音：ー，或者由エイ、オウ、ウウ读成的长音
    Long,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mora {
    /// 这一拍的片假名（按发音：长音拍是 ー）
    pub kana: String,
    /// 辅音，平文式罗马字的写法：k、sh、ch、ts、ky……没有辅音是空串。ッ 记 Q，ン 记 N
    pub consonant: String,
    /// 元音。ッ、ン 没有
    pub vowel: Option<Vowel>,
    pub kind: MoraKind,
}

impl Mora {
    /// 平文式罗马字：ka、shi、kyo；ッ 是 Q，ン 是 N，长音是前一拍元音
    pub fn romaji(&self) -> String {
        match self.kind {
            MoraKind::Sokuon | MoraKind::Hatsuon => self.consonant.clone(),
            _ => {
                let mut s = self.consonant.clone();
                if let Some(v) = self.vowel {
                    s.push(v.romaji());
                }
                s
            }
        }
    }
}

fn hira_to_kata(c: char) -> char {
    match c {
        'ぁ'..='ゖ' => char::from_u32(c as u32 + 0x60).unwrap_or(c),
        _ => c,
    }
}

/// 一个片假名（不含小字）的辅音和元音。不是假名返回 None
fn base(c: char) -> Option<(&'static str, Option<Vowel>)> {
    use Vowel::*;
    let r = match c {
        'ア' => ("", A), 'イ' => ("", I), 'ウ' => ("", U), 'エ' => ("", E), 'オ' => ("", O),
        'カ' => ("k", A), 'キ' => ("k", I), 'ク' => ("k", U), 'ケ' => ("k", E), 'コ' => ("k", O),
        'ガ' => ("g", A), 'ギ' => ("g", I), 'グ' => ("g", U), 'ゲ' => ("g", E), 'ゴ' => ("g", O),
        'サ' => ("s", A), 'シ' => ("sh", I), 'ス' => ("s", U), 'セ' => ("s", E), 'ソ' => ("s", O),
        'ザ' => ("z", A), 'ジ' => ("j", I), 'ズ' => ("z", U), 'ゼ' => ("z", E), 'ゾ' => ("z", O),
        'タ' => ("t", A), 'チ' => ("ch", I), 'ツ' => ("ts", U), 'テ' => ("t", E), 'ト' => ("t", O),
        'ダ' => ("d", A), 'ヂ' => ("j", I), 'ヅ' => ("z", U), 'デ' => ("d", E), 'ド' => ("d", O),
        'ナ' => ("n", A), 'ニ' => ("n", I), 'ヌ' => ("n", U), 'ネ' => ("n", E), 'ノ' => ("n", O),
        'ハ' => ("h", A), 'ヒ' => ("h", I), 'フ' => ("f", U), 'ヘ' => ("h", E), 'ホ' => ("h", O),
        'バ' => ("b", A), 'ビ' => ("b", I), 'ブ' => ("b", U), 'ベ' => ("b", E), 'ボ' => ("b", O),
        'パ' => ("p", A), 'ピ' => ("p", I), 'プ' => ("p", U), 'ペ' => ("p", E), 'ポ' => ("p", O),
        'マ' => ("m", A), 'ミ' => ("m", I), 'ム' => ("m", U), 'メ' => ("m", E), 'モ' => ("m", O),
        'ヤ' => ("y", A), 'ユ' => ("y", U), 'ヨ' => ("y", O),
        'ラ' => ("r", A), 'リ' => ("r", I), 'ル' => ("r", U), 'レ' => ("r", E), 'ロ' => ("r", O),
        'ワ' => ("w", A), 'ヰ' => ("", I), 'ヱ' => ("", E), 'ヲ' => ("", O),
        'ヴ' => ("v", U),
        'ン' => return Some(("N", None)),
        'ッ' => return Some(("Q", None)),
        // 单独出现的小字（前面没有能拼的假名）当普通假名
        'ァ' => ("", A), 'ィ' => ("", I), 'ゥ' => ("", U), 'ェ' => ("", E), 'ォ' => ("", O),
        'ャ' => ("y", A), 'ュ' => ("y", U), 'ョ' => ("y", O), 'ヮ' => ("w", A),
        'ヵ' => ("k", A), 'ヶ' => ("k", E),
        _ => return None,
    };
    Some((r.0, Some(r.1)))
}

/// 能和前一个假名拼成一拍的小字，和它带的元音
fn small(c: char) -> Option<Vowel> {
    match c {
        'ャ' | 'ァ' | 'ヮ' => Some(Vowel::A),
        'ュ' | 'ゥ' => Some(Vowel::U),
        'ョ' | 'ォ' => Some(Vowel::O),
        'ィ' => Some(Vowel::I),
        'ェ' => Some(Vowel::E),
        _ => None,
    }
}

/// 拼上小字之后的辅音：キ+ャ → ky，シ+ャ → sh，ウ+ィ → w，フ+ァ → f，ク+ヮ → kw
fn combined_onset(onset: &str, base_vowel: Vowel, small_char: char) -> String {
    let yoon = matches!(small_char, 'ャ' | 'ュ' | 'ョ');
    if small_char == 'ヮ' {
        return format!("{onset}w");
    }
    match onset {
        "" if base_vowel == Vowel::U => "w".into(),
        "" if base_vowel == Vowel::I => "y".into(),
        // 拗音、イ段 + ェ：sh / ch / j 本身已经是腭化音
        "sh" | "ch" | "j" => onset.into(),
        _ if base_vowel == Vowel::I && (yoon || small_char == 'ェ') => format!("{onset}y"),
        // フ+ュ、テ+ュ、デ+ュ
        _ if yoon => format!("{onset}y"),
        // ツ+ァ、テ+ィ、ト+ゥ、ヴ+ァ……辅音不变
        _ => onset.into(),
    }
}

/// 按写法的片假名（或平假名）读音拆成拍，不做长音、助词的改写（见 [`pronounce`]）。
/// 有假名以外的字返回 None
pub fn morae(reading: &str) -> Option<Vec<Mora>> {
    let chars: Vec<char> = reading.chars().map(hira_to_kata).collect();
    if chars.is_empty() {
        return None;
    }
    let mut out: Vec<Mora> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == 'ー' {
            let vowel = out.iter().rev().find_map(|m| m.vowel)?;
            out.push(Mora { kana: "ー".into(), consonant: String::new(), vowel: Some(vowel), kind: MoraKind::Long });
            i += 1;
            continue;
        }
        let (onset, vowel) = base(c)?;
        let Some(v) = vowel else {
            let kind = if c == 'ン' { MoraKind::Hatsuon } else { MoraKind::Sokuon };
            out.push(Mora { kana: c.to_string(), consonant: onset.into(), vowel: None, kind });
            i += 1;
            continue;
        };
        // 小写元音跟在同一个元音后面（ネェ、ナァ、フゥ）是拖长，单独算一拍长音
        if let Some(&next) = chars.get(i + 1)
            && matches!(next, 'ァ' | 'ィ' | 'ゥ' | 'ェ' | 'ォ')
            && small(next) == Some(v)
        {
            out.push(Mora { kana: c.to_string(), consonant: onset.into(), vowel: Some(v), kind: MoraKind::Plain });
            out.push(Mora { kana: "ー".into(), consonant: String::new(), vowel: Some(v), kind: MoraKind::Long });
            i += 2;
            continue;
        }
        // 后面跟着能拼的小字：拼成一拍
        if let Some(&next) = chars.get(i + 1)
            && let Some(sv) = small(next)
            && !matches!(c, 'ァ' | 'ィ' | 'ゥ' | 'ェ' | 'ォ' | 'ャ' | 'ュ' | 'ョ' | 'ヮ')
        {
            out.push(Mora {
                kana: format!("{c}{next}"),
                consonant: combined_onset(onset, v, next),
                vowel: Some(sv),
                kind: MoraKind::Plain,
            });
            i += 2;
            continue;
        }
        out.push(Mora { kana: c.to_string(), consonant: onset.into(), vowel: Some(v), kind: MoraKind::Plain });
        i += 1;
    }
    Some(out)
}

/// 词性是不是助词：UPOS 的 ADP / SCONJ / PART，或者 Sudachi 的「助詞」
fn is_particle(pos: &str, pos_ja: &str) -> bool {
    pos_ja.starts_with("助詞") || (pos_ja.is_empty() && matches!(pos, "ADP" | "SCONJ" | "PART"))
}

/// 动词（不含助动词）
fn is_verb(pos: &str, pos_ja: &str) -> bool {
    pos_ja.starts_with("動詞") || (pos_ja.is_empty() && pos == "VERB")
}

fn is_aux(pos: &str, pos_ja: &str) -> bool {
    pos_ja.starts_with("助動詞") || (pos_ja.is_empty() && pos == "AUX")
}

/// 一个词：tokens 里的读音（按写法）、表层形、UPOS、Sudachi 词性和活用形（没标是空串）
#[derive(Debug, Clone, Copy, Default)]
pub struct Word<'a> {
    pub reading: &'a str,
    pub surface: &'a str,
    pub pos: &'a str,
    pub pos_ja: &'a str,
    pub cform: &'a str,
}

fn long(vowel: Option<Vowel>) -> Mora {
    Mora { kana: "ー".into(), consonant: String::new(), vowel, kind: MoraKind::Long }
}

/// 一个词按发音拆成拍。规则见模块说明。读音里有假名以外的字返回 None
pub fn pronounce(w: Word) -> Option<Vec<Mora>> {
    let mut ms = morae(w.reading)?;
    // 1. 助词：は へ
    if is_particle(w.pos, w.pos_ja) && ms.len() == 1 {
        let replaced = match (w.surface, ms[0].kana.as_str()) {
            ("は", "ハ") => Some(("ワ", "w", Vowel::A)),
            ("へ", "ヘ") => Some(("エ", "", Vowel::E)),
            _ => None,
        };
        if let Some((kana, consonant, vowel)) = replaced {
            ms[0] = Mora { kana: kana.into(), consonant: consonant.into(), vowel: Some(vowel), kind: MoraKind::Plain };
        }
    }
    // ヲ 读オ，ヅ、ヂ 读ズ、ジ：音一样，写法统一成常用的那个
    for m in ms.iter_mut() {
        match m.kana.as_str() {
            "ヲ" => m.kana = "オ".into(),
            "ヅ" => m.kana = "ズ".into(),
            "ヂ" => m.kana = "ジ".into(),
            _ => {}
        }
    }
    // 2.「いう」（言う、そういう）读ユー
    let n = ms.len();
    if n >= 2
        && (ms[n - 2].kana == "イ" || ms[n - 2].kana == "ユ")
        && ms[n - 1].kana == "ウ"
        && (w.surface.ends_with("いう") || w.surface.ends_with("言う") || w.surface.ends_with("云う"))
    {
        ms[n - 2] = Mora { kana: "ユ".into(), consonant: "y".into(), vowel: Some(Vowel::U), kind: MoraKind::Plain };
        ms[n - 1] = long(Some(Vowel::U));
    }
    // 3. 长音：同一个元音连着（イイ、オオ、アア）、エ段+イ、オ段+ウ。
    //    动词词尾オ段后面的ウ是活用词尾（思う、問う），意志形（言おう）除外；ウ段后面的照样拖长（食う クー）
    let verb_ending = is_verb(w.pos, w.pos_ja) && !w.cform.starts_with("意志推量形");
    let last = ms.len().saturating_sub(1);
    for i in 1..ms.len() {
        let (prev, cur) = (&ms[i - 1], &ms[i]);
        if prev.vowel.is_none() || prev.kind == MoraKind::Long || cur.kind != MoraKind::Plain || !cur.consonant.is_empty() {
            continue;
        }
        let is_long = match (prev.vowel, cur.vowel) {
            (Some(Vowel::E), Some(Vowel::I)) => true,
            (Some(Vowel::O), Some(Vowel::U)) => !(verb_ending && i == last),
            (a, b) => a == b,
        };
        if is_long {
            let vowel = prev.vowel;
            ms[i] = long(vowel);
        }
    }
    Some(ms)
}

/// 一行里的词逐个 [`pronounce`]，再处理跨词的两件事：
///
/// - 长音：Sudachi 把「行こう」「でしょう」切成 行こ + う、でしょ + う，意志、推量的「う」
///   接在オ段后面读长音（イコー、デショー）；
/// - 小字：歌词里拖腔的「とぅるる」「寝ぇ」有时被切成 と + ぅるる、寝 + ぇ。打头的小字和前一个词的
///   最后一拍拼成一拍（トゥ），同元音的读长音（ネー）。
///
/// 拆不了的词是 None。
pub fn pronounce_words(words: &[Word]) -> Vec<Option<Vec<Mora>>> {
    let mut out: Vec<Option<Vec<Mora>>> = words.iter().map(|w| pronounce(*w)).collect();
    for i in 1..out.len() {
        let Some(first) = out[i].as_ref().and_then(|ms| ms.first()) else { continue };
        if first.kind != MoraKind::Plain || !matches!(first.kana.as_str(), "ァ" | "ィ" | "ゥ" | "ェ" | "ォ" | "ャ" | "ュ" | "ョ" | "ヮ") {
            continue;
        }
        let Some(prev) = out[i - 1].as_ref().and_then(|ms| ms.last()).filter(|m| m.kind == MoraKind::Plain) else { continue };
        let Some(joined) = morae(&format!("{}{}", prev.kana, first.kana)) else { continue };
        match joined.as_slice() {
            // 拼成一拍：前一个词的最后一拍换成它，小字不再单独算
            [one] => {
                let one = one.clone();
                *out[i - 1].as_mut().and_then(|ms| ms.last_mut()).expect("checked") = one;
                out[i].as_mut().expect("checked").remove(0);
            }
            // 同元音：小字是长音拍
            [_, second] if second.kind == MoraKind::Long => {
                out[i].as_mut().expect("checked")[0] = second.clone();
            }
            _ => {}
        }
    }
    for i in 1..out.len() {
        let w = words[i];
        if !(w.reading == "ウ" && is_aux(w.pos, w.pos_ja)) {
            continue;
        }
        let prev_o = out[i - 1].as_ref().and_then(|ms| ms.last()).is_some_and(|m| m.vowel == Some(Vowel::O));
        if prev_o && let Some(ms) = out[i].as_mut() {
            ms[0] = long(Some(Vowel::O));
        }
    }
    out
}

/// 拍的片假名连起来（按发音）
pub fn kana_of(ms: &[Mora]) -> String {
    ms.iter().map(|m| m.kana.as_str()).collect()
}

/// 元音序列，ッ、ン 不算：トーキョー → o o o o
pub fn vowels_of(ms: &[Mora]) -> Vec<Vowel> {
    ms.iter().filter_map(|m| m.vowel).collect()
}

/// 所有带元音的拍都是同一个元音时返回它（单元音词）。ッ、ン 不影响；一个元音都没有返回 None
pub fn single_vowel(ms: &[Mora]) -> Option<Vowel> {
    let mut vs = ms.iter().filter_map(|m| m.vowel);
    let first = vs.next()?;
    vs.all(|v| v == first).then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_small_kana_joins_the_previous_word() {
        let w = |reading: &'static str| Word { reading, ..Word::default() };
        let out = pronounce_words(&[w("ト"), w("ゥルル")]);
        assert_eq!(kana_of(out[0].as_ref().unwrap()), "トゥ");
        assert_eq!(vowels_of(out[0].as_ref().unwrap()), [Vowel::U]);
        assert_eq!(kana_of(out[1].as_ref().unwrap()), "ルル");
        // 同元音读长音
        let out = pronounce_words(&[w("ネ"), w("ェ")]);
        assert_eq!(kana_of(out[0].as_ref().unwrap()), "ネ");
        assert_eq!(out[1].as_ref().unwrap()[0].kind, MoraKind::Long);
        assert_eq!(vowels_of(out[1].as_ref().unwrap()), [Vowel::E]);
        // 前一个词拆不了、或是 ン：不动
        let out = pronounce_words(&[w("ン"), w("ァ")]);
        assert_eq!(kana_of(out[1].as_ref().unwrap()), "ァ");
    }

    fn word<'a>(reading: &'a str, surface: &'a str, pos: &'a str, pos_ja: &'a str) -> Word<'a> {
        Word { reading, surface, pos, pos_ja, cform: "" }
    }
    fn kana(reading: &str, surface: &str, pos: &str, pos_ja: &str) -> String {
        kana_of(&pronounce(word(reading, surface, pos, pos_ja)).unwrap())
    }
    fn romaji(reading: &str) -> String {
        morae(reading).unwrap().iter().map(|m| m.romaji()).collect::<Vec<_>>().join(".")
    }

    #[test]
    fn small_kana_join_the_previous_mora() {
        assert_eq!(romaji("キョウ"), "kyo.u");
        assert_eq!(romaji("シャツ"), "sha.tsu");
        assert_eq!(romaji("チェンジ"), "che.N.ji");
        assert_eq!(romaji("ティーン"), "ti.i.N");
        assert_eq!(romaji("ファン"), "fa.N");
        assert_eq!(romaji("ウィンター"), "wi.N.ta.a");
        assert_eq!(romaji("ヴァイオリン"), "va.i.o.ri.N");
        assert_eq!(romaji("デュエット"), "dyu.e.Q.to");
        assert_eq!(romaji("ジェット"), "je.Q.to");
        assert_eq!(romaji("キェ"), "kye");
        assert_eq!(morae("キョウ").unwrap().len(), 2);
    }

    #[test]
    fn hiragana_works_and_non_kana_is_rejected() {
        assert_eq!(romaji("きっと"), "ki.Q.to");
        assert!(morae("Love").is_none());
        assert!(morae("").is_none());
        // ー 前面没有元音
        assert!(morae("ー").is_none());
    }

    #[test]
    fn particles_are_read_as_sung() {
        assert_eq!(kana("ハ", "は", "ADP", "助詞-係助詞"), "ワ");
        assert_eq!(kana("ヘ", "へ", "ADP", "助詞-格助詞"), "エ");
        assert_eq!(kana("ヲ", "を", "ADP", "助詞-格助詞"), "オ");
        // 不是助词的は不改：花 ハナ、葉 ハ
        assert_eq!(kana("ハ", "葉", "NOUN", "名詞-普通名詞-一般"), "ハ");
        assert_eq!(kana("ハナ", "花", "NOUN", "名詞-普通名詞-一般"), "ハナ");
    }

    #[test]
    fn long_vowels_but_not_verb_endings() {
        assert_eq!(kana("トウキョウ", "東京", "PROPN", "名詞-固有名詞-地名-一般"), "トーキョー");
        assert_eq!(kana("エイエン", "永遠", "NOUN", "名詞-普通名詞-一般"), "エーエン");
        assert_eq!(kana("クウキ", "空気", "NOUN", "名詞-普通名詞-一般"), "クーキ");
        assert_eq!(kana("オモウ", "思う", "VERB", "動詞-一般"), "オモウ");
        assert_eq!(kana("トウ", "問う", "VERB", "動詞-一般"), "トウ");
        // 动词中间的オウ照样是长音：通じる ツウジル
        assert_eq!(kana("ツウジル", "通じる", "VERB", "動詞-一般"), "ツージル");
        // 有辅音的イ、ウ不是长音：家 イエ 不动，帰る カエル 不动
        assert_eq!(kana("カエル", "帰る", "VERB", "動詞-一般"), "カエル");
        // ン 后面不拼长音
        assert_eq!(kana("ゲンイン", "原因", "NOUN", "名詞-普通名詞-一般"), "ゲンイン");
    }

    #[test]
    fn same_vowels_drawn_out_and_iu_read_yuu() {
        assert_eq!(kana("イイ", "いい", "ADJ", "形容詞-非自立可能"), "イー");
        assert_eq!(kana("トオク", "遠く", "ADJ", "形容詞-一般"), "トーク");
        assert_eq!(kana("ネェ", "ねぇ", "PART", "助詞-終助詞"), "ネー");
        assert_eq!(kana("ダロウ", "だろう", "AUX", "助動詞"), "ダロー");
        assert_eq!(kana("キヅク", "気づく", "VERB", "動詞-一般"), "キズク");
        assert_eq!(kana("イウ", "言う", "VERB", "動詞-一般"), "ユー");
        assert_eq!(kana("ソウイウ", "そういう", "ADJ", "連体詞"), "ソーユー");
        let m = pronounce(Word { reading: "イオウ", surface: "言おう", pos: "VERB", pos_ja: "動詞-一般", cform: "意志推量形" }).unwrap();
        assert_eq!(kana_of(&m), "イオー");
        assert_eq!(morae("ネェ").unwrap().len(), 2);
    }


    #[test]
    fn volitional_u_after_o_is_long_across_words() {
        let words = [word("イコ", "行こ", "VERB", "動詞-非自立可能"), word("ウ", "う", "AUX", "助動詞")];
        let got: Vec<String> = pronounce_words(&words).iter().map(|m| kana_of(m.as_ref().unwrap())).collect();
        assert_eq!(got, ["イコ", "ー"]);
        // 不是オ段后面的う不动
        let words = [word("カ", "買", "VERB", "動詞-一般"), word("ウ", "う", "AUX", "助動詞")];
        assert_eq!(kana_of(pronounce_words(&words)[1].as_ref().unwrap()), "ウ");
    }

    #[test]
    fn single_vowel_ignores_moraic_consonants() {
        let m = pronounce(word("アシタ", "明日", "NOUN", "")).unwrap();
        assert_eq!(single_vowel(&m), None);
        let m = pronounce(word("アナタ", "あなた", "PRON", "")).unwrap();
        assert_eq!(single_vowel(&m), Some(Vowel::A));
        let m = pronounce(word("カンタン", "簡単", "ADJ", "")).unwrap();
        assert_eq!(single_vowel(&m), Some(Vowel::A));
        let m = pronounce(word("エイエン", "永遠", "NOUN", "")).unwrap();
        assert_eq!(single_vowel(&m), Some(Vowel::E));
        assert_eq!(vowels_of(&m), vec![Vowel::E, Vowel::E, Vowel::E]);
    }
}
