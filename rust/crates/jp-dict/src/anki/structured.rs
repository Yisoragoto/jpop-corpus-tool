//! 释义（结构化内容、图片）和调核位置 → 卡片 HTML。
//!
//! 移植自 Yomitan（GPL-3.0-or-later，Copyright (C) 2023-2026 Yomitan Authors）：
//! - `ext/js/display/structured-content-generator.js`（制卡分支：`AnkiTemplateRendererContentManager`）
//! - `ext/js/display/pronunciation-generator.js` 的 `createPronunciationDownstepPosition`
//! - `ext/js/templates/anki-template-renderer.js` 的 `_formatGlossary`、`_getHtml`、`_normalizeHtml`

use std::collections::HashMap;

use serde_json::{Map, Value};

use super::css::{PRONUNCIATION_STYLES, STRUCTURED_CONTENT_STYLES, StyleApplier};
use super::dom::{Element, Node, dataset_key, js_number, js_string};
use crate::furigana::downstep_positions;
use crate::text::is_code_point_japanese;

/// Handlebars `escapeExpression`
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            '`' => out.push_str("&#x60;"),
            '=' => out.push_str("&#x3D;"),
            _ => out.push(c),
        }
    }
    out
}

/// 词典图片在 Anki 媒体库里的文件名：`(词典名, 词典内路径) → 文件名`。
/// 查不到的记进 `missing`，调用方存好图片后再渲染一次（和 Yomitan 的 requirements 一样）。
pub struct MediaResolver<'a> {
    pub files: &'a HashMap<(String, String), String>,
    pub missing: Vec<(String, String)>,
}

impl MediaResolver<'_> {
    fn file_name(&mut self, dictionary: &str, path: &str) -> Option<String> {
        let key = (dictionary.to_owned(), path.to_owned());
        match self.files.get(&key) {
            Some(name) => Some(escape(name)),
            None => {
                if !self.missing.contains(&key) {
                    self.missing.push(key);
                }
                None
            }
        }
    }
}

/// 中文独有的字符范围（汉字、全角标点已在日文范围里）
fn is_code_point_chinese_only(cp: u32) -> bool {
    matches!(cp, 0x3100..=0x312f | 0x31a0..=0x31bf | 0x16fe0..=0x16fff | 0xfe50..=0xfe6f | 0xfe10..=0xfe1f)
}

/// JS `getLanguageFromText(text, null)`
fn language_from_text(text: &str) -> Option<&'static str> {
    if text.chars().any(|c| is_code_point_japanese(c as u32)) {
        Some("ja")
    } else if text.chars().any(|c| is_code_point_chinese_only(c as u32)) {
        Some("zh")
    } else {
        None
    }
}

fn number(map: &Map<String, Value>, key: &str) -> Option<f64> {
    map.get(key).and_then(Value::as_f64)
}

fn string<'a>(map: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    map.get(key).and_then(Value::as_str)
}

/// `#rrggbb` → `rgb(r, g, b)`：浏览器读回 `cssText` 时颜色是这个形式
fn normalize_color(value: &str) -> String {
    let Some(hex) = value.strip_prefix('#') else { return value.to_owned() };
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return value.to_owned();
    }
    let expand = |s: &str| -> Vec<u8> {
        s.chars().filter_map(|c| u8::from_str_radix(&format!("{c}{c}"), 16).ok()).collect()
    };
    let bytes: Vec<u8> = match hex.len() {
        3 | 4 => expand(hex),
        6 | 8 => (0..hex.len()).step_by(2).filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok()).collect(),
        _ => return value.to_owned(),
    };
    match bytes.as_slice() {
        [r, g, b] | [r, g, b, 255] => format!("rgb({r}, {g}, {b})"),
        [r, g, b, a] => {
            let alpha = (f64::from(*a) / 255.0 * 100.0).round() / 100.0;
            format!("rgba({r}, {g}, {b}, {})", js_number(alpha))
        }
        _ => value.to_owned(),
    }
}

pub struct Generator<'m, 'f> {
    pub media: &'m mut MediaResolver<'f>,
}

impl Generator<'_, '_> {
    pub fn create_structured_content(&mut self, content: &Value, dictionary: &str) -> Element {
        let mut node = Element::with_class("span", "structured-content");
        self.append(&mut node, content, dictionary, None);
        node
    }

