//! CQL 检索：按词的属性组合条件找词串（Sketch Engine / CQP 的 Corpus Query Language 的一个子集）。
//!
//! ```text
//! [lemma="君"] [pos="ADP"]              君 后面跟一个助词
//! [pos="VERB" & cform="命令形.*"]       所有命令形的动词
//! [lemma="て"] []{0,2} [lemma="しまう"]  て……しまう，中间最多隔两个词
//! "夢" [pos!="PUNCT"]+                  「夢」后面跟一个或几个非标点的词
//! [reading="きみ"]                      读音是キミ的词（平假名会换成片假名）
//! ```
//!
//! ## 语法
//!
//! - 一个词：`[条件]`；`[]` 是任意一个词；单独一个带引号的字符串 `"君"` 等于 `[word="君"]`。
//! - 条件：`属性="值"`、`属性!="值"`，用 `&`（且）、`|`（或）、`!`（非）、括号组合，`&` 比 `|` 先算。
//! - 属性：`word`（表层形）、`lemma`（辞书形）、`pos`（UPOS：NOUN、VERB…）、`tag`（日语词性：`名詞-普通名詞-一般`）、
//!   `ctype`（活用型）、`cform`（活用形）、`reading`（读音）、`norm`（正规化表记）。
//! - 值是**正则表达式，整个匹配**（和 CQL 一样）：`cform="命令形.*"`、`lemma="会う|逢う"`。
//! - 重复：`?`（0~1）、`*`（0 个以上）、`+`（1 个以上）、`{n}`、`{m,n}`、`{m,}`。
//!
//! ## 怎么匹配
//!
//! 词串不跨行（歌词的行就是自然的单位），标点、符号也是词——`[]` 能匹配「、」。
//! 每一行从左到右找：在一个起点上取**最长**的匹配，下一次从这个匹配的结尾之后接着找（不重叠）。
//! 能匹配空串的查询（比如只有 `[]*`）不允许。
//!
//! 没标过形态信息的词（`tag` 等为空）只能被 `tag=""` 这类条件匹配上，正常查询自然不会命中。

use anyhow::{Result, bail};
use regex::Regex;
use rusqlite::{Connection, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::models::{KwicHit, to_katakana};
use crate::subcorpus::Subcorpus;

/// 词的属性
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attr {
    Word,
    Lemma,
    Pos,
    Tag,
    Ctype,
    Cform,
    Reading,
    Norm,
}

impl Attr {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "word" | "surface" => Attr::Word,
            "lemma" => Attr::Lemma,
            "pos" | "upos" => Attr::Pos,
            "tag" | "pos_ja" => Attr::Tag,
            "ctype" => Attr::Ctype,
            "cform" => Attr::Cform,
            "reading" => Attr::Reading,
            "norm" | "normalized" => Attr::Norm,
            _ => return None,
        })
    }
}

/// 一个词的全部属性
#[derive(Debug, Clone, Default)]
pub struct CqlToken {
    pub word: String,
    pub lemma: String,
    pub pos: String,
    pub tag: String,
    pub ctype: String,
    pub cform: String,
    pub reading: String,
    pub norm: String,
}

impl CqlToken {
    fn get(&self, attr: Attr) -> &str {
        match attr {
            Attr::Word => &self.word,
            Attr::Lemma => &self.lemma,
            Attr::Pos => &self.pos,
            Attr::Tag => &self.tag,
            Attr::Ctype => &self.ctype,
            Attr::Cform => &self.cform,
            Attr::Reading => &self.reading,
            Attr::Norm => &self.norm,
        }
    }
}

/// 一个词的条件
#[derive(Debug, Clone)]
pub enum Cond {
    Any,
    Test { attr: Attr, regex: Regex, negate: bool },
    Not(Box<Cond>),
    And(Vec<Cond>),
    Or(Vec<Cond>),
}

impl Cond {
    pub fn matches(&self, tok: &CqlToken) -> bool {
        match self {
            Cond::Any => true,
            Cond::Test { attr, regex, negate } => regex.is_match(tok.get(*attr)) != *negate,
            Cond::Not(c) => !c.matches(tok),
            Cond::And(cs) => cs.iter().all(|c| c.matches(tok)),
            Cond::Or(cs) => cs.iter().any(|c| c.matches(tok)),
        }
    }
}

/// 查询里的一项：一个词的条件 + 重复次数
#[derive(Debug, Clone)]
pub struct Item {
    pub cond: Cond,
    pub min: usize,
    /// None 是不设上限（实际受行长限制）
    pub max: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Query {
    pub items: Vec<Item>,
}

// ────────────────────────────── 解析 ──────────────────────────────

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn new(src: &str) -> Self {
        Self { chars: src.chars().collect(), pos: 0 }
    }

