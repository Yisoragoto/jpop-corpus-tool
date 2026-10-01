//! 查词结果 → 卡片字段。Yomitan 制卡模板（`{expression}` `{glossary}` 这类标记）的移植。
//!
//! 移植自 Yomitan（GPL-3.0-or-later，Copyright (C) 2023-2026 Yomitan Authors）：
//! - `ext/data/templates/default-anki-field-templates.handlebars`：每个标记对应的内联模板，逐个手译
//! - `ext/js/data/anki-note-data-creator.js`：模板用到的数据（词频、调核分组、例句挖空……）
//! - `ext/js/templates/anki-template-renderer.js`：`furigana`、`furiganaPlain`、`pitchCategories` 等 helper
//! - `ext/js/dictionary/dictionary-data-util.js`：`getGroupedPronunciations`
//!
//! 输出和 Yomitan 的 `test/data/anki-note-builder-test-results.json` 逐字对账（`tests/yomitan_anki_fields.rs`）。
//!
//! 和 Yomitan 有意不同的一处：词典样式的作用域。Yomitan 在浏览器里用 CSSOM 把选择器逐条加前缀，
//! 解析失败时退回 CSS 嵌套写法（`.yomitan-glossary {…}`）。这里没有 CSS 解析器，始终用嵌套写法——
//! 也就是 Yomitan 自己测试里的输出。Anki 25.02 起的桌面版、较新的 AnkiDroid / AnkiMobile 都支持。

use std::collections::HashMap;

use serde_json::Value;

use super::dom::js_number;
use super::structured::{MediaResolver, escape, format_glossary, pronunciation_position};
use crate::furigana::{distribute_furigana, distribute_furigana_inflected, is_non_noun_verb_or_adjective, pitch_category};
use crate::translator::{Pronunciation, Tag, TermDictionaryEntry, TermSource};

/// Yomitan 的查词结果模式决定模板里的 `definition.type`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// split 模式：一个词条一本词典
    Term,
    /// group / term 模式：一个词条合并多本词典
    TermGrouped,
}

pub struct NoteContext<'a> {
    /// 例句全文
    pub sentence: &'a str,
    /// 查词起点在例句里的位置（按字符）
    pub offset: usize,
    pub document_title: &'a str,
    /// 词典名 → 该词典自带的 styles.css
    pub dictionary_styles: &'a HashMap<String, String>,
}

pub struct NoteRenderer<'a> {
    entry: &'a TermDictionaryEntry,
    kind: EntryKind,
    ctx: &'a NoteContext<'a>,
}

fn scope_css(css: &str, scope: &str) -> String {
    format!("{scope} {{{css}\n}}")
}

fn glossary_scoped(css: &str) -> String {
    scope_css(css, ".yomitan-glossary")
}

fn dictionary_scoped(css: &str, dictionary: &str) -> String {
    let escaped = dictionary.replace('\\', "\\\\").replace('"', "\\\"");
    glossary_scoped(&scope_css(css, &format!("[data-dictionary=\"{escaped}\"]")))
}

/// `{{furigana expression reading}}`
fn furigana_html(expression: &str, reading: &str) -> String {
    distribute_furigana(expression, reading)
        .into_iter()
        .map(|s| if s.reading.is_empty() { s.text } else { format!("<ruby>{}<rt>{}</rt></ruby>", s.text, s.reading) })
        .collect()
}

/// `{{furiganaPlain ...}}`（helper 返回普通字符串，Handlebars 会再转义一次）
fn furigana_plain(expression: &str, reading: &str) -> String {
    let mut out = String::new();
    for s in distribute_furigana(expression, reading) {
        if s.reading.is_empty() {
            out.push_str(&s.text);
        } else {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&format!("{}[{}]", s.text, s.reading));
        }
    }
    escape(&out)
}

fn unique<'b>(items: impl Iterator<Item = &'b str>) -> Vec<&'b str> {
    let mut out: Vec<&str> = Vec::new();
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// 一本词典里合并后的发音：（发音, 读音, 用这个发音的词形们）
type GroupedPronunciations<'b> = Vec<(&'b str, Vec<(&'b Pronunciation, &'b str, Vec<&'b str>)>)>;

struct PitchItem<'b> {
    reading: &'b str,
    positions: &'b Value,
    exclusive_terms: Vec<&'b str>,
    exclusive_readings: Vec<&'b str>,
}