    fn append<'c>(&mut self, container: &mut Element, content: &'c Value, dictionary: &str, language: Option<&'c str>) {
        match content {
            Value::String(text) => {
                if !text.is_empty() {
                    container.append_text(text);
                    if language.is_none()
                        && let Some(lang) = language_from_text(text)
                    {
                        container.set_attr("lang", lang);
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.append(container, item, dictionary, language);
                }
            }
            Value::Object(map) => {
                if let Some(node) = self.generic_element(map, dictionary, language) {
                    container.append_element(node);
                }
            }
            _ => {}
        }
    }

    fn generic_element<'c>(
        &mut self,
        content: &'c Map<String, Value>,
        dictionary: &str,
        language: Option<&'c str>,
    ) -> Option<Element> {
        let tag = string(content, "tag")?;
        Some(match tag {
            "br" => self.element(tag, content, dictionary, language, false, false, false),
            "ruby" | "rt" | "rp" => self.element(tag, content, dictionary, language, false, true, false),
            "table" => {
                let mut wrapper = Element::with_class("div", "gloss-sc-table-container");
                wrapper.append_element(self.element(tag, content, dictionary, language, false, true, false));
                wrapper
            }
            "thead" | "tbody" | "tfoot" | "tr" => self.element(tag, content, dictionary, language, false, true, false),
            "th" | "td" => self.element(tag, content, dictionary, language, true, true, true),
            "div" | "span" | "ol" | "ul" | "li" | "details" | "summary" => {
                self.element(tag, content, dictionary, language, false, true, true)
            }
            "img" => self.create_definition_image(content, dictionary),
            "a" => self.link(content, dictionary, language),
            _ => return None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn element<'c>(
        &mut self,
        tag: &str,
        content: &'c Map<String, Value>,
        dictionary: &str,
        mut language: Option<&'c str>,
        table_cell: bool,
        has_children: bool,
        has_style: bool,
    ) -> Element {
        let mut node = Element::with_class(tag, &format!("gloss-sc-{tag}"));
        if let Some(Value::Object(data)) = content.get("data") {
            for (key, value) in data {
                let mut key2 = String::from("sc");
                let mut chars = key.chars();
                if let Some(first) = chars.next() {
                    key2.extend(first.to_uppercase());
                    key2.extend(chars);
                }
                node.set_dataset(&key2, &js_string(value));
            }
        }
        if let Some(lang) = string(content, "lang") {
            node.set_attr("lang", lang);
            language = Some(lang);
        }
        if table_cell {
            if let Some(n) = number(content, "colSpan") {
                node.set_attr("colspan", &js_number(n.trunc()));
            }
            if let Some(n) = number(content, "rowSpan") {
                node.set_attr("rowspan", &js_number(n.trunc()));
            }
        }
        if has_style {
            if let Some(Value::Object(style)) = content.get("style") {
                apply_style(&mut node, style);
            }
            if let Some(title) = string(content, "title") {
                node.set_attr("title", title);
            }
            if content.get("open") == Some(&Value::Bool(true)) {
                node.set_attr("open", "");
            }
        }
        if has_children && let Some(child) = content.get("content") {
            self.append(&mut node, child, dictionary, language);
        }
        node
    }

    fn link<'c>(&mut self, content: &'c Map<String, Value>, dictionary: &str, mut language: Option<&'c str>) -> Element {
        let href = string(content, "href").unwrap_or_default();
        let internal = href.starts_with('?');
        let mut node = Element::with_class("a", "gloss-link");
        node.set_dataset("external", if internal { "false" } else { "true" });
        let mut text = Element::with_class("span", "gloss-link-text");
        if let Some(lang) = string(content, "lang") {
            node.set_attr("lang", lang);
            language = Some(lang);
        }
        if let Some(child) = content.get("content") {
            self.append(&mut text, child, dictionary, language);
        }
        node.append_element(text);
        if !internal {
            let mut icon = Element::with_class("span", "gloss-link-external-icon icon");
            icon.set_dataset("icon", "external-link");
            node.append_element(icon);
        }
        node.set_attr("href", if internal { "#" } else { href });
        node
    }

    /// JS `createDefinitionImage`（制卡分支：`<img>`，媒体文件名由 Anki 媒体库给）
    pub fn create_definition_image(&mut self, data: &Map<String, Value>, dictionary: &str) -> Element {
        let path = string(data, "path").unwrap_or_default();
        let width = number(data, "width").unwrap_or(100.0);
        let height = number(data, "height").unwrap_or(100.0);
        let preferred_width = number(data, "preferredWidth");
        let preferred_height = number(data, "preferredHeight");
        let inv_aspect_ratio = match (preferred_width, preferred_height) {
            (Some(w), Some(h)) => h / w,
            _ => height / width,
        };
        let used_width = match (preferred_width, preferred_height) {
            (Some(w), _) => w,
            (None, Some(h)) => h / inv_aspect_ratio,
            _ => width,
        };

        let mut node = Element::with_class("a", "gloss-image-link");
        node.set_attr("target", "_blank");
        node.set_attr("rel", "noreferrer noopener");

        let mut container = Element::with_class("span", "gloss-image-container");
        let mut sizer = Element::with_class("span", "gloss-image-sizer");
        let mut background = Element::with_class("span", "gloss-image-background");
        let overlay = Element::with_class("span", "gloss-image-container-overlay");
        let mut link_text = Element::with_class("span", "gloss-image-link-text");
        link_text.append_text("Image");

        let image_rendering = match (string(data, "imageRendering"), data.get("pixelated")) {
            (Some(r), _) => r.to_owned(),
            (None, Some(Value::Bool(true))) => "pixelated".to_owned(),
            _ => "auto".to_owned(),
        };
        let flag = |key: &str, default: &str| match data.get(key) {
            Some(Value::Bool(b)) => b.to_string(),
            _ => default.to_owned(),
        };
        node.set_dataset("path", path);
        node.set_dataset("dictionary", dictionary);
        node.set_dataset("imageLoadState", "not-loaded");
        node.set_dataset("hasAspectRatio", "true");
        node.set_dataset("imageRendering", &image_rendering);
        node.set_dataset("appearance", string(data, "appearance").unwrap_or("auto"));
        node.set_dataset("background", &flag("background", "true"));
        node.set_dataset("collapsed", &flag("collapsed", "false"));
        node.set_dataset("collapsible", &flag("collapsible", "true"));
        if let Some(align) = string(data, "verticalAlign") {
            node.set_dataset("verticalAlign", align);
        }
        let size_units = string(data, "sizeUnits");
        let has_preferred = preferred_width.is_some() || preferred_height.is_some();
        if let Some(units) = size_units
            && has_preferred
        {
            node.set_dataset("sizeUnits", units);
        }

        sizer.set_style("padding-top", &format!("{}%", js_number(inv_aspect_ratio * 100.0)));
        if let Some(border) = string(data, "border") {
            container.set_style("border", border);
        }
        if let Some(radius) = string(data, "borderRadius") {
            container.set_style("border-radius", radius);
        }
        container.set_style("width", &format!("{}em", js_number(used_width)));
        if let Some(title) = string(data, "title") {
            container.set_attr("title", title);
        }

        let mut image = Element::with_class("img", "gloss-image");
        // HTMLImageElement.width 是 unsigned long：赋值时截断取整，读回来也是整数
        let image_width = if size_units == Some("em") && has_preferred {
            const EM_SIZE: f64 = 14.0;
            const SCALE: f64 = 2.0; // 2 × devicePixelRatio（离屏渲染时为 1）
            image.set_style("width", &format!("{}em", js_number(used_width)));
            image.set_style("height", &format!("{}em", js_number(used_width * inv_aspect_ratio)));
            (used_width * EM_SIZE * SCALE).trunc()
        } else {
            used_width.trunc()
        };
        image.set_attr("width", &js_number(image_width));
        image.set_attr("height", &js_number((image_width * inv_aspect_ratio).trunc()));
        // Anki 不给 100% 宽高就显示不对（Yomitan 原注释）
        image.set_style("width", "100%");
        image.set_style("height", "100%");

        if let Some(url) = self.media.file_name(dictionary, path) {
            image.set_attr("src", &url);
            node.set_attr("href", &url);
            node.set_dataset("imageLoadState", "loaded");
            background.set_style("--image", &format!("url(\"{url}\")"));
        }

        container.append_element(sizer);
        container.append_element(background);
        container.append_element(overlay);
        container.append_element(image);
        node.append_element(container);
        node.append_element(link_text);
        node
    }
}

