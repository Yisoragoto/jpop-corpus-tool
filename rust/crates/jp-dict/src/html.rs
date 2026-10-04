//! HTML 转义，全工作区只此一份。
//!
//! 以前有四份：`jp-anki` 的 `dict.rs`、`mine.rs`、`mining_report.rs` 各一份，加上这个 crate 里
//! 制卡用的那一份（`anki::escape`）。四份转义的字符不一样——`mine.rs` 那份只管 `& < >`，引号原样写进卡片——
//! 哪天要多转义一个字符，得记得改四处。
//!
//! 现在只有 [`entity`] 这一张表：
//!
//! - [`escape`]：`& < > " '` 五个，和 Python 的 `html.escape`（`quote=True`）逐字相同。
//!   例句、曲名、报告这些「我们自己拼的 HTML」都用它；
//! - `anki::escape`：在这五个之上再转义 `` ` `` 和 `=`，是 Yomitan 模板用的 Handlebars `escapeExpression`。
//!   它多出来的两个不能省（见那个函数的注释），但前五个是从这里拿的。
//!
//! `anki/dom.rs` 里另有两个函数不归这里管：它们是 DOM 序列化（`innerHTML`）的规则，
//! 文本节点不转义引号、属性值不转义尖括号，和「把一段文字安全地放进 HTML」不是一回事。

/// 这个字符在 HTML 里要写成什么。不用转义的返回 `None`。
///
/// 单引号写成 `&#x27;` 而不是 `&#39;`：Python 的 `html.escape` 和 Handlebars 都是这个写法，
/// 已经写进用户卡片里的也是它。读回来的那一侧（`jp-anki` 的 `plain_field`）两种都认。
pub fn entity(c: char) -> Option<&'static str> {
    match c {
        '&' => Some("&amp;"),
        '<' => Some("&lt;"),
        '>' => Some("&gt;"),
        '"' => Some("&quot;"),
        '\'' => Some("&#x27;"),
        _ => None,
    }
}

/// 把一段文字转义成能安全放进 HTML 的形式——文本节点和带引号的属性值里都能放。
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match entity(c) {
            Some(entity) => out.push_str(entity),
            None => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_five_characters_are_escaped_the_way_python_does_it() {
        // 右边是 CPython 3.12 `html.escape(...)` 的输出
        assert_eq!(escape("<b>&\"x\"</b>"), "&lt;b&gt;&amp;&quot;x&quot;&lt;/b&gt;");
        assert_eq!(escape("it's"), "it&#x27;s");
        assert_eq!(escape("夜に駆ける a=b `c`"), "夜に駆ける a=b `c`", "别的字符一个都不动");
        assert_eq!(escape(""), "");
    }

    #[test]
    fn an_ampersand_that_already_starts_an_entity_is_escaped_again() {
        // 不猜「这是不是已经转义过了」：传进来的一律当原文。猜的话原文里的「&amp;」就再也写不出来
        assert_eq!(escape("&amp;"), "&amp;amp;");
    }
}
