//! 从语料组装一张卡片。
//!
//! 例句来自用户自己听的歌，这是这套卡片和现成词库的区别：
//! 「夜」这个词的例句是《夜に駆ける》里的那一句，不是教科书造的。
//!
//! 全程只读本地库，一个请求都不发。

use std::collections::BTreeMap;

use anyhow::Result;
use rusqlite::{Connection, params_from_iter};
use serde::Serialize;

use jp_dict::html::escape;

use crate::dict::Definition;

/// UPOS → 日文词性标签。和 PyQt 版一致，卡片上显示的就是这个。
pub fn pos_label(upos: &str) -> &'static str {
    match upos {
        "NOUN" => "名詞",
        "VERB" => "動詞",
        "ADJ" => "形容詞",
        "ADV" => "副詞",
        "AUX" => "助動詞",
        "PRON" => "代名詞",
        "PROPN" => "固有名詞",
        "INTJ" => "感動詞",
        "ADP" => "助詞",
        "CCONJ" => "接続詞",
        "NUM" => "数詞",
        "PART" => "接辞",
        "SCONJ" => "従属接",
        "DET" => "限定詞",
        _ => "",
    }
}

/// 语料里的一条例句。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Example {
    pub song_id: String,
    pub artist: String,
    pub title: String,
    pub time_sec: Option<f64>,
    /// 歌词原文
    pub text: String,
    /// 这一行里这个词的实际写法（可能是活用形）
    pub surface: String,
    pub audio_path: String,
    /// 这一句的结束时间，取下一行的时间戳。切音频用。
    pub end_sec: Option<f64>,
    /// 歌词行 id。音频文件名 `jpop_{id}.mp3` 用它，和 Python 一致。
    pub utterance_id: i64,
}

impl Example {
    /// 「サカナクション「夜の踊り子」01:23.4」
    pub fn source_label(&self) -> String {
        format!("{}「{}」", self.artist, self.title)
    }

    fn timestamp(&self) -> String {
        let Some(t) = self.time_sec else {
            return String::new();
        };
        let minutes = (t / 60.0).floor() as i64;
        let seconds = t - (minutes as f64) * 60.0;
        format!("{minutes:02}:{seconds:04.1}")
    }
}

