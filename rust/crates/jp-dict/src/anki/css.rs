//! 把 class 换成行内 style：Yomitan `ext/js/dom/css-style-applier.js` 的移植
//! （Copyright (C) 2023-2026 Yomitan Authors，GPL-3.0-or-later）。
//!
//! 卡片离开 Yomitan 就没有它的样式表了，所以 Yomitan 制卡时按
//! `data/structured-content-style.json`、`data/pronunciation-style.json`（原样取自 Yomitan）
//! 把命中的规则写进 `style`，再删掉 `class`。
//!
//! 选择器匹配只实现这两张表实际用到的语法：类、属性（`=` `^=`）、`:not()`、`:root`、
//! `:hover` `:focus`、`:nth-of-type` `:nth-last-of-type`、伪元素，以及后代、子、兄弟组合符。
//! 卡片 HTML 是挂在一个游离的容器里生成的，`:root`、`:hover`、`:focus`、伪元素永远不匹配——
//! 和 Yomitan 在浏览器里的结果相同。

use std::sync::LazyLock;

use serde::Deserialize;

use super::dom::{Element, Node};

#[derive(Debug, Clone, PartialEq)]
enum AttrOp {
    Exists,
    Equals(String),
    Prefix(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Simple {
    Class(String),
    Attr(String, AttrOp),
    Not(Vec<Simple>),
    NthOfType(usize),
    NthLastOfType(usize),
    /// `:root` `:hover` `:focus` 伪元素、不认识的伪类：在游离的卡片片段里永远不匹配
    Never,
}

#[derive(Debug, Clone)]
struct Compound {
    tag: Option<String>,
    simples: Vec<Simple>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Combinator {
    Descendant,
    Child,
    Sibling,
    Adjacent,
}

#[derive(Debug, Clone)]
struct Complex {
    compounds: Vec<Compound>,
    /// `combinators[i]` 连接 `compounds[i]` 和 `compounds[i + 1]`
    combinators: Vec<Combinator>,
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()
}

struct Parser<'a> {
    chars: Vec<char>,
    pos: usize,
    _src: &'a str,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self { chars: src.chars().collect(), pos: 0, _src: src }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn ident(&mut self) -> String {
        let start = self.pos;
        while self.peek().is_some_and(is_ident_char) {
            self.pos += 1;
        }
        String::from_iter(&self.chars[start..self.pos])
    }

    fn skip_ws(&mut self) -> bool {
        let start = self.pos;
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
        self.pos > start
    }

    fn until(&mut self, end: char) -> String {
        let start = self.pos;
        let mut depth = 0;
        while let Some(c) = self.peek() {
            if c == '(' {
                depth += 1;
            } else if c == ')' && depth > 0 {
                depth -= 1;
            } else if c == end && depth == 0 {
                break;
            }
            self.pos += 1;
        }
        let out = String::from_iter(&self.chars[start..self.pos]);
        self.pos += 1;
        out
    }

    fn compound(&mut self) -> Option<Compound> {
        let mut tag = None;
        if self.peek() == Some('*') {
            self.pos += 1;
        } else if self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            tag = Some(self.ident().to_ascii_lowercase());
        }
        let mut simples = Vec::new();
        loop {
            match self.peek() {
                Some('.') => {
                    self.pos += 1;
                    simples.push(Simple::Class(self.ident()));
                }
                Some('[') => {
                    self.pos += 1;
                    let body = self.until(']');
                    simples.push(parse_attr(&body));
                }
                Some(':') => {
                    self.pos += 1;
                    if self.peek() == Some(':') {
                        self.pos += 1;
                        self.ident();
                        simples.push(Simple::Never);
                        continue;
                    }
                    let name = self.ident();
                    let args = if self.peek() == Some('(') {
                        self.pos += 1;
                        Some(self.until(')'))
                    } else {
                        None
                    };
                    simples.push(match (name.as_str(), args) {
                        ("not", Some(inner)) => {
                            let mut p = Parser::new(&inner);
                            match p.compound() {
                                Some(c) if c.tag.is_none() => Simple::Not(c.simples),
                                _ => Simple::Never,
                            }
                        }
                        ("nth-of-type", Some(n)) => n.trim().parse().map(Simple::NthOfType).unwrap_or(Simple::Never),
                        ("nth-last-of-type", Some(n)) => {
                            n.trim().parse().map(Simple::NthLastOfType).unwrap_or(Simple::Never)
                        }
                        _ => Simple::Never,
                    });
                }
                _ => break,
            }
        }
        (tag.is_some() || !simples.is_empty()).then_some(Compound { tag, simples })
    }

    fn complex(&mut self) -> Option<Complex> {
        self.skip_ws();
        let mut compounds = vec![self.compound()?];
        let mut combinators = Vec::new();
        loop {
            let had_ws = self.skip_ws();
            let combinator = match self.peek() {
                None => break,
                Some('>') => Combinator::Child,
                Some('~') => Combinator::Sibling,
                Some('+') => Combinator::Adjacent,
                Some(_) if had_ws => Combinator::Descendant,
                Some(_) => return None,
            };
            if combinator != Combinator::Descendant {
                self.pos += 1;
                self.skip_ws();
            }
            compounds.push(self.compound()?);
            combinators.push(combinator);
        }
        Some(Complex { compounds, combinators })
    }
}