/// JS `_setStructuredContentElementStyle`：只认这些属性，顺序也照抄
fn apply_style(node: &mut Element, style: &Map<String, Value>) {
    const STRING_PROPS: &[(&str, &str)] = &[
        ("fontStyle", "font-style"),
        ("fontWeight", "font-weight"),
        ("fontSize", "font-size"),
        ("color", "color"),
        ("background", "background"),
        ("backgroundColor", "background-color"),
        ("verticalAlign", "vertical-align"),
        ("textAlign", "text-align"),
        ("textEmphasis", "text-emphasis"),
        ("textShadow", "text-shadow"),
    ];
    let color_like = |css: &str, v: &str| {
        if matches!(css, "color" | "background-color" | "border-color" | "text-decoration-color") {
            normalize_color(v)
        } else {
            v.to_owned()
        }
    };
    for (key, css) in STRING_PROPS {
        if let Some(v) = string(style, key) {
            node.set_style(css, &color_like(css, v));
        }
    }
    match style.get("textDecorationLine") {
        Some(Value::String(v)) => node.set_style("text-decoration", v),
        Some(Value::Array(items)) => {
            let joined = items.iter().map(js_string).collect::<Vec<_>>().join(" ");
            node.set_style("text-decoration", &joined);
        }
        _ => {}
    }
    const MORE: &[(&str, &str)] = &[
        ("textDecorationStyle", "text-decoration-style"),
        ("textDecorationColor", "text-decoration-color"),
        ("borderColor", "border-color"),
        ("borderStyle", "border-style"),
        ("borderRadius", "border-radius"),
        ("borderWidth", "border-width"),
        ("clipPath", "clip-path"),
        ("margin", "margin"),
    ];
    for (key, css) in MORE {
        if let Some(v) = string(style, key) {
            node.set_style(css, &color_like(css, v));
        }
    }
    for (key, css) in [
        ("marginTop", "margin-top"),
        ("marginLeft", "margin-left"),
        ("marginRight", "margin-right"),
        ("marginBottom", "margin-bottom"),
    ] {
        match style.get(key) {
            Some(Value::Number(n)) => node.set_style(css, &format!("{}em", js_number(n.as_f64().unwrap_or(0.0)))),
            Some(Value::String(v)) => node.set_style(css, v),
            _ => {}
        }
    }
    for (key, css) in [
        ("padding", "padding"),
        ("paddingTop", "padding-top"),
        ("paddingLeft", "padding-left"),
        ("paddingRight", "padding-right"),
        ("paddingBottom", "padding-bottom"),
        ("wordBreak", "word-break"),
        ("whiteSpace", "white-space"),
        ("cursor", "cursor"),
        ("listStyleType", "list-style-type"),
    ] {
        if let Some(v) = string(style, key) {
            node.set_style(css, v);
        }
    }
}