/// 取一个词的例句。
///
/// **顺序是确定的**，同一个词导两次拿到同样的句子。Python 版用
/// `ORDER BY RANDOM()`，重导一次卡片内容就变了，没法复现也没法测。
///
/// 但确定顺序有个副作用：直接按 id 排会让例句全挤在同一首歌里。
/// 所以先按 (歌, 行) 排稳，再**优先挑没出现过的歌**——既确定又有分布。
pub fn examples(
    conn: &Connection,
    lemma: &str,
    limit: usize,
    song_ids: &[String],
) -> Result<Vec<Example>> {
    let mut sql = String::from(
        "SELECT u.song_id, s.artist, s.title, u.time_sec, u.text, t.surface,
                COALESCE(s.audio_path,''),
                (SELECT MIN(u2.time_sec) FROM utterances u2
                 WHERE u2.song_id = u.song_id AND u2.time_sec > u.time_sec) AS end_sec,
                u.id
         FROM tokens t
         JOIN utterances u ON u.id = t.utterance_id
         JOIN songs s ON s.id = u.song_id
         WHERE t.lemma = ?1",
    );
    let mut values: Vec<String> = vec![lemma.to_string()];
    if !song_ids.is_empty() {
        let marks: Vec<String> = (0..song_ids.len())
            .map(|i| format!("?{}", i + 2))
            .collect();
        sql.push_str(&format!(" AND u.song_id IN ({})", marks.join(",")));
        values.extend(song_ids.iter().cloned());
    }
    sql.push_str(" ORDER BY u.song_id, u.line_idx");

    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<Example> = stmt
        .query_map(params_from_iter(values), |r| {
            Ok(Example {
                song_id: r.get(0)?,
                artist: r.get(1)?,
                title: r.get(2)?,
                time_sec: r.get(3)?,
                text: r.get(4)?,
                surface: r.get(5)?,
                audio_path: r.get(6)?,
                end_sec: r.get(7)?,
                utterance_id: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    // 同一句歌词（副歌）只留一条；再优先挑不同的歌
    let mut seen_text: std::collections::BTreeSet<String> = Default::default();
    let mut unique: Vec<Example> = Vec::new();
    for row in rows {
        if seen_text.insert(row.text.clone()) {
            unique.push(row);
        }
    }
    let mut picked: Vec<Example> = Vec::new();
    let mut used_songs: std::collections::BTreeSet<String> = Default::default();
    for row in &unique {
        if picked.len() >= limit {
            break;
        }
        if used_songs.insert(row.song_id.clone()) {
            picked.push(row.clone());
        }
    }
    // 还没凑够就把同一首歌的其它句子补上
    for row in unique {
        if picked.len() >= limit {
            break;
        }
        if !picked.iter().any(|p| p.text == row.text) {
            picked.push(row);
        }
    }
    Ok(picked)
}

/// 例句的 HTML：高亮目标词 + 出处和时间戳。
pub fn sentence_html(examples: &[Example], lemma: &str) -> String {
    let mut out = String::new();
    for example in examples {
        // 这一行里的实际写法优先（活用形），退回词典形
        let target = if !example.surface.is_empty() && example.text.contains(&example.surface) {
            &example.surface
        } else {
            lemma
        };
        // 只高亮第一次出现的，和 PyQt 版一致。
        //
        // **在原文里找，找到之后三段各自转义。** 以前是先把整句转义、再去转义后的文本里找转义后的词：
        // 「'」变成 `&#x27;` 之后，找「7」「x」「27」会先碰到实体里的那几个字符，高亮插进实体中间，
        // 卡片上是一串拆坏的乱码；词不在这一行里时，也可能在 `&amp;` 里「找到」一个 amp。
        let found = if target.is_empty() { None } else { example.text.find(target) };
        let highlighted = match found {
            Some(at) => {
                let (before, rest) = example.text.split_at(at);
                let (hit, after) = rest.split_at(target.len());
                format!(r#"{}<b style="color:#c0392b">{}</b>{}"#, escape(before), escape(hit), escape(after))
            }
            None => escape(&example.text),
        };
        out.push_str(&format!(
            r#"<div class="sent">{highlighted}<span class="sent-src">{}{}</span></div>"#,
            escape(&example.source_label()),
            {
                let stamp = example.timestamp();
                if stamp.is_empty() {
                    String::new()
                } else {
                    format!(" {stamp}")
                }
            }
        ));
    }
    out
}

/// 释义的 HTML：按词典分组，每组一个标题。
///
/// 标题颜色对没有固定配色的词典**由名字散列决定**——同一部词典每次
/// 都是同一个颜色，用户扫一眼就知道这条是哪来的。
pub fn meaning_html(definitions: &[Definition]) -> String {
    // 和 Python 一样：一条释义都没有也写出空的外层，卡片字段不是空串
    if definitions.is_empty() {
        return "<div class='defs'></div>".to_string();
    }
    // 保持出现顺序分组：顺序就是 dict_registry 里的优先级
    let mut order: Vec<String> = Vec::new();
    let mut groups: BTreeMap<String, Vec<&Definition>> = BTreeMap::new();
    for d in definitions {
        if !groups.contains_key(&d.source) {
            order.push(d.source.clone());
        }
        groups.entry(d.source.clone()).or_default().push(d);
    }

    let mut out = String::from("<div class='defs'>");
    for source in &order {
        out.push_str("<div class='dict-group'>");
        if !source.is_empty() {
            let (class, style) = header_style(source);
            out.push_str(&format!(
                "<span class='dict-hdr {class}'{style}>{}</span>",
                escape(source)
            ));
        }
        out.push_str("<div class='meaning'>");
        for d in &groups[source] {
            out.push_str("<div class='ym-item'>");
            if d.is_html {
                // 已经排好版的，原样放
                out.push_str(&d.text);
            } else {
                out.push_str(&render_plain(&d.text));
            }
            out.push_str("</div>");
        }
        out.push_str("</div></div>");
    }
    out.push_str("</div>");
    out
}

/// 释义是用别的形查到的（`勉強し` → `勉強する`）时，在最上面注明「辞書形：…」。
/// 和 Python `_prefix_lookup_term_html` 一致。卡片的 Expression 不变。
pub fn prefix_lookup_term(meaning: &str, expression: &str, lookup_term: &str) -> String {
    if lookup_term.is_empty() || lookup_term == expression {
        return meaning.to_string();
    }
    const OPEN: &str = "<div class='defs'>";
    let note = format!("<div class='ym-term-ref'>辞書形：{}</div>", escape(lookup_term));
    match meaning.strip_prefix(OPEN) {
        Some(rest) => format!("{OPEN}{note}{rest}"),
        None => format!("{note}{meaning}"),
    }
}

/// 只保留前 `limit` 部词典的释义。顺序就是 `dict_registry` 里的优先级，
/// 所以「前几部」就是用户自己排在最上面的那几部。
fn take_first_dicts(definitions: Vec<Definition>, limit: usize) -> Vec<Definition> {
    if limit == 0 {
        return Vec::new();
    }
    let mut kept: Vec<String> = Vec::new();
    definitions
        .into_iter()
        .filter(|d| {
            if kept.contains(&d.source) {
                return true;
            }
            if kept.len() < limit {
                kept.push(d.source.clone());
                return true;
            }
            false
        })
        .collect()
}

/// 有固定配色的两部老词典。其余按名字散列取色。
const KNOWN_DICTS: &[&str] = &["明鏡", "小学館"];
const PALETTE: &[&str] = &[
    "#c0392b", "#2471a3", "#16a085", "#8e44ad", "#d35400", "#2c7a7b", "#6c5ce7", "#b03a5b",
    "#4b6584", "#5f8f3f",
];

fn header_style(source: &str) -> (String, String) {
    if KNOWN_DICTS.contains(&source) {
        return (format!("dict-hdr-{source}"), String::new());
    }
    // 和 Python 的 `_hdr_style` 一模一样：md5 十六进制摘要的前 8 位当整数取模。
    // 之前用的是 FNV——一样稳定，但颜色和 PyQt 做的卡片不一样，
    // 「刷新旧牌组」一跑，用户已有卡片上的标题颜色全会变。
    let digest = md5(source.as_bytes());
    let hash = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    let color = PALETTE[(hash as usize) % PALETTE.len()];
    (
        "dict-hdr-other".to_string(),
        format!(" style='background:{color}'"),
    )
}

/// MD5。**只用来给词典标题取色**，好和 Python 的 `hashlib.md5` 取到同一个颜色。
/// 不是安全用途；为这一处不值得引入 md-5 和它的一串依赖。
pub(crate) fn md5(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
        0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
        0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
        0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
        0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
        0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
        0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
    ];
    let mut state: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];

    let mut message = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_le_bytes());

    let (blocks, _) = message.as_chunks::<64>();
    for chunk in blocks {
        let words: [u32; 16] = std::array::from_fn(|i| {
            u32::from_le_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]])
        });
        let [mut a, mut b, mut c, mut d] = state;
        for (i, (&k, &shift)) in K.iter().zip(S.iter()).enumerate() {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k).wrapping_add(words[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(shift));
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
    }

    let mut out = [0u8; 16];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// 纯文本释义排成一个义项块。和 Python `_build_meaning_html._render_item` 逐字一致：
/// 先折叠空白再转义；开头可能有【词形】、义项号（圆圈数字或全半角数字）、〔标签〕，
/// 也可能是 `[注]` 这种说明。
fn render_plain(raw: &str) -> String {
    static SENSE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"^(【[^】]+】)?([①②③④⑤⑥⑦⑧⑨⑩⑪⑫⑬⑭⑮⑯⑰⑱⑲⑳]|[0-9０-９]+)(?:〔([^〕]+)〕)?(.*)$",
        )
        .unwrap()
    });
    static NOTE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^(\[[^\]]+\])(.*)$").unwrap());

    let text = escape(&crate::dict::clean_text(raw));
    if let Some(m) = SENSE.captures(&text) {
        let prefix = m.get(1).map_or("", |g| g.as_str());
        let tag = m.get(3).map_or("", |g| g.as_str());
        let prefix_html = if prefix.is_empty() {
            String::new()
        } else {
            format!("<div class='ym-term-ref'>{prefix}</div>")
        };
        let tag_html = if tag.is_empty() {
            String::new()
        } else {
            format!("<span class='ym-tag'>〔{tag}〕</span>")
        };
        return format!(
            "{prefix_html}<div class='ym-sense'><span class='ym-index'>{}</span>{tag_html}<span class='ym-gloss'>{}</span></div>",
            &m[2],
            m[4].trim()
        );
    }
    if let Some(m) = NOTE.captures(&text) {
        return format!(
            "<div class='ym-note'><span class='ym-note-label'>{}</span><span class='ym-note-body'>{}</span></div>",
            &m[1],
            m[2].trim()
        );
    }
    format!("<div class='ym-sense'><span class='ym-gloss'>{text}</span></div>")
}