/// 词频数字：`displayValue` 开头的整数优先，否则用 `frequency`；同一本词典只取第一个
fn frequency_numbers(entry: &TermDictionaryEntry, mode: Option<&str>) -> Vec<f64> {
    let mut previous: Option<&str> = None;
    let mut out = Vec::new();
    for f in &entry.frequencies {
        let wrong_mode = mode.is_some_and(|m| f.frequency_mode.as_deref().is_some_and(|fm| !fm.is_empty() && fm != m));
        if previous == Some(f.dictionary.as_str()) || wrong_mode {
            continue;
        }
        previous = Some(&f.dictionary);
        if let Some(display) = &f.display_value {
            let digits: String = display.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(parsed) = digits.parse::<f64>()
                && parsed > 0.0
            {
                out.push(parsed);
                continue;
            }
        }
        if f.frequency > 0.0 {
            out.push(f.frequency);
        }
    }
    out
}

fn frequency_harmonic(entry: &TermDictionaryEntry, mode: Option<&str>) -> f64 {
    let numbers = frequency_numbers(entry, mode);
    if numbers.is_empty() {
        return -1.0;
    }
    let total: f64 = numbers.iter().map(|n| 1.0 / n).sum();
    (numbers.len() as f64 / total).floor()
}

fn frequency_average(entry: &TermDictionaryEntry, mode: Option<&str>) -> f64 {
    let numbers = frequency_numbers(entry, mode);
    if numbers.is_empty() {
        return -1.0;
    }
    (numbers.iter().sum::<f64>() / numbers.len() as f64).floor()
}

fn pronunciations_equivalent(a: &Pronunciation, b: &Pronunciation) -> bool {
    let tags_equal = |x: &[Tag], y: &[Tag]| {
        x.len() == y.len() && x.iter().zip(y).all(|(t1, t2)| t1.name == t2.name && t1.dictionaries == t2.dictionaries)
    };
    match (a, b) {
        (
            Pronunciation::PitchAccent { positions: p1, nasal_positions: n1, devoice_positions: d1, tags: t1 },
            Pronunciation::PitchAccent { positions: p2, nasal_positions: n2, devoice_positions: d2, tags: t2 },
        ) => tags_equal(t1, t2) && p1 == p2 && n1 == n2 && d1 == d2,
        (
            Pronunciation::PhoneticTranscription { ipa: i1, tags: t1 },
            Pronunciation::PhoneticTranscription { ipa: i2, tags: t2 },
        ) => tags_equal(t1, t2) && i1 == i2,
        _ => false,
    }
}

impl<'a> NoteRenderer<'a> {
    pub fn new(entry: &'a TermDictionaryEntry, kind: EntryKind, ctx: &'a NoteContext<'a>) -> Self {
        Self { entry, kind, ctx }
    }