fn parse_attr(body: &str) -> Simple {
    let unquote = |v: &str| v.trim().trim_matches(|c| c == '"' || c == '\'').to_owned();
    if let Some((name, value)) = body.split_once("^=") {
        Simple::Attr(name.trim().to_owned(), AttrOp::Prefix(unquote(value)))
    } else if let Some((name, value)) = body.split_once('=') {
        if name.ends_with(['~', '|', '$', '*']) {
            return Simple::Never;
        }
        Simple::Attr(name.trim().to_owned(), AttrOp::Equals(unquote(value)))
    } else {
        Simple::Attr(body.trim().to_owned(), AttrOp::Exists)
    }
}

/// 从容器到目标元素的路径：每一层是（元素, 在父元素子节点里的下标）
type Path<'a> = Vec<(&'a Element, usize)>;

fn element_siblings(parent: &Element) -> impl Iterator<Item = (usize, &Element)> {
    parent.children.iter().enumerate().filter_map(|(i, n)| match n {
        Node::Element(e) => Some((i, e)),
        Node::Text(_) => None,
    })
}

fn matches_simple(simple: &Simple, element: &Element, parent: Option<&Element>, index: usize) -> bool {
    match simple {
        Simple::Class(name) => element.classes().iter().any(|c| c == name),
        Simple::Attr(name, op) => match (element.attr(name), op) {
            (None, _) => false,
            (Some(_), AttrOp::Exists) => true,
            (Some(v), AttrOp::Equals(want)) => &v == want,
            (Some(v), AttrOp::Prefix(want)) => !want.is_empty() && v.starts_with(want.as_str()),
        },
        Simple::Not(inner) => !inner.iter().all(|s| matches_simple(s, element, parent, index)),
        Simple::NthOfType(n) | Simple::NthLastOfType(n) => {
            let Some(parent) = parent else { return false };
            let same: Vec<usize> =
                element_siblings(parent).filter(|(_, e)| e.tag == element.tag).map(|(i, _)| i).collect();
            let position = same.iter().position(|&i| i == index);
            match (simple, position) {
                (Simple::NthOfType(_), Some(p)) => p + 1 == *n,
                (Simple::NthLastOfType(_), Some(p)) => same.len() - p == *n,
                _ => false,
            }
        }
        Simple::Never => false,
    }
}

fn matches_compound(compound: &Compound, element: &Element, parent: Option<&Element>, index: usize) -> bool {
    compound.tag.as_ref().is_none_or(|t| *t == element.tag)
        && compound.simples.iter().all(|s| matches_simple(s, element, parent, index))
}

/// `ancestors` 是 `element` 之上的路径（不含它自己）
fn matches_at(sel: &Complex, k: usize, element: &Element, index: usize, ancestors: &[(&Element, usize)]) -> bool {
    let parent = ancestors.last().map(|(e, _)| *e);
    if !matches_compound(&sel.compounds[k], element, parent, index) {
        return false;
    }
    if k == 0 {
        return true;
    }
    match sel.combinators[k - 1] {
        Combinator::Child => match ancestors.split_last() {
            Some((&(p, pi), rest)) => matches_at(sel, k - 1, p, pi, rest),
            None => false,
        },
        Combinator::Descendant => (0..ancestors.len())
            .rev()
            .any(|d| matches_at(sel, k - 1, ancestors[d].0, ancestors[d].1, &ancestors[..d])),
        Combinator::Sibling | Combinator::Adjacent => {
            let Some(parent) = parent else { return false };
            let previous: Vec<(usize, &Element)> = element_siblings(parent).filter(|(i, _)| *i < index).collect();
            let candidates: Vec<&(usize, &Element)> = if sel.combinators[k - 1] == Combinator::Adjacent {
                previous.last().into_iter().collect()
            } else {
                previous.iter().collect()
            };
            candidates.into_iter().any(|&(i, e)| matches_at(sel, k - 1, e, i, ancestors))
        }
    }
}

#[derive(Deserialize)]
struct RawRule {
    selectors: Vec<String>,
    styles: Vec<(String, String)>,
}

struct Rule {
    joined: String,
    selectors: Vec<Option<Complex>>,
    css: String,
}

pub struct StyleApplier {
    rules: Vec<Rule>,
}

pub static STRUCTURED_CONTENT_STYLES: LazyLock<StyleApplier> =
    LazyLock::new(|| StyleApplier::from_json(include_str!("../../data/structured-content-style.json")));

pub static PRONUNCIATION_STYLES: LazyLock<StyleApplier> =
    LazyLock::new(|| StyleApplier::from_json(include_str!("../../data/pronunciation-style.json")));