/// 一张组装好的卡片，字段名和 note type 对齐。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub expression: String,
    pub reading: String,
    pub meaning: String,
    pub sentence: String,
    pub sentence_audio: String,
    pub source: String,
    pub jlpt: String,
    pub pitch: String,
    pub freq: String,
    pub part_of_speech: String,
    /// 实际查到释义用的形。和 Expression 不同时，释义顶部会注明「辞書形：…」
    pub lookup_term: String,
    /// 用到的例句，UI 预览和切音频时用
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<Example>,
    /// 一条例句都没有。这种词做成卡片没什么意义，UI 该提示。
    pub no_examples: bool,
    /// 一条释义都查不到
    pub no_definitions: bool,
}

impl Card {
    /// 转成 AnkiConnect 要的字段表。
    pub fn fields(&self) -> BTreeMap<String, String> {
        [
            ("Expression", &self.expression),
            ("Reading", &self.reading),
            ("Meaning", &self.meaning),
            ("Sentence", &self.sentence),
            ("SentenceAudio", &self.sentence_audio),
            ("Source", &self.source),
            ("JLPT", &self.jlpt),
            ("Pitch", &self.pitch),
            ("Freq", &self.freq),
            ("PartOfSpeech", &self.part_of_speech),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CardOptions {
    /// 每张卡放几条例句
    pub max_examples: usize,
    /// 释义最多取前几部词典。`None` 表示不限。
    ///
    /// **不限是有代价的**：库里启用了 19 部词典，每部最多 8 条，
    /// 实测一个常用词的 Meaning 字段能到 17,000 字符——卡片背面
    /// 根本读不完。默认不限是为了和 PyQt 版行为一致，
    /// 但界面上给了这个旋钮，因为多数人会想调。
    pub max_dicts: Option<usize>,
}

impl Default for CardOptions {
    fn default() -> Self {
        Self {
            max_examples: 2,
            max_dicts: None,
        }
    }
}

/// 卡片上和词本身有关的字段：读音、释义、JLPT、音高、词频、词性。
///
/// 导出新卡和刷新旧卡用的是**同一份**，保证两条路写出来的内容一样。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WordFields {
    pub reading: String,
    pub meaning: String,
    pub jlpt: String,
    pub pitch: String,
    pub freq: String,
    pub part_of_speech: String,
    /// 实际查到释义用的形
    pub lookup_term: String,
    pub no_definitions: bool,
}

impl WordFields {
    /// 刷新旧卡要改的字段。**不含** Expression / Sentence / SentenceAudio / Source——
    /// 例句和音频是用户复习过的语境，刷新不碰。和 Python 的更新、刷新线程写的字段一致。
    pub fn update_map(&self) -> BTreeMap<String, String> {
        [
            ("Reading", &self.reading),
            ("Meaning", &self.meaning),
            ("JLPT", &self.jlpt),
            ("Pitch", &self.pitch),
            ("Freq", &self.freq),
            ("PartOfSpeech", &self.part_of_speech),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
    }
}

/// 查一个词的词典字段。`max_dicts` 为 `None` 表示不截断词典数。
pub fn word_fields(
    conn: &Connection,
    lemma: &str,
    upos: &str,
    max_dicts: Option<usize>,
) -> Result<WordFields> {
    // 词元查不到时换几个形查（勉強し → 勉強する、YOU → you）。和 Python 的导出线程一致
    let found = crate::dict::lookup_with_candidates(conn, lemma, upos, "")?;
    let mut definitions = found.definitions;
    if let Some(limit) = max_dicts {
        definitions = take_first_dicts(definitions, limit);
    }
    // 音高、词频：词元查不到时用实际查到释义的那个形再查一次
    let mut pitch = crate::dict::lookup_pitch(conn, lemma, &found.reading)?;
    if found.term != lemma {
        let alt = crate::dict::lookup_pitch(conn, &found.term, &found.reading)?;
        if !alt.is_empty() {
            pitch = alt;
        }
    }
    let mut freq = crate::dict::lookup_freq(conn, lemma)?;
    if freq.is_empty() && found.term != lemma {
        freq = crate::dict::lookup_freq(conn, &found.term)?;
    }
    Ok(WordFields {
        meaning: prefix_lookup_term(&meaning_html(&definitions), lemma, &found.term),
        no_definitions: definitions.is_empty(),
        jlpt: crate::dict::lookup_jlpt(conn, lemma)?,
        pitch,
        freq,
        part_of_speech: pos_label(upos).to_string(),
        reading: found.reading,
        lookup_term: found.term,
    })
}

/// 组装一张卡片。
pub fn build(
    conn: &Connection,
    lemma: &str,
    upos: &str,
    song_ids: &[String],
    options: CardOptions,
) -> Result<Card> {
    let fields = word_fields(conn, lemma, upos, options.max_dicts)?;
    let examples = examples(conn, lemma, options.max_examples, song_ids)?;

    // 出处去重且保序：同一首歌两句不该写两遍歌名
    let mut sources: Vec<String> = Vec::new();
    for example in &examples {
        let label = example.source_label();
        if !sources.contains(&label) {
            sources.push(label);
        }
    }

    Ok(Card {
        expression: lemma.to_string(),
        reading: fields.reading,
        meaning: fields.meaning,
        sentence: sentence_html(&examples, lemma),
        sentence_audio: String::new(),
        source: sources.join("、"),
        jlpt: fields.jlpt,
        pitch: fields.pitch,
        freq: fields.freq,
        part_of_speech: fields.part_of_speech,
        lookup_term: fields.lookup_term,
        no_examples: examples.is_empty(),
        no_definitions: fields.no_definitions,
        examples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn lookup_candidates_follow_the_python_rules() {
        // サ変连用形：「勉強する」排在前面
        assert_eq!(crate::dict::lookup_candidates("勉強し", "VERB", "勉強し"), vec!["勉強する", "勉強し"]);
        // 两个字的「探し」不当サ変，造不出「探する」
        assert_eq!(crate::dict::lookup_candidates("探し", "VERB", ""), vec!["探し"]);
        // 形容词连用形
        assert_eq!(crate::dict::lookup_candidates("早く", "ADJ", "早く"), vec!["早く", "早い"]);
        // よく → よい / 良い / いい
        assert_eq!(
            crate::dict::lookup_candidates("よく", "ADV", "よく"),
            vec!["よく", "よい", "良い", "いい"]
        );
    }

    #[test]
    fn a_meaning_found_under_another_form_says_so_on_top() {
        let meaning = "<div class='defs'><div class='dict-group'></div></div>";
        assert_eq!(prefix_lookup_term(meaning, "勉強し", "勉強し"), meaning);
        assert_eq!(
            prefix_lookup_term(meaning, "勉強し", "勉強する"),
            "<div class='defs'><div class='ym-term-ref'>辞書形：勉強する</div><div class='dict-group'></div></div>"
        );
    }

    #[test]
    fn plain_definitions_render_exactly_like_python() {
        // 对账里 437 个词差在这：全角数字开头的义项号
        assert_eq!(
            render_plain("１コップ 名 ガラス"),
            "<div class='ym-sense'><span class='ym-index'>１</span><span class='ym-gloss'>コップ 名 ガラス</span></div>"
        );
        // 44 个：【词形】前缀 + 圆圈数字
        assert_eq!(
            render_plain("【癖】① 無意識"),
            "<div class='ym-term-ref'>【癖】</div><div class='ym-sense'><span class='ym-index'>①</span><span class='ym-gloss'>無意識</span></div>"
        );
        assert_eq!(
            render_plain("2〔名〕本文"),
            "<div class='ym-sense'><span class='ym-index'>2</span><span class='ym-tag'>〔名〕</span><span class='ym-gloss'>本文</span></div>"
        );
        assert_eq!(
            render_plain("[注] 本文"),
            "<div class='ym-note'><span class='ym-note-label'>[注]</span><span class='ym-note-body'>本文</span></div>"
        );
        // 【词形】后面不是义项号：整行当释义
        assert_eq!(
            render_plain("【蓬】蓬生"),
            "<div class='ym-sense'><span class='ym-gloss'>【蓬】蓬生</span></div>"
        );
        // 先折叠空白再转义
        assert_eq!(
            render_plain("a\u{3000}\n <b>"),
            "<div class='ym-sense'><span class='ym-gloss'>a &lt;b&gt;</span></div>"
        );
    }

    #[test]
    fn no_definitions_still_writes_the_empty_wrapper_like_python() {
        assert_eq!(meaning_html(&[]), "<div class='defs'></div>");
    }

    #[test]
    fn md5_matches_the_reference_vectors() {
        assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(&md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            hex(&md5(b"The quick brown fox jumps over the lazy dog")),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
        // 刚好跨一个 64 字节块边界
        assert_eq!(hex(&md5(&[b'a'; 56])), "3b0c8ac703f828b04c6c197006d17218");
    }

    #[test]
    fn dictionary_header_colors_match_the_python_cards() {
        // 对账里抽出来的真实样本：Python 生成的卡片上这部词典是 #2471a3
        let (class, style) = header_style("明鏡日汉双解辞典");
        assert_eq!(class, "dict-hdr-other");
        assert_eq!(style, " style='background:#2471a3'");
        // 两部老词典走固定配色，不带 style
        assert_eq!(header_style("明鏡"), ("dict-hdr-明鏡".to_string(), String::new()));
    }

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE songs (id TEXT PRIMARY KEY, artist TEXT, title TEXT, audio_path TEXT);
             CREATE TABLE utterances (id INTEGER PRIMARY KEY, song_id TEXT, line_idx INTEGER,
                time_sec REAL, text TEXT);
             CREATE TABLE tokens (id INTEGER PRIMARY KEY, utterance_id INTEGER,
                token_idx INTEGER, surface TEXT, lemma TEXT, pos TEXT);
             CREATE TABLE dict_registry (name TEXT, zip_path TEXT, dict_type TEXT,
                enabled INTEGER, sort_order INTEGER);
             CREATE TABLE dict_terms (id INTEGER PRIMARY KEY, term TEXT, reading TEXT,
                dict_name TEXT, defs_json TEXT);
             CREATE TABLE yomitan_zh (term TEXT, reading TEXT, zh_defs TEXT);
             CREATE TABLE yomitan_pitch (term TEXT, reading TEXT, positions TEXT);
             CREATE TABLE yomitan_freq (term TEXT, freq INTEGER);
             CREATE TABLE jlpt_cache (lemma TEXT, level TEXT);

             INSERT INTO songs VALUES ('001','ヨルシカ','夜行','D:/a/001.flac'),
                                      ('002','YOASOBI','夜に駆ける','D:/a/002.flac');
             INSERT INTO utterances VALUES
                (1,'001',0,10.0,'夜が明ける'),
                (2,'001',1,20.0,'夜が明ける'),
                (3,'001',2,30.0,'長い夜だった'),
                (4,'002',0,64.5,'夜に駆けていく');
             INSERT INTO tokens VALUES
                (1,1,0,'夜','夜','NOUN'),
                (2,2,0,'夜','夜','NOUN'),
                (3,3,2,'夜','夜','NOUN'),
                (4,4,0,'夜','夜','NOUN');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn a_repeated_lyric_line_appears_once() {
        // 副歌重复的行在库里是两条 utterance，卡片上不该出现两遍
        let conn = db();
        let got = examples(&conn, "夜", 5, &[]).unwrap();
        let texts: Vec<&str> = got.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts.len(), texts.iter().collect::<std::collections::BTreeSet<_>>().len());
    }

    #[test]
    fn examples_prefer_different_songs() {
        // 按 id 直接排的话两条都来自 001，看不出这个词在别处怎么用
        let conn = db();
        let got = examples(&conn, "夜", 2, &[]).unwrap();
        assert_eq!(got.len(), 2);
        assert_ne!(got[0].song_id, got[1].song_id, "两条例句该来自不同的歌");
    }

    #[test]
    fn the_same_word_twice_gives_the_same_examples() {
        // Python 用 ORDER BY RANDOM()，重导一次卡片就变了，没法复现
        let conn = db();
        let a = examples(&conn, "夜", 2, &[]).unwrap();
        let b = examples(&conn, "夜", 2, &[]).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn a_song_filter_narrows_the_examples() {
        let conn = db();
        let got = examples(&conn, "夜", 5, &["002".to_string()]).unwrap();
        assert!(got.iter().all(|e| e.song_id == "002"), "{got:?}");
    }

    #[test]
    fn the_target_word_is_highlighted_once() {
        let conn = db();
        let got = examples(&conn, "夜", 1, &["002".to_string()]).unwrap();
        let html = sentence_html(&got, "夜");
        assert!(html.contains(r#"<b style="color:#c0392b">夜</b>"#), "{html}");
        assert_eq!(html.matches("<b style").count(), 1);
        // 出处和时间戳
        assert!(html.contains("YOASOBI「夜に駆ける」"), "{html}");
        assert!(html.contains("01:04.5"), "{html}");
    }

    #[test]
    fn lyrics_are_escaped_before_highlighting() {
        // 歌词里出现 < > & 不该破坏卡片模板
        let example = Example {
            song_id: "001".into(),
            artist: "A".into(),
            title: "T".into(),
            time_sec: Some(0.0),
            text: "<b>夜</b> & more".into(),
            surface: "夜".into(),
            audio_path: String::new(),
            end_sec: None,
            utterance_id: 0,
        };
        let html = sentence_html(std::slice::from_ref(&example), "夜");
        assert!(html.contains("&lt;b&gt;"), "{html}");
        assert!(html.contains("&amp;"), "{html}");
        // 高亮标签本身是我们加的，要保留
        assert!(html.contains(r#"<b style="color:#c0392b">夜</b>"#), "{html}");
    }

    fn line(text: &str, surface: &str) -> Example {
        Example {
            song_id: "001".into(),
            artist: "A".into(),
            title: "T".into(),
            time_sec: None,
            text: text.into(),
            surface: surface.into(),
            audio_path: String::new(),
            end_sec: None,
            utterance_id: 0,
        }
    }

    #[test]
    fn the_highlight_never_lands_inside_an_entity() {
        // 以前是先转义、再去转义后的文本里找词。「'」转成 `&#x27;` 之后，找「7」先碰到的是
        // 实体里的那个 7：高亮把实体拆成两半，卡片上显示出一串乱码，真正的那个词反而没标上
        let html = sentence_html(&[line("It's 7 o'clock", "7")], "7");
        assert!(html.contains(r#"It&#x27;s <b style="color:#c0392b">7</b> o&#x27;clock"#), "{html}");
        assert_eq!(html.matches("<b style").count(), 1);

        let html = sentence_html(&[line("R&B amp", "amp")], "amp");
        assert!(html.contains(r#"R&amp;B <b style="color:#c0392b">amp</b>"#), "{html}");

        // 词本身带要转义的字符：整个词在高亮里，转义的是词，不是高亮标签
        let html = sentence_html(&[line("say \"a<b\" again", "a<b")], "a<b");
        assert!(html.contains(r#"say &quot;<b style="color:#c0392b">a&lt;b</b>&quot; again"#), "{html}");
    }

    #[test]
    fn a_word_that_is_not_in_the_line_is_not_found_inside_an_entity() {
        // 这一行里既没有表层形也没有词典形：不高亮。以前会在 `&amp;` 里找到「amp」
        let html = sentence_html(&[line("A & B", "")], "amp");
        assert_eq!(html.matches("<b style").count(), 0, "{html}");
        assert!(html.contains("A &amp; B"), "{html}");
    }

    #[test]
    fn the_inflected_form_is_highlighted_not_the_lemma() {
        // 歌词里写的是「駆けて」，词典形是「駆ける」——高亮要跟着歌词
        let example = Example {
            song_id: "001".into(),
            artist: "A".into(),
            title: "T".into(),
            time_sec: None,
            text: "夜に駆けていく".into(),
            surface: "駆け".into(),
            audio_path: String::new(),
            end_sec: None,
            utterance_id: 0,
        };
        let html = sentence_html(std::slice::from_ref(&example), "駆ける");
        assert!(html.contains(r#"<b style="color:#c0392b">駆け</b>"#), "{html}");
    }

    #[test]
    fn html_definitions_are_not_escaped_but_plain_ones_are() {
        let defs = vec![
            Definition {
                source: "A".into(),
                text: "<div class='x'>排好版的</div>".into(),
                is_html: true,
            },
            Definition {
                source: "B".into(),
                text: "<script>纯文本".into(),
                is_html: false,
            },
        ];
        let html = meaning_html(&defs);
        assert!(html.contains("<div class='x'>排好版的</div>"), "{html}");
        assert!(html.contains("&lt;script&gt;纯文本"), "{html}");
        assert!(!html.contains("<script>"), "纯文本里的标签必须转义：{html}");
    }

    #[test]
    fn definitions_are_grouped_in_registry_order() {
        let defs = vec![
            Definition { source: "先".into(), text: "1".into(), is_html: false },
            Definition { source: "后".into(), text: "2".into(), is_html: false },
            Definition { source: "先".into(), text: "3".into(), is_html: false },
        ];
        let html = meaning_html(&defs);
        assert!(html.find("先").unwrap() < html.find("后").unwrap(), "{html}");
        // 同一部词典只出一次分组（"dict-hdr" 这个子串在
        // class='dict-hdr dict-hdr-other' 里会出现两次，不能拿它数）
        assert_eq!(html.matches("dict-group").count(), 2, "{html}");
        assert_eq!(html.matches("ym-item").count(), 3, "{html}");
    }

    #[test]
    fn a_dictionary_always_gets_the_same_colour() {
        // 用 DefaultHasher 的话每次启动颜色都不同，扫一眼认不出来源
        let a = header_style("某部词典");
        let b = header_style("某部词典");
        assert_eq!(a, b);
        assert!(a.1.contains("background:"), "{a:?}");
        // 有固定配色的两部不走散列
        assert_eq!(header_style("明鏡").1, "");
    }

    #[test]
    fn a_sense_marker_is_laid_out_separately() {
        let html = render_plain("①晚上");
        assert!(html.contains("<span class='ym-index'>①</span>"), "{html}");
        assert!(html.contains("晚上"), "{html}");
    }

    #[test]
    fn a_card_reports_when_it_has_nothing_to_show() {
        // 没例句也没释义的词做成卡片没意义，UI 要能提示
        let conn = db();
        let card = build(&conn, "不存在的词", "NOUN", &[], CardOptions::default()).unwrap();
        assert!(card.no_examples);
        assert!(card.no_definitions);
        assert_eq!(card.sentence, "");
    }

    #[test]
    fn a_built_card_fills_the_note_type_fields() {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO dict_registry VALUES ('D','z','terms',1,1);
             INSERT INTO dict_terms (term,reading,dict_name,defs_json)
                VALUES ('夜','よる','D','[\"晚上\"]');
             INSERT INTO yomitan_freq VALUES ('夜', 376);
             INSERT INTO jlpt_cache VALUES ('夜','n3');",
        )
        .unwrap();
        let card = build(&conn, "夜", "NOUN", &[], CardOptions::default()).unwrap();
        assert_eq!(card.expression, "夜");
        assert_eq!(card.reading, "よる");
        assert_eq!(card.jlpt, "N3");
        assert_eq!(card.freq, "376");
        assert_eq!(card.part_of_speech, "名詞");
        assert!(card.source.contains("ヨルシカ「夜行」"), "{}", card.source);
        assert!(!card.no_examples);

        // 字段名要和 note type 完全对上，少一个 Anki 那边就是空的
        let fields = card.fields();
        for name in crate::model::FIELDS {
            assert!(fields.contains_key(*name), "缺字段 {name}");
        }
        assert_eq!(fields.len(), crate::model::FIELDS.len());
    }

    #[test]
    fn limiting_dictionaries_keeps_the_top_ranked_ones() {
        // 19 部词典全上的话一个常用词的释义能到 17000 字符，读不完
        let defs = vec![
            Definition { source: "第一".into(), text: "a".into(), is_html: false },
            Definition { source: "第二".into(), text: "b".into(), is_html: false },
            Definition { source: "第一".into(), text: "c".into(), is_html: false },
            Definition { source: "第三".into(), text: "d".into(), is_html: false },
        ];
        let kept = take_first_dicts(defs.clone(), 2);
        assert_eq!(kept.len(), 3, "前两部词典的三条都要留下");
        assert!(kept.iter().all(|d| d.source != "第三"));
        // 0 表示不要释义
        assert!(take_first_dicts(defs.clone(), 0).is_empty());
        // 上限大于实际部数时原样返回
        assert_eq!(take_first_dicts(defs.clone(), 99).len(), 4);
    }

    #[test]
    fn two_examples_from_one_song_name_it_once() {
        let conn = db();
        let card = build(
            &conn,
            "夜",
            "NOUN",
            &["001".to_string()],
            CardOptions { max_examples: 2, ..Default::default() },
        )
        .unwrap();
        assert_eq!(card.source, "ヨルシカ「夜行」");
    }
}
