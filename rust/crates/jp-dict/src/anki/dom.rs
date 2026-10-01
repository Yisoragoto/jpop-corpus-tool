//! 生成卡片 HTML 用的最小 DOM。
//!
//! Yomitan 在浏览器里用真 DOM 拼好元素再取 `innerHTML`。要和它逐字一致，得照浏览器的规则：
//! - 属性按第一次设置的先后排；改已有属性不挪位置；
//! - `style` 是个声明列表，`cssText` 形如 `width: 100%; height: 100%;`；
//! - `dataset` 的驼峰键和 `data-` 属性名互转，带「-小写字母」的键在浏览器里会抛异常（Yomitan 吞掉异常跳过）；
//! - 序列化按 HTML 片段序列化算法转义，空元素没有结束标签。

use serde_json::Value;

#[derive(Debug, Clone)]
enum AttrValue {
    Text(String),
    /// 值取自 `style` 声明列表
    Style,
}

#[derive(Debug, Clone)]
pub struct Element {
    pub tag: String,
    attrs: Vec<(String, AttrValue)>,
    style: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub enum Node {
    Element(Element),
    Text(String),
}

const VOID_ELEMENTS: &[&str] =
    &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];

impl Element {
    pub fn new(tag: &str) -> Self {
        Self { tag: tag.to_owned(), attrs: Vec::new(), style: Vec::new(), children: Vec::new() }
    }

    /// `document.createElement(tag)` 再设 `className`
    pub fn with_class(tag: &str, class: &str) -> Self {
        let mut element = Self::new(tag);
        element.set_attr("class", class);
        element
    }

    pub fn set_attr(&mut self, name: &str, value: &str) {
        if let Some((_, v)) = self.attrs.iter_mut().find(|(n, _)| n == name) {
            if matches!(v, AttrValue::Style) {
                self.style.clear();
            }
            *v = AttrValue::Text(value.to_owned());
        } else {
            self.attrs.push((name.to_owned(), AttrValue::Text(value.to_owned())));
        }
    }

    pub fn remove_attr(&mut self, name: &str) {
        if let Some(i) = self.attrs.iter().position(|(n, _)| n == name) {
            if matches!(self.attrs[i].1, AttrValue::Style) {
                self.style.clear();
            }
            self.attrs.remove(i);
        }
    }

    pub fn attr(&self, name: &str) -> Option<String> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| match v {
            AttrValue::Text(t) => t.clone(),
            AttrValue::Style => self.css_text(),
        })
    }

    pub fn attr_names(&self) -> Vec<String> {
        self.attrs.iter().map(|(n, _)| n.clone()).collect()
    }

    /// `element.style.setProperty(name, value)`；`name` 用连字符写法。空值等于删除。
    pub fn set_style(&mut self, name: &str, value: &str) {
        if value.is_empty() {
            self.style.retain(|(n, _)| n != name);
            return;
        }
        match self.style.iter_mut().find(|(n, _)| n == name) {
            Some((_, v)) => *v = value.to_owned(),
            None => self.style.push((name.to_owned(), value.to_owned())),
        }
        match self.attrs.iter_mut().find(|(n, _)| n == "style") {
            Some((_, v @ AttrValue::Text(_))) => {
                // 先前被 setAttribute 写成了纯文本：浏览器会重新解析，这里简化为以声明列表为准
                *v = AttrValue::Style;
            }
            Some(_) => {}
            None => self.attrs.push(("style".to_owned(), AttrValue::Style)),
        }
    }

    pub fn css_text(&self) -> String {
        self.style.iter().map(|(n, v)| format!("{n}: {v};")).collect::<Vec<_>>().join(" ")
    }

    /// `element.dataset[key] = value`。键不合法时浏览器抛异常，这里返回 false（调用方和 Yomitan 一样忽略）。
    pub fn set_dataset(&mut self, key: &str, value: &str) -> bool {
        let chars: Vec<char> = key.chars().collect();
        if chars.windows(2).any(|w| w[0] == '-' && w[1].is_ascii_lowercase()) {
            return false;
        }
        let mut name = String::from("data-");
        for c in chars {
            if c.is_ascii_uppercase() {
                name.push('-');
                name.push(c.to_ascii_lowercase());
            } else {
                name.push(c);
            }
        }
        if !is_valid_attribute_name(&name) {
            return false;
        }
        self.set_attr(&name, value);
        true
    }

    pub fn append(&mut self, node: Node) {
        self.children.push(node);
    }

    pub fn append_element(&mut self, element: Element) {
        self.children.push(Node::Element(element));
    }

    pub fn append_text(&mut self, text: &str) {
        self.children.push(Node::Text(text.to_owned()));
    }

    pub fn classes(&self) -> Vec<String> {
        self.attr("class").map(|c| c.split_ascii_whitespace().map(str::to_owned).collect()).unwrap_or_default()
    }

    /// `innerHTML`
    pub fn inner_html(&self) -> String {
        let mut out = String::new();
        for child in &self.children {
            serialize(child, &mut out);
        }
        out
    }
}