impl StyleApplier {
    fn from_json(json: &str) -> Self {
        let raw: Vec<RawRule> = serde_json::from_str(json).expect("内置样式表应当能解析");
        let rules = raw
            .into_iter()
            .map(|r| Rule {
                joined: r.selectors.join(","),
                // 解析不了的选择器在浏览器里 matches 会抛异常，Yomitan 跳过；这里记为不匹配
                selectors: r.selectors.iter().map(|s| Parser::new(s).complex()).collect(),
                css: r.styles.iter().map(|(p, v)| format!("{p}:{v};")).collect(),
            })
            .collect();
        Self { rules }
    }

    /// JS `_selectorMightMatch`
    fn might_match(joined: &str, classes: &[String]) -> bool {
        classes.iter().any(|class| {
            let needle = format!(".{class}");
            let mut start = 0;
            while let Some(found) = joined[start..].find(&needle) {
                let end = start + found + needle.len();
                match joined[end..].chars().next() {
                    Some(c) if c.is_ascii_alphanumeric() || c == '-' || c == '_' => start = end,
                    _ => return true,
                }
            }
            false
        })
    }

    fn style_for(&self, element: &Element, index: usize, ancestors: &[(&Element, usize)]) -> String {
        let classes = element.classes();
        let mut css = String::new();
        for rule in self.rules.iter().filter(|r| Self::might_match(&r.joined, &classes)) {
            let matched = rule.selectors.iter().flatten().any(|sel| {
                let last = sel.compounds.len() - 1;
                matches_at(sel, last, element, index, ancestors)
            });
            if matched {
                css.push_str(&rule.css);
            }
        }
        css.push_str(&element.css_text());
        css
    }

    /// JS `applyClassStyles`：先算完所有元素的样式，再统一删 class、写 style
    pub fn apply(&self, root: &mut Element) {
        let mut computed: Vec<Option<String>> = Vec::new();
        let mut path: Path<'_> = vec![(&*root, 0)];
        collect(self, root, &mut path, &mut computed);
        let mut iter = computed.into_iter();
        write(root, &mut iter);
    }
}

fn collect<'a>(applier: &StyleApplier, element: &'a Element, path: &mut Path<'a>, out: &mut Vec<Option<String>>) {
    for (index, child) in element.children.iter().enumerate() {
        let Node::Element(child) = child else { continue };
        let has_class = child.attr("class").is_some_and(|c| !c.is_empty());
        out.push(has_class.then(|| applier.style_for(child, index, path)));
        path.push((child, index));
        collect(applier, child, path, out);
        path.pop();
    }
}

fn write(element: &mut Element, styles: &mut impl Iterator<Item = Option<String>>) {
    for child in &mut element.children {
        let Node::Element(child) = child else { continue };
        if let Some(Some(style)) = styles.next() {
            child.remove_attr("class");
            if style.is_empty() {
                child.remove_attr("style");
            } else {
                child.set_attr("style", &style);
            }
        }
        write(child, styles);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(attrs: &[(&str, &str)]) -> Element {
        let mut a = Element::with_class("a", "gloss-image-link");
        for (k, v) in attrs {
            a.set_attr(k, v);
        }
        let mut container = Element::with_class("span", "gloss-image-container");
        container.append_element(Element::with_class("span", "gloss-image-background"));
        a.append_element(container);
        a
    }

    #[test]
    fn descendant_attribute_and_not_selectors_match_like_a_browser() {
        let sel = Parser::new(".gloss-image-link:not([data-appearance=monochrome]) .gloss-image-background")
            .complex()
            .unwrap();
        let a = link(&[("data-appearance", "auto")]);
        let Node::Element(container) = &a.children[0] else { unreachable!() };
        let Node::Element(background) = &container.children[0] else { unreachable!() };
        let path = [(&a, 0), (container, 0)];
        assert!(matches_at(&sel, 1, background, 0, &path));
        let mono = link(&[("data-appearance", "monochrome")]);
        let Node::Element(container) = &mono.children[0] else { unreachable!() };
        let Node::Element(background) = &container.children[0] else { unreachable!() };
        assert!(!matches_at(&sel, 1, background, 0, &[(&mono, 0), (container, 0)]));
    }

    #[test]
    fn pseudo_elements_hover_and_root_never_match() {
        for s in [".gloss-image-link-text::before", ".gloss-image-link[data-collapsed=true]:hover .x", ":root[data-browser=firefox] .x"] {
            let sel = Parser::new(s).complex().unwrap();
            assert!(sel.compounds.iter().any(|c| c.simples.contains(&Simple::Never)), "{s}");
        }
    }

    #[test]
    fn might_match_requires_a_whole_class_name() {
        let classes = vec!["gloss-image".to_owned()];
        assert!(StyleApplier::might_match(".gloss-image", &classes));
        assert!(StyleApplier::might_match(".a .gloss-image,.b", &classes));
        assert!(!StyleApplier::might_match(".gloss-image-link", &classes));
    }
}