/// JS `_normalizeHtml` + `innerHTML`：放进临时容器，class → 行内 style，删掉不需要的 data-*，文本里的换行变 `<br>`
fn html(node: Element, applier: &StyleApplier, keep_structured_dataset: bool) -> String {
    let mut container = Element::new("div");
    container.append_element(node);
    applier.apply(&mut container);
    clean(&mut container, keep_structured_dataset);
    container.inner_html()
}

fn clean(element: &mut Element, keep_structured_dataset: bool) {
    for child in &mut element.children {
        if let Node::Element(child) = child {
            for name in child.attr_names() {
                let Some(key) = dataset_key(&name) else { continue };
                // `/^sc([^a-z]|$)/`：留下结构化内容自带的 data-sc-*
                let keep = keep_structured_dataset
                    && key.strip_prefix("sc").is_some_and(|rest| rest.chars().next().is_none_or(|c| !c.is_ascii_lowercase()));
                if !keep {
                    child.remove_attr(&name);
                }
            }
            clean(child, keep_structured_dataset);
        }
    }
    let children = std::mem::take(&mut element.children);
    for child in children {
        match child {
            Node::Text(text) if text.contains('\n') => {
                for (i, part) in text.split('\n').enumerate() {
                    if i > 0 {
                        element.append_element(Element::new("br"));
                    }
                    element.append_text(part);
                }
            }
            other => element.append(other),
        }
    }
}

