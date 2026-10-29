//! 给 `tokens` 补日语形态信息：细分词性、活用型、活用形、读音、正规化表记（语料分析第二阶段）。
//!
//! ## 为什么不直接重新分词
//!
//! 存量的 token 有两种来源：GiNZA 建的（UPOS 是按上下文判的，和 Sudachi 查表的 UPOS 只有 90.55% 一致），
//! 以及用户校正过的。整首歌重新分词会把这两种都换掉——词频、搭配、存档的分析结果全跟着变。
//! 所以这里**只加信息不改数据**：surface / lemma / pos 一个字不动，新列按位置对齐填进去。
//!
//! ## 怎么对齐
//!
//! 1. 用 Sudachi 重新分析这一行原文，拿到每个词在原文里的字符区间；
//! 2. 库里每个 token 也按顺序在原文里找出区间；
//! 3. **区间和写法都一样**的，直接用这一行上下文里的分析结果（`aligned`）；
//! 4. 对不上的（GiNZA 切得不一样、用户改过切分、歌词改过），把这个 token 的写法单独分析一次，
//!    正好是一个词、写法一样的就用它（`fallback`，没有上下文，读音偶尔会不同）；
//! 5. 还是不行的标成「标不上」：`pos_ja = ''`，其余列留空（`unknown`）。
//!
//! `pos_ja` 为 NULL 表示还没标过，后台作业（jp-app 的 `annotate_backlog`）会找到它们。
//! 新导入、补分词时写 token 就带着这些列（`import::write_line_tokens`），不需要再对齐；
//! 分词校正套回去的 token 是没有这些列的，由导入 / 补分词最后调一次 [`annotate_song`] 补上。

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::Serialize;

/// 一个词的形态信息（新加的五列）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Morph {
    /// 日语词性的前四级，去掉 `*`，用 `-` 连：`名詞-普通名詞-一般`、`動詞-非自立可能`
    pub pos_ja: String,
    /// 活用型：`五段-カ行`、`形容詞`；不活用的词是空串
    pub ctype: String,
    /// 活用形：`連用形-促音便`、`命令形`；不活用的词是空串
    pub cform: String,
    /// 读音（片假名，Sudachi 的 `reading_form`）
    pub reading: String,
    /// 正规化表记（Sudachi 的 `normalized_form`）：キミ → 君、かわいい → 可愛い、逢う → 会う
    pub normalized: String,
}

impl Morph {
    pub fn of(token: &jp_tokenizer::Token) -> Self {
        let field = |i: usize| token.pos.get(i).filter(|p| p.as_str() != "*").cloned().unwrap_or_default();
        let pos_ja = token.pos.iter().take(4).filter(|p| p.as_str() != "*").cloned().collect::<Vec<_>>().join("-");
        Self { pos_ja, ctype: field(4), cform: field(5), reading: token.reading.clone(), normalized: token.normalized.clone() }
    }
}

/// 一次标注的结果
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotated {
    pub songs: usize,
    pub lines: usize,
    pub tokens: usize,
    /// 按位置对上了，用的是带上下文的分析
    pub aligned: usize,
    /// 对不上位置，把这个词单独分析得到的
    pub fallback: usize,
    /// 标不上
    pub unknown: usize,
}

impl Annotated {
    pub fn add(&mut self, other: &Annotated) {
        self.songs += other.songs;
        self.lines += other.lines;
        self.tokens += other.tokens;
        self.aligned += other.aligned;
        self.fallback += other.fallback;
        self.unknown += other.unknown;
    }
}

/// 一串写法在原文里依次出现的字节区间。找不到的是 None，游标不动
fn spans<'a>(text: &str, surfaces: impl Iterator<Item = &'a str>) -> Vec<Option<(usize, usize)>> {
    let mut cursor = 0;
    surfaces
        .map(|s| {
            if s.is_empty() {
                return None;
            }
            let found = text[cursor..].find(s)?;
            let start = cursor + found;
            cursor = start + s.len();
            Some((start, cursor))
        })
        .collect()
}