    fn fail<T>(&self, message: &str) -> Result<T> {
        bail!("CQL 第 {} 个字符附近：{message}", self.pos + 1)
    }

    fn skip_ws(&mut self) {
        while self.chars.get(self.pos).is_some_and(|c| c.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_ws();
        self.chars.get(self.pos).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn string(&mut self) -> Result<String> {
        let quote = match self.peek() {
            Some(q @ ('"' | '\'')) => q,
            _ => return self.fail("这里要一个带引号的值"),
        };
        self.pos += 1;
        let mut out = String::new();
        loop {
            match self.chars.get(self.pos) {
                None => return self.fail("引号没有结束"),
                Some(&c) if c == quote => {
                    self.pos += 1;
                    return Ok(out);
                }
                // 引号里的反斜杠原样留给正则（\. \d 之类），只有 \" 是转义引号
                Some('\\') if self.chars.get(self.pos + 1) == Some(&quote) => {
                    out.push(quote);
                    self.pos += 2;
                }
                Some(&c) => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
    }

    fn name(&mut self) -> String {
        self.skip_ws();
        let start = self.pos;
        while self.chars.get(self.pos).is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_') {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn number(&mut self) -> Option<usize> {
        self.skip_ws();
        let start = self.pos;
        while self.chars.get(self.pos).is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect::<String>().parse().ok()
    }

    fn regex(&self, attr: Attr, value: &str) -> Result<Regex> {
        let value = if attr == Attr::Reading { to_katakana(value) } else { value.to_string() };
        match Regex::new(&format!("^(?:{value})$")) {
            Ok(re) => Ok(re),
            Err(err) => self.fail(&format!("正则写错了：{err}")),
        }
    }

    // expr := term ('|' term)*
    fn expr(&mut self) -> Result<Cond> {
        let mut terms = vec![self.term()?];
        while self.eat('|') {
            terms.push(self.term()?);
        }
        Ok(if terms.len() == 1 { terms.pop().expect("至少一个") } else { Cond::Or(terms) })
    }

    // term := factor ('&' factor)*
    fn term(&mut self) -> Result<Cond> {
        let mut factors = vec![self.factor()?];
        while self.eat('&') {
            factors.push(self.factor()?);
        }
        Ok(if factors.len() == 1 { factors.pop().expect("至少一个") } else { Cond::And(factors) })
    }

    // factor := '!' factor | '(' expr ')' | attr ('='|'!=') string
    fn factor(&mut self) -> Result<Cond> {
        if self.eat('!') {
            return Ok(Cond::Not(Box::new(self.factor()?)));
        }
        if self.eat('(') {
            let inner = self.expr()?;
            if !self.eat(')') {
                return self.fail("缺少 )");
            }
            return Ok(inner);
        }
        let name = self.name();
        if name.is_empty() {
            return self.fail("这里要一个属性名，比如 lemma");
        }
        let Some(attr) = Attr::parse(&name) else {
            return self.fail(&format!("不认识的属性「{name}」，可用的有 word lemma pos tag ctype cform reading norm"));
        };
        let negate = if self.eat('!') {
            if !self.eat('=') {
                return self.fail("这里要 = 或 !=");
            }
            true
        } else if self.eat('=') {
            false
        } else {
            return self.fail("这里要 = 或 !=");
        };
        let value = self.string()?;
        let regex = self.regex(attr, &value)?;
        Ok(Cond::Test { attr, regex, negate })
    }

    fn quantifier(&mut self) -> Result<(usize, Option<usize>)> {
        Ok(match self.peek() {
            Some('?') => {
                self.pos += 1;
                (0, Some(1))
            }
            Some('*') => {
                self.pos += 1;
                (0, None)
            }
            Some('+') => {
                self.pos += 1;
                (1, None)
            }
            Some('{') => {
                self.pos += 1;
                let Some(min) = self.number() else { return self.fail("重复次数要写成 {n} 或 {m,n}") };
                let max = if self.eat(',') { self.number() } else { Some(min) };
                if !self.eat('}') {
                    return self.fail("重复次数要写成 {n} 或 {m,n}");
                }
                if max.is_some_and(|m| m < min) {
                    return self.fail("重复次数的上限比下限小");
                }
                (min, max)
            }
            _ => (1, Some(1)),
        })
    }

    fn query(&mut self) -> Result<Query> {
        let mut items = Vec::new();
        while let Some(c) = self.peek() {
            let cond = match c {
                '[' => {
                    self.pos += 1;
                    if self.eat(']') {
                        Cond::Any
                    } else {
                        let cond = self.expr()?;
                        if !self.eat(']') {
                            return self.fail("缺少 ]");
                        }
                        cond
                    }
                }
                '"' | '\'' => {
                    let value = self.string()?;
                    Cond::Test { attr: Attr::Word, regex: self.regex(Attr::Word, &value)?, negate: false }
                }
                _ => return self.fail("这里要 [ 或带引号的词"),
            };
            let (min, max) = self.quantifier()?;
            items.push(Item { cond, min, max });
        }
        if items.is_empty() {
            bail!("CQL 查询是空的");
        }
        if items.iter().all(|i| i.min == 0) {
            bail!("这个查询能匹配空串：至少要有一个必须出现的词");
        }
        Ok(Query { items })
    }
}

pub fn parse(src: &str) -> Result<Query> {
    Parser::new(src).query()
}

// ────────────────────────────── 匹配 ──────────────────────────────

impl Query {
    /// 从 `start` 开始最长能匹配到哪（不含），匹配不上是 None。空匹配不算
    pub fn longest_at(&self, toks: &[CqlToken], start: usize) -> Option<usize> {
        // 能到达的位置集合，逐项推进（NFA 模拟，按位置去重）
        let mut reach = vec![start];
        for item in &self.items {
            let mut next: Vec<usize> = Vec::new();
            for &p in &reach {
                // 从 p 起连续满足条件的词有几个
                let mut run = 0;
                while p + run < toks.len() && item.max.is_none_or(|m| run < m) && item.cond.matches(&toks[p + run]) {
                    run += 1;
                }
                if run >= item.min {
                    next.extend((item.min..=run).map(|k| p + k));
                }
            }
            next.sort_unstable();
            next.dedup();
            if next.is_empty() {
                return None;
            }
            reach = next;
        }
        reach.into_iter().filter(|&e| e > start).max()
    }

    /// 一行里所有不重叠的匹配：(起点, 长度)
    pub fn find_all(&self, toks: &[CqlToken]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < toks.len() {
            match self.longest_at(toks, i) {
                Some(end) => {
                    out.push((i, end - i));
                    i = end;
                }
                None => i += 1,
            }
        }
        out
    }
}

// ────────────────────────────── 检索 ──────────────────────────────

/// CQL 检索的参数
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CqlQuery {
    pub query: String,
    pub subcorpus: Subcorpus,
    /// 同一首歌里文本相同的行折叠成一条（和 KWIC 的去重同一规则）
    pub dedup: bool,
    pub cross_line: bool,
    pub limit: Option<i64>,
}

pub(crate) fn search(conn: &Connection, query: &CqlQuery) -> Result<Vec<KwicHit>> {
    let parsed = parse(&query.query)?;
    let (clause, params) = query.subcorpus.and_clause("u.song_id");
    // 顺序和 KWIC 一样：歌手、曲名、时间、行号
    let sql = format!(
        "SELECT u.song_id, s.artist, s.title, COALESCE(s.audio_path,''), u.id, u.line_idx, u.time_sec, u.text, \
                t.surface, t.lemma, COALESCE(t.pos,''), COALESCE(t.pos_ja,''), COALESCE(t.ctype,''), COALESCE(t.cform,''), \
                COALESCE(t.reading,''), COALESCE(t.normalized,'') \
         FROM tokens t JOIN utterances u ON u.id = t.utterance_id JOIN songs s ON s.id = u.song_id \
         WHERE 1{clause} \
         ORDER BY s.artist, s.title, u.song_id, u.time_sec, u.line_idx, u.id, t.token_idx"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;

    struct LineMeta {
        hit: crate::search::RawHit,
        toks: Vec<CqlToken>,
    }
    let mut lines: Vec<LineMeta> = Vec::new();
    while let Some(row) = rows.next()? {
        let utterance_id: i64 = row.get(4)?;
        if lines.last().map(|l| l.hit.utterance_id) != Some(utterance_id) {
            lines.push(LineMeta {
                hit: crate::search::RawHit {
                    song_id: row.get(0)?,
                    artist: row.get(1)?,
                    title: row.get(2)?,
                    audio_path: row.get(3)?,
                    utterance_id,
                    line_idx: row.get(5)?,
                    time_sec: row.get(6)?,
                    text: row.get(7)?,
                    token_idx: 0,
                    repeat_count: 1,
                    match_len: 1,
                },
                toks: Vec::new(),
            });
        }
        lines.last_mut().expect("刚 push 过").toks.push(CqlToken {
            word: row.get(8)?,
            lemma: row.get(9)?,
            pos: row.get(10)?,
            tag: row.get(11)?,
            ctype: row.get(12)?,
            cform: row.get(13)?,
            reading: row.get(14)?,
            norm: row.get(15)?,
        });
    }

    let mut raw = Vec::new();
    for line in &lines {
        for (start, len) in parsed.find_all(&line.toks) {
            let mut hit = line.hit.clone();
            hit.token_idx = start as i64;
            hit.match_len = len;
            raw.push(hit);
        }
    }
    if query.dedup {
        raw = crate::search::fold_repeats(raw);
    }
    if let Some(n) = query.limit.filter(|n| *n > 0) {
        raw.truncate(n as usize);
    }
    crate::search::finish(conn, raw, query.cross_line)
}

/// 解析一下，不检索：界面边输边提示语法错
pub fn check(src: &str) -> Result<()> {
    parse(src).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(word: &str, lemma: &str, pos: &str, cform: &str) -> CqlToken {
        CqlToken { word: word.into(), lemma: lemma.into(), pos: pos.into(), cform: cform.into(), ..Default::default() }
    }

    fn line() -> Vec<CqlToken> {
        // 君 が 泣い て しまっ た 、 走れ
        vec![
            tok("君", "君", "PRON", ""),
            tok("が", "が", "ADP", ""),
            tok("泣い", "泣く", "VERB", "連用形-イ音便"),
            tok("て", "て", "SCONJ", ""),
            tok("しまっ", "しまう", "VERB", "連用形-促音便"),
            tok("た", "た", "AUX", "終止形-一般"),
            tok("、", "、", "PUNCT", ""),
            tok("走れ", "走る", "VERB", "命令形"),
        ]
    }

    #[test]
    fn parses_and_matches_sequences() {
        let toks = line();
        let q = parse(r#"[lemma="君"] [pos="ADP"]"#).unwrap();
        assert_eq!(q.find_all(&toks), vec![(0, 2)]);
        let q = parse(r#"[pos="VERB" & cform="命令形.*"]"#).unwrap();
        assert_eq!(q.find_all(&toks), vec![(7, 1)]);
        // 中间最多隔两个词
        let q = parse(r#"[lemma="泣く"] []{0,2} [lemma="た"]"#).unwrap();
        assert_eq!(q.find_all(&toks), vec![(2, 4)]);
        let q = parse(r#"[lemma="泣く"] []{0,1} [lemma="た"]"#).unwrap();
        assert!(q.find_all(&toks).is_empty());
        // 裸字符串 = word，值是整匹配的正则
        assert_eq!(parse(r#""泣い|走れ""#).unwrap().find_all(&toks), vec![(2, 1), (7, 1)]);
        assert!(parse(r#""泣""#).unwrap().find_all(&toks).is_empty(), "整个匹配，不是包含");
    }

    #[test]
    fn longest_match_and_no_overlap() {
        let toks = line();
        // 非标点的词一个以上：被「、」断成两段
        let q = parse(r#"[pos!="PUNCT"]+"#).unwrap();
        assert_eq!(q.find_all(&toks), vec![(0, 6), (7, 1)]);
        let q = parse(r#"[pos="VERB"] [pos!="PUNCT"]*"#).unwrap();
        assert_eq!(q.find_all(&toks), vec![(2, 4), (7, 1)]);
    }

    #[test]
    fn boolean_operators_and_negation() {
        let toks = line();
        let q = parse(r#"[(pos="VERB" | pos="AUX") & !cform="連用形.*"]"#).unwrap();
        assert_eq!(q.find_all(&toks), vec![(5, 1), (7, 1)]);
        let q = parse(r#"[lemma!="君" & pos="PRON"]"#).unwrap();
        assert!(q.find_all(&toks).is_empty());
    }

    #[test]
    fn reading_values_are_converted_to_katakana() {
        let toks = vec![CqlToken { word: "君".into(), reading: "キミ".into(), ..Default::default() }];
        assert_eq!(parse(r#"[reading="きみ"]"#).unwrap().find_all(&toks), vec![(0, 1)]);
    }

    #[test]
    fn errors_say_where_and_what() {
        let err = |q: &str| parse(q).unwrap_err().to_string();
        assert!(err(r#"[lemma="君""#).contains("缺少 ]"));
        assert!(err(r#"[lema="君"]"#).contains("不认识的属性「lema」"));
        assert!(err(r#"[lemma="君]"#).contains("引号没有结束"));
        assert!(err(r#"[lemma="(君"]"#).contains("正则写错了"));
        assert!(err("[]*").contains("空串"));
        assert!(err(r#"[]{3,1} "a""#).contains("上限比下限小"));
        assert!(err("").contains("空的"));
    }
}