fn multi_line(text: &str) -> String {
    escape(text).split('\n').collect::<Vec<_>>().join("<br>")
}

/// JS `_formatGlossary(dictionary, content)`
pub fn format_glossary(dictionary: &str, content: &Value, media: &mut MediaResolver<'_>) -> String {
    match content {
        Value::String(text) => multi_line(text),
        Value::Object(map) => match string(map, "type") {
            Some("image") => {
                let node = Generator { media }.create_definition_image(map, dictionary);
                html(node, &STRUCTURED_CONTENT_STYLES, true)
            }
            Some("structured-content") => {
                let empty = Value::Null;
                let node = Generator { media }.create_structured_content(map.get("content").unwrap_or(&empty), dictionary);
                html(node, &STRUCTURED_CONTENT_STYLES, true)
            }
            Some("text") => multi_line(string(map, "text").unwrap_or_default()),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// JS `pronunciation format='position'`
pub fn pronunciation_position(reading: &str, positions: &Value) -> String {
    if reading.is_empty() {
        return String::new();
    }
    let text = match positions {
        Value::Number(n) => js_number(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => downstep_positions(s).iter().map(|p| p.to_string()).collect::<Vec<_>>().join(","),
        _ => return String::new(),
    };
    let mut n1 = Element::with_class("span", "pronunciation-downstep-notation");
    n1.set_dataset("downstepPosition", &text);
    for (class, content) in [
        ("pronunciation-downstep-notation-prefix", "["),
        ("pronunciation-downstep-notation-number", text.as_str()),
        ("pronunciation-downstep-notation-suffix", "]"),
    ] {
        let mut n2 = Element::with_class("span", class);
        n2.append_text(content);
        n1.append_element(n2);
    }
    html(n1, &PRONUNCIATION_STYLES, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn no_media() -> HashMap<(String, String), String> {
        HashMap::new()
    }

    #[test]
    fn a_pitch_position_renders_like_yomitan() {
        assert_eq!(
            pronunciation_position("ばんごう", &json!(3)),
            r#"<span style="display:inline;"><span>[</span><span>3</span><span>]</span></span>"#
        );
        assert_eq!(pronunciation_position("", &json!(3)), "");
    }

    #[test]
    fn structured_content_keeps_sc_data_and_detects_language() {
        let files = no_media();
        let mut media = MediaResolver { files: &files, missing: Vec::new() };
        let content = json!({
            "type": "structured-content",
            "content": {"tag": "span", "data": {"content": "sense", "dic-item": ""}, "style": {"fontWeight": "bold", "marginLeft": 0.5, "color": "#ff0000"}, "content": "一\n二"}
        });
        assert_eq!(
            format_glossary("D", &content, &mut media),
            // lang 落在直接装着文字的那个元素上，排在 style 之后（先设样式、后追加子节点）
            r#"<span><span data-sc-content="sense" style="font-weight: bold; color: rgb(255, 0, 0); margin-left: 0.5em;" lang="ja">一<br>二</span></span>"#
        );
    }

    #[test]
    fn a_stored_image_gets_src_and_missing_ones_are_reported() {
        let mut files = no_media();
        files.insert(("D".into(), "a.png".into()), "jpop_dict_1.png".into());
        let mut media = MediaResolver { files: &files, missing: Vec::new() };
        let html = format_glossary("D", &json!({"type": "image", "path": "a.png", "width": 20, "height": 10}), &mut media);
        assert!(html.contains(r#"src="jpop_dict_1.png""#), "{html}");
        assert!(html.contains(r#"href="jpop_dict_1.png""#), "{html}");
        let _ = format_glossary("D", &json!({"type": "image", "path": "b.png"}), &mut media);
        assert_eq!(media.missing, vec![("D".to_owned(), "b.png".to_owned())]);
    }

    #[test]
    fn plain_text_is_escaped_with_line_breaks() {
        let files = no_media();
        let mut media = MediaResolver { files: &files, missing: Vec::new() };
        assert_eq!(format_glossary("D", &json!("a<b>\n=c"), &mut media), "a&lt;b&gt;<br>&#x3D;c");
    }
}