/// 给一行的 token 标注（整行都重写）。返回这一行的统计
pub fn annotate_utterance(conn: &Connection, utterance_id: i64, analyzer: &jp_tokenizer::Analyzer) -> Result<Annotated> {
    let text: String = conn.query_row("SELECT text FROM utterances WHERE id = ?1", params![utterance_id], |r| r.get(0))?;
    let existing: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, surface FROM tokens WHERE utterance_id = ?1 ORDER BY token_idx, id")?;
        stmt.query_map(params![utterance_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    let mut stats = Annotated::default();
    if existing.is_empty() {
        return Ok(stats);
    }
    stats.lines = 1;
    let analyzed = analyzer.analyze(&text).with_context(|| format!("分词失败：{text}"))?;
    let analyzed: Vec<&jp_tokenizer::Token> = analyzed.iter().filter(|t| !t.surface.trim().is_empty()).collect();
    let analyzed_spans = spans(&text, analyzed.iter().map(|t| t.surface.as_str()));
    let by_span: std::collections::HashMap<(usize, usize), &jp_tokenizer::Token> = analyzed_spans
        .iter()
        .zip(&analyzed)
        .filter_map(|(span, t)| span.map(|s| (s, *t)))
        .collect();
    let existing_spans = spans(&text, existing.iter().map(|(_, s)| s.as_str()));

    let mut update = conn.prepare_cached(
        "UPDATE tokens SET pos_ja = ?2, ctype = ?3, cform = ?4, reading = ?5, normalized = ?6 WHERE id = ?1",
    )?;
    for ((id, surface), span) in existing.iter().zip(existing_spans) {
        stats.tokens += 1;
        let in_context = span.and_then(|s| by_span.get(&s)).filter(|t| t.surface == *surface);
        let morph = match in_context {
            Some(t) => {
                stats.aligned += 1;
                Some(Morph::of(t))
            }
            None => {
                let alone = analyzer.analyze(surface).unwrap_or_default();
                let alone: Vec<_> = alone.iter().filter(|t| !t.surface.trim().is_empty()).collect();
                match alone.as_slice() {
                    [one] if one.surface == *surface => {
                        stats.fallback += 1;
                        Some(Morph::of(one))
                    }
                    _ => {
                        stats.unknown += 1;
                        None
                    }
                }
            }
        };
        match morph {
            Some(m) => update.execute(params![id, m.pos_ja, m.ctype, m.cform, m.reading, m.normalized])?,
            None => update.execute(params![id, "", None::<String>, None::<String>, None::<String>, None::<String>])?,
        };
    }
    Ok(stats)
}

/// 给一首歌里还没标过的行标注。**在一个事务里调**
pub fn annotate_song(conn: &Connection, song_id: &str, analyzer: &jp_tokenizer::Analyzer) -> Result<Annotated> {
    let lines: Vec<i64> = {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT u.id FROM utterances u JOIN tokens t ON t.utterance_id = u.id \
             WHERE u.song_id = ?1 AND t.pos_ja IS NULL ORDER BY u.id",
        )?;
        stmt.query_map(params![song_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    let mut total = Annotated::default();
    for id in lines {
        total.add(&annotate_utterance(conn, id, analyzer)?);
    }
    if total.lines > 0 {
        total.songs = 1;
    }
    Ok(total)
}

/// 还有没标过的 token 的歌（歌 id 升序）
pub fn songs_needing_annotation(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT u.song_id FROM tokens t JOIN utterances u ON u.id = t.utterance_id \
         WHERE t.pos_ja IS NULL ORDER BY u.song_id",
    )?;
    Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
}

/// 整个库补一遍（给命令行和测试用；界面那边是一首一个事务，见 jp-app 的 `annotate_backlog`）。**在一个事务里调**
pub fn annotate_missing(conn: &Connection, analyzer: &jp_tokenizer::Analyzer) -> Result<Annotated> {
    let mut total = Annotated::default();
    for song in songs_needing_annotation(conn)? {
        total.add(&annotate_song(conn, &song, analyzer)?);
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_follow_the_text_in_order_and_skip_what_is_missing() {
        let text = "夜はまだ 長い、夜";
        let got = spans(text, ["夜", "は", "まだ", "長い", "×", "夜"].into_iter());
        let len = |s: &str| s.len();
        let a = len("夜");
        let b = a + len("は");
        let c = b + len("まだ") + 1; // 空格
        assert_eq!(got[0], Some((0, a)));
        assert_eq!(got[1], Some((a, b)));
        assert_eq!(got[3], Some((c, c + len("長い"))));
        assert_eq!(got[4], None, "找不到的不动游标");
        // 最后的「夜」是第二个，不是开头那个
        assert_eq!(got[5], Some((text.len() - len("夜"), text.len())));
    }

    #[test]
    fn morph_joins_the_japanese_pos_and_drops_placeholders() {
        let t = jp_tokenizer::Token {
            surface: "走れ".into(),
            lemma: "走る".into(),
            normalized: "走る".into(),
            reading: "ハシレ".into(),
            pos: ["動詞", "一般", "*", "*", "五段-ラ行", "命令形"].map(String::from).to_vec(),
            upos: "VERB".into(),
        };
        let m = Morph::of(&t);
        assert_eq!(m.pos_ja, "動詞-一般");
        assert_eq!(m.ctype, "五段-ラ行");
        assert_eq!(m.cform, "命令形");
        let n = jp_tokenizer::Token { pos: ["代名詞", "*", "*", "*", "*", "*"].map(String::from).to_vec(), ..t };
        let m = Morph::of(&n);
        assert_eq!((m.pos_ja.as_str(), m.ctype.as_str(), m.cform.as_str()), ("代名詞", "", ""));
    }
}