    fn unique_terms(&self) -> Vec<&'a str> {
        unique(self.entry.headwords.iter().map(|h| h.term.as_str()))
    }

    fn unique_readings(&self) -> Vec<&'a str> {
        unique(self.entry.headwords.iter().map(|h| h.reading.as_str()))
    }

    fn dictionary_names(&self) -> Vec<&'a str> {
        unique(self.entry.definitions.iter().map(|d| d.dictionary.as_str()))
    }

    fn dictionary_aliases(&self) -> Vec<&'a str> {
        unique(self.entry.definitions.iter().map(|d| d.dictionary_alias.as_str()))
    }

    fn primary_source(&self) -> Option<&'a TermSource> {
        self.entry.headwords.iter().flat_map(|h| &h.sources).find(|s| s.is_primary)
    }

    /// 渲染一个标记（不带花括号）。不认识的标记返回 `None`。
    pub fn render(&self, marker: &str, media: &mut MediaResolver<'_>) -> Option<String> {
        let first = |v: Vec<&str>| v.first().map(|s| escape(s)).unwrap_or_default();
        Some(match marker {
            "expression" => first(self.unique_terms()),
            "reading" => first(self.unique_readings()),
            "furigana" => {
                let (terms, readings) = (self.unique_terms(), self.unique_readings());
                furigana_html(terms.first().copied().unwrap_or(""), readings.first().copied().unwrap_or(""))
            }
            "furigana-plain" => {
                let (terms, readings) = (self.unique_terms(), self.unique_readings());
                furigana_plain(terms.first().copied().unwrap_or(""), readings.first().copied().unwrap_or(""))
            }
            "glossary" => self.glossary(None, false, false, media),
            "glossary-brief" => self.glossary(None, true, false, media),
            "glossary-no-dictionary" => self.glossary(None, false, true, media),
            "glossary-first" => self.glossary_first(false, false, media),
            "glossary-first-brief" => self.glossary_first(true, false, media),
            "glossary-first-no-dictionary" => self.glossary_first(false, true, media),
            "cloze-prefix" => self.cloze().prefix,
            "cloze-body" => self.cloze().body,
            "cloze-body-kana" => self.cloze().body_kana,
            "cloze-suffix" => self.cloze().suffix,
            "sentence" => self.cloze().sentence,
            "conjugation" => self.conjugation(),
            "dictionary" => first(self.dictionary_names()),
            "dictionary-alias" => first(self.dictionary_aliases()),
            "document-title" => escape(self.ctx.document_title),
            "frequencies" => self.frequencies(None),
            "frequency-harmonic-rank" => self.rank(frequency_harmonic(self.entry, Some("rank-based")), "9999999"),
            "frequency-harmonic-occurrence" => self.rank(frequency_harmonic(self.entry, Some("occurrence-based")), "0"),
            "frequency-average-rank" => self.rank(frequency_average(self.entry, Some("rank-based")), "9999999"),
            "frequency-average-occurrence" => self.rank(frequency_average(self.entry, Some("occurrence-based")), "0"),
            "pitch-accent-positions" => self.pitch_accent_positions(),
            "pitch-accent-categories" => self.pitch_accent_categories(),
            _ => return None,
        })
    }

    /// `{single-glossary-<词典>}`：只取一本词典的释义（Yomitan 按词典动态生成的标记）
    pub fn single_glossary(&self, dictionary: &str, brief: bool, no_dictionary_tag: bool, media: &mut MediaResolver<'_>) -> String {
        self.glossary(Some(dictionary), brief, no_dictionary_tag, media)
    }

    fn rank(&self, value: f64, missing: &str) -> String {
        if value == -1.0 { missing.to_owned() } else { js_number(value) }
    }

    /// 内联模板 `glossary-single`
    #[allow(clippy::too_many_arguments)]
    fn glossary_single(
        &self,
        dictionary: &str,
        dictionary_alias: &str,
        tags: &[&Tag],
        glossary: &[&Value],
        brief: bool,
        no_dictionary_tag: bool,
        media: &mut MediaResolver<'_>,
    ) -> String {
        let mut out = String::new();
        if !brief {
            let mut any = false;
            for tag in tags {
                out.push_str(if any { ", " } else { "<i>(" });
                out.push_str(&escape(&tag.name));
                any = true;
            }
            if !no_dictionary_tag {
                out.push_str(if any { ", " } else { "<i>(" });
                out.push_str(&escape(dictionary_alias));
                any = true;
            }
            if any {
                out.push_str(")</i> ");
            }
        }
        if glossary.len() <= 1 {
            for g in glossary {
                out.push_str(&format_glossary(dictionary, g, media));
            }
        } else {
            out.push_str("<ul>");
            for g in glossary {
                out.push_str("<li>");
                out.push_str(&format_glossary(dictionary, g, media));
                out.push_str("</li>");
            }
            out.push_str("</ul>");
        }
        out
    }

    fn styles_of(&self, dictionary: &str) -> Option<&'a String> {
        self.ctx.dictionary_styles.get(dictionary).filter(|s| !s.is_empty())
    }

    fn glossary(&self, selected: Option<&str>, brief: bool, no_dictionary_tag: bool, media: &mut MediaResolver<'_>) -> String {
        let mut out = String::from(r#"<div style="text-align: left;" class="yomitan-glossary">"#);
        match self.kind {
            EntryKind::Term => {
                let names = self.dictionary_names();
                let dictionary = names.first().copied().unwrap_or("");
                if selected.is_none_or(|s| s == dictionary) {
                    let aliases = self.dictionary_aliases();
                    let tags: Vec<&Tag> = self.entry.definitions.iter().flat_map(|d| &d.tags).collect();
                    let glossary: Vec<&Value> = self.entry.definitions.iter().flat_map(|d| &d.entries).collect();
                    out.push_str(&self.glossary_single(
                        dictionary,
                        aliases.first().copied().unwrap_or(""),
                        &tags,
                        &glossary,
                        brief,
                        no_dictionary_tag,
                        media,
                    ));
                    let styles: String = self
                        .entry
                        .definitions
                        .iter()
                        .filter_map(|d| self.styles_of(&d.dictionary))
                        .map(|css| glossary_scoped(css))
                        .collect();
                    if !styles.is_empty() {
                        out.push_str(&format!("<style>{styles}</style>"));
                    }
                }
            }
            EntryKind::TermGrouped => {
                out.push_str("<ol>");
                for d in &self.entry.definitions {
                    if selected.is_some_and(|s| s != d.dictionary) {
                        continue;
                    }
                    out.push_str(&format!(r#"<li data-dictionary="{}">"#, escape(&d.dictionary)));
                    let tags: Vec<&Tag> = d.tags.iter().collect();
                    let glossary: Vec<&Value> = d.entries.iter().collect();
                    out.push_str(&self.glossary_single(
                        &d.dictionary,
                        &d.dictionary_alias,
                        &tags,
                        &glossary,
                        brief,
                        no_dictionary_tag,
                        media,
                    ));
                    out.push_str("</li>");
                    if let Some(css) = self.styles_of(&d.dictionary) {
                        out.push_str(&format!("<style>{}</style>", dictionary_scoped(css, &d.dictionary)));
                    }
                }
                out.push_str("</ol>");
            }
        }
        out.push_str("</div>");
        out
    }

    fn glossary_first(&self, brief: bool, no_dictionary_tag: bool, media: &mut MediaResolver<'_>) -> String {
        match self.kind {
            EntryKind::Term => self.glossary(None, brief, no_dictionary_tag, media),
            EntryKind::TermGrouped => {
                let mut out = String::from(r#"<div style="text-align: left;" class="yomitan-glossary">"#);
                if let Some(d) = self.entry.definitions.first() {
                    let tags: Vec<&Tag> = d.tags.iter().collect();
                    let glossary: Vec<&Value> = d.entries.iter().collect();
                    out.push_str(&self.glossary_single(
                        &d.dictionary,
                        &d.dictionary_alias,
                        &tags,
                        &glossary,
                        brief,
                        no_dictionary_tag,
                        media,
                    ));
                    if let Some(css) = self.styles_of(&d.dictionary) {
                        out.push_str(&format!("<style>{}</style>", glossary_scoped(css)));
                    }
                }
                out.push_str("</div>");
                out
            }
        }
    }

    fn frequencies(&self, selected: Option<&str>) -> String {
        if self.entry.frequencies.is_empty() {
            return String::new();
        }
        let show_headword = self.unique_terms().len() > 1 || self.unique_readings().len() > 1;
        let mut out = String::from(r#"<ul style="text-align: left;">"#);
        for f in &self.entry.frequencies {
            if selected.is_some_and(|s| s != f.dictionary) {
                continue;
            }
            out.push_str("<li>");
            if show_headword && let Some(h) = self.entry.headwords.get(f.headword_index) {
                out.push_str(&format!("({}) ", furigana_html(&h.term, &h.reading)));
            }
            let value = f.display_value.clone().unwrap_or_else(|| js_number(f.frequency));
            out.push_str(&format!("{}: {}", escape(&f.dictionary_alias), escape(&value)));
            out.push_str("</li>");
        }
        out.push_str("</ul>");
        out
    }

    fn conjugation(&self) -> String {
        let chains = &self.entry.inflection_rule_chain_candidates;
        if chains.is_empty() {
            return String::new();
        }
        let multiple = chains.len() > 1;
        let mut out = String::new();
        if multiple {
            out.push_str("<ul>");
        }
        for chain in chains {
            if chain.inflection_rules.is_empty() {
                continue;
            }
            if multiple {
                out.push_str("<li>");
            }
            let names: Vec<String> = chain.inflection_rules.iter().map(|r| escape(&r.name)).collect();
            out.push_str(&names.join(" « "));
            if multiple {
                out.push_str("</li>");
            }
        }
        if multiple {
            out.push_str("</ul>");
        }
        out
    }

    /// JS `getGroupedPronunciations` 只取调核
    fn pitches(&self) -> Vec<PitchItem<'a>> {
        let headwords = &self.entry.headwords;
        let all_terms = unique(headwords.iter().map(|h| h.term.as_str()));
        let all_readings = unique(headwords.iter().map(|h| h.reading.as_str()));
        // 词典 → [(发音, 读音, 词形们)]，保持首次出现的顺序
        let mut groups: GroupedPronunciations<'a> = Vec::new();
        for tp in &self.entry.pronunciations {
            let Some(h) = headwords.get(tp.headword_index) else { continue };
            let index = match groups.iter().position(|(d, _)| *d == tp.dictionary) {
                Some(i) => i,
                None => {
                    groups.push((&tp.dictionary, Vec::new()));
                    groups.len() - 1
                }
            };
            let list = &mut groups[index].1;
            for p in &tp.pronunciations {
                let existing = list.iter_mut().find(|(p2, reading, _)| *reading == h.reading && pronunciations_equivalent(p2, p));
                match existing {
                    Some((_, _, terms)) => {
                        if !terms.contains(&h.term.as_str()) {
                            terms.push(&h.term);
                        }
                    }
                    None => list.push((p, &h.reading, vec![&h.term])),
                }
            }
        }
        let mut out = Vec::new();
        for (_, list) in groups {
            for (p, reading, terms) in list {
                let Pronunciation::PitchAccent { positions, .. } = p else { continue };
                let same_terms = terms.len() == all_terms.len() && terms.iter().all(|t| all_terms.contains(t));
                let exclusive_terms = if same_terms { Vec::new() } else { terms.iter().copied().filter(|t| all_terms.contains(t)).collect() };
                let exclusive_readings = if all_readings.len() > 1 { vec![reading] } else { Vec::new() };
                out.push(PitchItem { reading, positions, exclusive_terms, exclusive_readings });
            }
        }
        out
    }

    fn pitch_accent_positions(&self) -> String {
        let pitches = self.pitches();
        let count = pitches.len();
        if count == 0 {
            return String::new();
        }
        let mut out = String::new();
        if count > 1 {
            out.push_str("<ol>");
        }
        for pitch in &pitches {
            if count > 1 {
                out.push_str("<li>");
            }
            let exclusive: Vec<&str> =
                pitch.exclusive_terms.iter().chain(&pitch.exclusive_readings).copied().collect();
            if !exclusive.is_empty() {
                out.push_str(&format!("<em>({} only) </em>", exclusive.concat()));
            }
            out.push_str(&pronunciation_position(pitch.reading, pitch.positions));
            if count > 1 {
                out.push_str("</li>");
            }
        }
        if count > 1 {
            out.push_str("</ol>");
        }
        out
    }

    fn pitch_accent_categories(&self) -> String {
        let mut categories: Vec<&str> = Vec::new();
        for tp in &self.entry.pronunciations {
            let Some(h) = self.entry.headwords.get(tp.headword_index) else { continue };
            let verb = is_non_noun_verb_or_adjective(&h.word_classes);
            for p in &tp.pronunciations {
                if let Pronunciation::PitchAccent { positions, .. } = p
                    && let Some(category) = pitch_category(&h.reading, positions, verb)
                    && !categories.contains(&category)
                {
                    categories.push(category);
                }
            }
        }
        categories.join(",")
    }

    fn cloze(&self) -> Cloze {
        let headword = self.entry.headwords.first();
        let term = headword.map(|h| h.term.as_str()).unwrap_or("");
        let reading = headword.map(|h| h.reading.as_str()).unwrap_or("");
        let original_len = self.primary_source().map(|s| s.original_text.chars().count()).unwrap_or(0);
        let chars: Vec<char> = self.ctx.sentence.chars().collect();
        let clamp = |i: usize| i.min(chars.len());
        let (start, end) = (clamp(self.ctx.offset), clamp(self.ctx.offset + original_len));
        let body = String::from_iter(&chars[start..end]);
        let body_kana = distribute_furigana_inflected(term, reading, &body)
            .into_iter()
            .map(|s| if s.reading.is_empty() { s.text } else { s.reading })
            .collect();
        Cloze {
            sentence: String::from_iter(&chars),
            prefix: String::from_iter(&chars[..start]),
            body,
            body_kana,
            suffix: String::from_iter(&chars[end..]),
        }
    }
}

struct Cloze {
    sentence: String,
    prefix: String,
    body: String,
    body_kana: String,
    suffix: String,
}