/// XML Name 的近似：ASCII 里只允许字母、数字和 `-._:`，非 ASCII 放行。
fn is_valid_attribute_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false };
    let ascii_ok = |c: char, first: bool| {
        c.is_ascii_alphabetic() || c == '_' || c == ':' || (!first && (c.is_ascii_digit() || c == '-' || c == '.'))
    };
    let ok = |c: char, first: bool| if c.is_ascii() { ascii_ok(c, first) } else { !c.is_control() && !c.is_whitespace() };
    ok(first, true) && chars.all(|c| ok(c, false))
}

/// `data-sc-content` → `scContent`（`dataset` 的键）
pub fn dataset_key(attribute: &str) -> Option<String> {
    let rest = attribute.strip_prefix("data-")?;
    let mut out = String::new();
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '-'
            && let Some(&next) = chars.peek()
            && next.is_ascii_lowercase()
        {
            out.push(next.to_ascii_uppercase());
            chars.next();
            continue;
        }
        out.push(c);
    }
    Some(out)
}

fn escape_text(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

fn escape_attribute(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

fn serialize(node: &Node, out: &mut String) {
    match node {
        Node::Text(text) => escape_text(text, out),
        Node::Element(element) => {
            out.push('<');
            out.push_str(&element.tag);
            for (name, value) in &element.attrs {
                out.push(' ');
                out.push_str(name);
                out.push_str("=\"");
                match value {
                    AttrValue::Text(t) => escape_attribute(t, out),
                    AttrValue::Style => escape_attribute(&element.css_text(), out),
                }
                out.push('"');
            }
            out.push('>');
            if VOID_ELEMENTS.contains(&element.tag.as_str()) {
                return;
            }
            for child in &element.children {
                serialize(child, out);
            }
            out.push_str("</");
            out.push_str(&element.tag);
            out.push('>');
        }
    }
}

/// JS `String(value)`（给 `dataset` 赋值时的隐式转换）
pub fn js_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => js_number(n.as_f64().unwrap_or(0.0)),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_owned(),
        Value::Array(items) => items
            .iter()
            .map(|v| if v.is_null() { String::new() } else { js_string(v) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_owned(),
    }
}

/// JS 数字转字符串：整数不带小数点，其余取最短往返表示。
pub fn js_number(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_owned();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    if n == 0.0 {
        return "0".to_owned();
    }
    let s = format!("{n}");
    s.strip_suffix(".0").map(str::to_owned).unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_keep_first_set_order_and_style_serializes_like_css_text() {
        let mut img = Element::with_class("img", "gloss-image");
        img.set_attr("width", "350");
        img.set_style("width", "100%");
        img.set_attr("height", "350");
        img.set_style("height", "100%");
        img.set_attr("width", "351");
        let mut span = Element::new("span");
        span.append_element(img);
        span.append_text("a<b & \u{a0}");
        assert_eq!(
            span.inner_html(),
            r#"<img class="gloss-image" width="351" style="width: 100%; height: 100%;" height="350">a&lt;b &amp; &nbsp;"#
        );
    }

    #[test]
    fn dataset_keys_round_trip_and_bad_keys_are_rejected() {
        let mut e = Element::new("div");
        assert!(e.set_dataset("scContent", "glossary"));
        assert!(e.set_dataset("sc外字", ""));
        assert!(!e.set_dataset("scDic-item", ""));
        assert!(!e.set_dataset("sc bad", ""));
        assert_eq!(e.attr_names(), ["data-sc-content", "data-sc外字"]);
        assert_eq!(dataset_key("data-sc-content").as_deref(), Some("scContent"));
        assert_eq!(dataset_key("data-sc-dic_item").as_deref(), Some("scDic_item"));
    }

    #[test]
    fn numbers_print_like_javascript() {
        assert_eq!(js_number(350.0), "350");
        assert_eq!(js_number(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(js_number(-1.0), "-1");
        assert_eq!(js_string(&serde_json::json!([1, 3])), "1,3");
    }
}
