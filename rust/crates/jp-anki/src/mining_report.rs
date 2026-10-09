//! 挖词报告：读 Anki collection 里已经挖出来的卡，按歌手、歌曲、JLPT 统计学习进度，
//! 写成一个自带搜索和排序的 HTML。对应 PyQt 版菜单「生成挖词报告…」调用的 `legacy/generate_report.py`。
//!
//! 页面骨架、CSS、JS 是从 `legacy/generate_report.py` 原样抽出来的（`data/mining_report/`），
//! 拼接逻辑逐行照搬；在合成的 collection 上和 Python 写出的文件逐字节对过（`examples/mining_report.rs`）。
//!
//! 有意不同的几处：
//! - **Python 只认「JPOP Corpus」笔记。** 现在一键制卡做的是「Lyrics」笔记，用户真实的 collection 里
//!   JPOP Corpus 是 0 张，照搬的话报告永远是「找不到卡片」。Lyrics 卡的歌手、歌名直接取 `Artist`、
//!   `SongTitle` 字段（空的话从 `MiscInfo` 的「歌手「歌名」 时间」里解析）；它没有 JLPT 字段，查本地的
//!   `jlpt_cache`；词频取 Lapis 排序用的 `FreqSort`。
//! - 词频一列表头写「词频」，不写「JPDB 词频」：`FreqSort` 是几部词频词典的调和平均，不只是 JPDB。
//! - 牌组名里的层级分隔符 `\x1f` 写成 `::`（Python 原样写出去，页面上看不见；见 `learning::deck_display_name`）。
//! - 字段反转义用 `learning::plain_field`：只解 Anki 字段里实际出现的几个实体，`&nbsp;` 解成普通空格；
//!   Python 用完整的 `html.unescape`。真实 collection 里这几个字段一个实体都没有。

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::learning::{plain_field, register_unicase, snapshot};

const CSS: &str = include_str!("../data/mining_report/report.css");
const JS: &str = include_str!("../data/mining_report/report.js");
const PAGE: &str = include_str!("../data/mining_report/page.html");
const LEGEND: &str = include_str!("../data/mining_report/legend.html");

const JLPT_ORDER: [&str; 6] = ["N5", "N4", "N3", "N2", "N1", "未分级"];

fn jlpt_color(level: &str) -> &'static str {
    match level {
        "N5" => "#4caf50",
        "N4" => "#2196f3",
        "N3" => "#ff9800",
        "N2" => "#e91e63",
        "N1" => "#9c27b0",
        _ => "#bdbdbd",
    }
}

fn is_level(value: &str) -> bool {
    matches!(value, "N5" | "N4" | "N3" | "N2" | "N1")
}

/// 一张已挖的卡
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinedCard {
    pub expression: String,
    /// 出处里的第一个歌手
    pub artist: String,
    /// 出处里的第一首歌，解析不出来是空串
    pub song: String,
    /// N5–N1，别的都是空串
    pub jlpt: String,
    pub freq: String,
    pub deck: String,
    /// 复习过至少一次
    pub studied: bool,
}

#[derive(Debug, Clone, Default)]
pub struct MinedCollection {
    pub collection_path: PathBuf,
    /// 有卡的笔记类型，按 JPOP Corpus、Lyrics 的顺序
    pub note_types: Vec<String>,
    pub cards: Vec<MinedCard>,
}

#[derive(Clone, Copy)]
enum Kind {
    JpopCorpus,
    Lyrics,
}

impl Kind {
    fn note_type(self) -> &'static str {
        match self {
            Kind::JpopCorpus => crate::model::NOTE_TYPE,
            Kind::Lyrics => crate::lyrics_model::NOTE_TYPE,
        }
    }
}

/// Python 的 `str.strip()`：比 Rust 的 `trim()` 多去掉 `\x1c`–`\x1f`
fn python_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
}

/// Python `parse_source`：出处「ヨルシカ「春ひさぎ」、Vaundy「怪獣の花唄」」取第一组（歌手, 歌名）。
/// 解析不出来时整串当歌手，空串是「(不明)」。
pub fn parse_source(raw: &str) -> (String, String) {
    // Python 的 `\s` 包括 \x1c–\x1f，Rust regex 的不包括，补上
    static SOURCE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"([^「、\s\x1c-\x1f][^「]*?)「([^」]+)」").unwrap());
    let src = plain_field(raw);
    if let Some(caps) = SOURCE.captures(&src) {
        return (python_strip(&caps[1]).to_owned(), python_strip(&caps[2]).to_owned());
    }
    if src.is_empty() { ("(不明)".to_owned(), String::new()) } else { (src, String::new()) }
}

/// 读 collection（先复制一份再读，不碰 Anki 正在用的库）。
///
/// `deck_filter` 非空时只算牌组名里含这个子串的（不分大小写）。`jlpt_of` 给 Lyrics 卡查 JLPT。
pub fn load(collection: &Path, deck_filter: &str, jlpt_of: &dyn Fn(&str) -> Option<String>) -> Result<MinedCollection> {
    let snap = snapshot(collection)?;
    let snapshot_path = snap.path();
    let conn = Connection::open_with_flags(snapshot_path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI)
        .with_context(|| format!("打不开 {}", snapshot_path.display()))?;
    register_unicase(&conn)?;

    let filter = deck_filter.trim().to_lowercase();
    let mut out = MinedCollection { collection_path: collection.to_path_buf(), ..Default::default() };
    for kind in [Kind::JpopCorpus, Kind::Lyrics] {
        let model_id: Option<i64> = conn
            .query_row("SELECT id FROM notetypes WHERE name=?1", [kind.note_type()], |r| r.get(0))
            .ok();
        let Some(model_id) = model_id else { continue };
        let positions: HashMap<String, usize> = {
            let mut stmt = conn.prepare("SELECT name, ord FROM fields WHERE ntid = ?1")?;
            stmt.query_map([model_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                .filter_map(|row| row.ok())
                .map(|(name, ord)| (name, ord as usize))
                .collect()
        };
        // 和 Python 同一句 SQL：没有 ORDER BY，行的先后决定同数时谁排前面
        let mut stmt = conn.prepare(
            "SELECT c.reps, d.name AS deck, n.flds
            FROM cards c
            JOIN notes n ON n.id = c.nid
            JOIN decks d ON d.id = c.did
            WHERE n.mid = ?",
        )?;
        let rows = stmt.query_map([model_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default(), r.get::<_, String>(2)?))
        })?;
        let before = out.cards.len();
        for row in rows {
            let (reps, deck, flds) = row?;
            let deck = deck.replace('\x1f', "::");
            if !filter.is_empty() && !deck.to_lowercase().contains(&filter) {
                continue;
            }
            let values: Vec<&str> = flds.split('\x1f').collect();
            let at = |i: usize| values.get(i).copied().unwrap_or("");
            let named = |name: &str| positions.get(name).map_or("", |&i| at(i));
            let card = match kind {
                // Python 按位置取：Expression 0、Source 5、JLPT 6、Freq 8
                Kind::JpopCorpus => {
                    let (artist, song) = parse_source(at(5));
                    let jlpt = plain_field(at(6));
                    MinedCard {
                        expression: plain_field(at(0)),
                        artist,
                        song,
                        jlpt: if is_level(&jlpt) { jlpt } else { String::new() },
                        freq: plain_field(at(8)),
                        deck,
                        studied: reps > 0,
                    }
                }
                Kind::Lyrics => {
                    let expression = plain_field(named("Expression"));
                    let mut artist = plain_field(named("Artist"));
                    let mut song = plain_field(named("SongTitle"));
                    if artist.is_empty() && song.is_empty() {
                        (artist, song) = parse_source(named("MiscInfo"));
                    } else if artist.is_empty() {
                        artist = "(不明)".to_owned();
                    }
                    let jlpt = jlpt_of(&expression).filter(|level| is_level(level)).unwrap_or_default();
                    MinedCard { expression, artist, song, jlpt, freq: plain_field(named("FreqSort")), deck, studied: reps > 0 }
                }
            };
            out.cards.push(card);
        }
        if out.cards.len() > before {
            out.note_types.push(kind.note_type().to_owned());
        }
    }
    Ok(out)
}

/// Python `html.escape`
fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

/// Python 的 `f"{n:,}"`
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// （标签, 已学, 总数）
type ProgressRow = (String, usize, usize);

/// Python `progress_rows`：按卡片数降序，同数保持第一次出现的先后（`sorted` 是稳定的），取前 `top` 个
fn progress_rows(cards: &[MinedCard], key: impl Fn(&MinedCard) -> String, top: usize) -> Vec<ProgressRow> {
    let mut order: Vec<String> = Vec::new();
    let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
    for card in cards {
        let label = key(card);
        let entry = counts.entry(label.clone()).or_insert_with(|| {
            order.push(label);
            (0, 0)
        });
        entry.0 += 1;
        entry.1 += usize::from(card.studied);
    }
    let mut rows: Vec<ProgressRow> = order
        .into_iter()
        .map(|label| {
            let (total, studied) = counts[&label];
            (label, studied, total)
        })
        .collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.2));
    rows.truncate(top);
    rows
}

/// 一行进度条。已学、未学两段宽度按最大总数算
fn bar_row(out: &mut String, label_html: &str, title: Option<&str>, studied: usize, total: usize, max_total: usize) {
    let pct_bar = total as f64 / max_total as f64 * 100.0;
    let pct_unstudied = (total - studied) as f64 / max_total as f64 * 100.0;
    let pct_done = studied as f64 / total as f64 * 100.0;
    let title = title.map(|t| format!(" title=\"{}\"", esc(t))).unwrap_or_default();
    let _ = write!(
        out,
        "<div class=\"bar-row\"><span class=\"bar-label\"{title}>{label_html}</span><span class=\"bar-track\">\
         <span class=\"bar-fill-s\" style=\"width:{:.2}%\"></span><span class=\"bar-fill-u\" style=\"width:{:.2}%\"></span>\
         </span><span class=\"bar-val\">{studied}/{total} <span class=\"muted\">({:.0}%)</span></span></div>",
        pct_bar - pct_unstudied,
        pct_unstudied,
        pct_done,
    );
}

/// Python `render_progress_bars`
fn render_progress_bars(rows: &[ProgressRow]) -> String {
    if rows.is_empty() {
        return "<p style='color:var(--muted)'>（データなし）</p>".to_owned();
    }
    let max_total = rows.iter().map(|r| r.2).max().unwrap_or(0).max(1);
    let mut out = String::new();
    for (label, studied, total) in rows {
        bar_row(&mut out, &esc(label), Some(label), *studied, *total, max_total);
    }
    out
}

/// Python `render_jlpt_bars`
fn render_jlpt_bars(cards: &[MinedCard]) -> String {
    let mut totals = [0usize; 6];
    let mut studied = [0usize; 6];
    for card in cards {
        let i = JLPT_ORDER.iter().position(|k| *k == card.jlpt).unwrap_or(5);
        totals[i] += 1;
        studied[i] += usize::from(card.studied);
    }
    let grand = cards.len().max(1);
    let mut segments = String::new();
    for (i, level) in JLPT_ORDER.iter().enumerate() {
        if totals[i] > 0 {
            let _ = write!(
                segments,
                "<div class=\"jlpt-seg\" style=\"width:{:.1}%;background:{}\" title=\"{level}: {} 词\"></div>",
                totals[i] as f64 / grand as f64 * 100.0,
                jlpt_color(level),
                grouped(totals[i]),
            );
        }
    }
    let max_total = totals.iter().copied().max().unwrap_or(0).max(1);
    let mut rows = String::new();
    for (i, level) in JLPT_ORDER.iter().enumerate() {
        if totals[i] == 0 {
            continue;
        }
        let label = format!("<span class=\"dot\" style=\"background:{}\"></span>{}", jlpt_color(level), esc(level));
        bar_row(&mut rows, &label, None, studied[i], totals[i], max_total);
    }
    format!("<div class=\"jlpt-stack\">{segments}</div><div class=\"bar-list\" style=\"margin-top:12px\">{rows}</div>")
}

/// Python `render_word_table`：同一个词只留一行，有复习过的那张优先
fn render_word_table(cards: &[MinedCard]) -> String {
    let mut order: Vec<&str> = Vec::new();
    let mut seen: HashMap<&str, &MinedCard> = HashMap::new();
    for card in cards {
        match seen.get(card.expression.as_str()) {
            None => {
                order.push(&card.expression);
                seen.insert(&card.expression, card);
            }
            Some(_) if card.studied => {
                seen.insert(&card.expression, card);
            }
            Some(_) => {}
        }
    }
    let mut words: Vec<&MinedCard> = order.iter().map(|e| seen[e]).collect();
    words.sort_by(|a, b| {
        let key = |c: &MinedCard| (!c.studied, if c.jlpt.is_empty() { "ZZ".to_owned() } else { c.jlpt.clone() });
        key(a).cmp(&key(b)).then_with(|| a.expression.cmp(&b.expression))
    });

    let mut out = String::new();
    for card in words {
        let badge = if card.jlpt.is_empty() {
            "<span style=\"color:var(--muted)\">—</span>".to_owned()
        } else {
            format!("<span class=\"badge\" style=\"background:{}\">{}</span>", jlpt_color(&card.jlpt), esc(&card.jlpt))
        };
        let status = if card.studied { "✅" } else { "⬜" };
        let blob = format!("{} {} {} {} {status}", card.expression, card.jlpt, card.artist, card.song).to_lowercase();
        let freq = if card.freq.is_empty() { "—".to_owned() } else { esc(&card.freq) };
        let _ = write!(
            out,
            "<tr data-s=\"{}\"><td class=\"lem\">{}</td><td class=\"cen\">{status}</td><td class=\"cen\">{badge}</td>\
             <td class=\"num\">{freq}</td><td class=\"src\">{}</td><td class=\"src\">{}</td>\
             <td class=\"src\" style=\"color:var(--muted)\">{}</td></tr>",
            esc(&blob),
            esc(&card.expression),
            esc(&card.artist),
            esc(&card.song),
            esc(&card.deck),
        );
    }
    out
}

/// 报告页的数字，界面上也显示
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSummary {
    pub cards: usize,
    pub studied: usize,
    pub words: usize,
    pub artists: usize,
    pub songs: usize,
    pub decks: usize,
    pub note_types: Vec<String>,
}

pub fn summarize(collection: &MinedCollection) -> ReportSummary {
    use std::collections::HashSet;
    let cards = &collection.cards;
    ReportSummary {
        cards: cards.len(),
        studied: cards.iter().filter(|c| c.studied).count(),
        words: cards.iter().map(|c| c.expression.as_str()).collect::<HashSet<_>>().len(),
        artists: cards.iter().map(|c| c.artist.as_str()).collect::<HashSet<_>>().len(),
        songs: cards
            .iter()
            .filter(|c| !c.song.is_empty())
            .map(|c| (c.artist.as_str(), c.song.as_str()))
            .collect::<HashSet<_>>()
            .len(),
        decks: cards.iter().map(|c| c.deck.as_str()).collect::<HashSet<_>>().len(),
        note_types: collection.note_types.clone(),
    }
}

/// Python `build_html`。`now` 是「生成时间」那一栏（`%Y-%m-%d %H:%M`），由调用方给，测试时固定。
/// 换行是 `\n`；写文件用 [`write_report`]，和 Python 在 Windows 上文本模式写出的一样换成 CRLF。
pub fn render_html(collection: &MinedCollection, now: &str) -> String {
    let cards = &collection.cards;
    let s = summarize(collection);
    let pct_studied = if s.cards > 0 { s.studied as f64 / s.cards as f64 * 100.0 } else { 0.0 };

    let mut summary = format!(
        "<div class=\"sc\"><div class=\"val\">{}</div><div class=\"lbl\">已挖词汇</div></div>\
         <div class=\"sc\"><div class=\"val\">{}<span style=\"font-size:1rem;font-weight:400;color:var(--muted)\"> ({pct_studied:.0}%)</span></div>\
         <div class=\"lbl\">已学卡片</div></div>\
         <div class=\"sc\"><div class=\"val\">{}</div><div class=\"lbl\">总卡片数</div></div>\
         <div class=\"sc\"><div class=\"val\">{}</div><div class=\"lbl\">歌手数</div></div>\
         <div class=\"sc\"><div class=\"val\">{}</div><div class=\"lbl\">歌曲数</div></div>",
        grouped(s.words),
        grouped(s.studied),
        grouped(s.cards),
        s.artists,
        s.songs,
    );
    if s.decks > 1 {
        let _ = write!(summary, "<div class=\"sc\"><div class=\"val\">{}</div><div class=\"lbl\">牌组数</div></div>", s.decks);
    }

    let artist_rows = progress_rows(cards, |c| c.artist.clone(), 25);
    let song_rows = progress_rows(
        cards,
        |c| if c.song.is_empty() { c.artist.clone() } else { format!("{}「{}」", c.artist, c.song) },
        25,
    );
    let artist_bars = format!("{LEGEND}<div class=\"bar-list\">{}</div>", render_progress_bars(&artist_rows));
    let song_bars = format!("{LEGEND}<div class=\"bar-list\">{}</div>", render_progress_bars(&song_rows));

    let note_types = if s.note_types.is_empty() { crate::model::NOTE_TYPE.to_owned() } else { s.note_types.join("、") };
    let values = HashMap::from([
        ("css", CSS.to_owned()),
        ("note_types", esc(&note_types)),
        ("source", esc(&collection.collection_path.display().to_string())),
        ("now", now.to_owned()),
        ("summary", summary),
        ("artist_bars", artist_bars),
        ("song_bars", song_bars),
        ("jlpt", render_jlpt_bars(cards)),
        ("word_count", grouped(s.words)),
        ("rows", render_word_table(cards)),
        ("js", JS.to_owned()),
    ]);
    fill(PAGE, &values)
}

/// 按顺序替换模板里的 `{{名字}}`。只扫模板本身，填进去的内容里就算有 `{{` 也不会再被替换
fn fill(template: &str, values: &HashMap<&str, String>) -> String {
    let mut out = String::with_capacity(template.len() * 4);
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let Some(len) = rest[start + 2..].find("}}") else { break };
        let name = &rest[start + 2..start + 2 + len];
        out.push_str(&rest[..start]);
        match values.get(name) {
            Some(value) => out.push_str(value),
            None => out.push_str(&rest[start..start + len + 4]),
        }
        rest = &rest[start + len + 4..];
    }
    out.push_str(rest);
    out
}

/// 写报告文件。Windows 上换行写成 CRLF——Python 的 `Path.write_text` 在文本模式下就是这么写的。
pub fn write_report(path: &Path, html: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("建不了目录 {}", parent.display()))?;
    }
    let content = if cfg!(windows) { html.replace('\n', "\r\n") } else { html.to_owned() };
    std::fs::write(path, content).with_context(|| format!("写不了 {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(expression: &str, artist: &str, song: &str, jlpt: &str, studied: bool) -> MinedCard {
        MinedCard {
            expression: expression.into(),
            artist: artist.into(),
            song: song.into(),
            jlpt: jlpt.into(),
            freq: String::new(),
            deck: "JPOP".into(),
            studied,
        }
    }

    #[test]
    fn source_parsing_follows_the_python_pattern() {
        let cases = [
            ("ヨルシカ「春ひさぎ」、Vaundy「怪獣の花唄」", ("ヨルシカ", "春ひさぎ")),
            ("  ずっと真夜中でいいのに。「秒針を噛む」", ("ずっと真夜中でいいのに。", "秒針を噛む")),
            ("サカナクション「ユリイカ」 0:39", ("サカナクション", "ユリイカ")),
            ("<b>King Gnu</b>「白日」", ("King Gnu", "白日")),
            // Python 先反转义再去标签：&lt;D&gt; 变成 <D> 之后被当成标签去掉
            ("A&amp;B「C&lt;D&gt;」", ("A&B", "C")),
            ("YOASOBI", ("YOASOBI", "")),
            ("", ("(不明)", "")),
        ];
        for (raw, (artist, song)) in cases {
            assert_eq!(parse_source(raw), (artist.to_owned(), song.to_owned()), "{raw}");
        }
    }

    #[test]
    fn progress_rows_keep_first_seen_order_for_ties_and_cut_at_top() {
        let cards = vec![
            card("a", "B", "", "", true),
            card("b", "A", "", "", false),
            card("c", "A", "", "", true),
            card("d", "C", "", "", false),
            card("e", "B", "", "", false),
        ];
        let rows = progress_rows(&cards, |c| c.artist.clone(), 2);
        assert_eq!(rows, vec![("B".to_owned(), 1, 2), ("A".to_owned(), 1, 2)]);
    }

    #[test]
    fn word_table_keeps_one_row_per_word_and_prefers_the_studied_copy() {
        let mut first = card("夜", "X", "1", "N5", false);
        first.deck = "旧".into();
        let mut studied = card("夜", "Y", "2", "N5", true);
        studied.deck = "新".into();
        let html = render_word_table(&[first, card("朝", "Z", "", "", false), studied, card("昼", "Z", "", "N3", false)]);
        assert_eq!(html.matches("<tr ").count(), 3);
        let order: Vec<&str> = ["夜", "昼", "朝"].to_vec();
        let positions: Vec<usize> = order.iter().map(|w| html.find(&format!("<td class=\"lem\">{w}</td>")).unwrap()).collect();
        assert!(positions.windows(2).all(|p| p[0] < p[1]), "复习过的在前，其次按 JLPT，没等级的最后");
        assert!(html.contains("<td class=\"src\" style=\"color:var(--muted)\">新</td>"));
    }

    #[test]
    fn html_escapes_like_python_and_fills_every_placeholder() {
        let collection = MinedCollection {
            collection_path: PathBuf::from("C:/x/collection.anki2"),
            note_types: vec!["Lyrics".into()],
            cards: vec![card("it's \"ok\" <b>", "A&B", "曲", "N2", true)],
        };
        let html = render_html(&collection, "2026-09-17 20:00");
        assert!(!html.contains("{{"), "模板占位符没填完");
        assert!(html.contains("<td class=\"lem\">it&#x27;s &quot;ok&quot; &lt;b&gt;</td>"));
        assert!(html.contains("笔记类型：Lyrics"));
        assert!(html.contains("生成时间：2026-09-17 20:00"));
        assert!(html.contains("词汇列表（共 1 词）"));
    }

    #[test]
    fn placeholders_inside_filled_values_are_left_alone() {
        let values = HashMap::from([("a", "{{b}}".to_owned()), ("b", "x".to_owned())]);
        assert_eq!(fill("[{{a}}|{{b}}|{{c}}]", &values), "[{{b}}|x|{{c}}]");
    }

    /// 和 Anki 同构的最小 collection：JPOP Corpus、Lyrics 各几张，外加一张别的笔记类型
    fn collection_with_both_note_types(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jp-anki-mining-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("collection.anki2");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE notetypes (id INTEGER PRIMARY KEY, name TEXT);
             CREATE TABLE fields (ntid INTEGER, ord INTEGER, name TEXT);
             CREATE TABLE notes (id INTEGER PRIMARY KEY, mid INTEGER, flds TEXT);
             CREATE TABLE cards (id INTEGER PRIMARY KEY, nid INTEGER, did INTEGER, reps INTEGER);
             CREATE TABLE decks (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO notetypes VALUES (1, 'JPOP Corpus'), (2, 'Lyrics'), (3, 'Basic');
             INSERT INTO decks VALUES (10, 'JPOP'), (20, 'JPOP' || char(31) || 'ヨルシカ');",
        )
        .unwrap();
        let lyrics_fields: Vec<&str> =
            crate::lyrics_model::FIELDS.iter().chain(crate::lyrics_model::SONG_FIELDS).copied().collect();
        for (ord, name) in lyrics_fields.iter().enumerate() {
            conn.execute("INSERT INTO fields VALUES (2, ?1, ?2)", rusqlite::params![ord as i64, name]).unwrap();
        }
        let add = |id: i64, mid: i64, fields: Vec<String>, deck: i64, reps: i64| {
            let flds = fields.join("\x1f");
            conn.execute("INSERT INTO notes VALUES (?1, ?2, ?3)", rusqlite::params![id, mid, flds]).unwrap();
            conn.execute("INSERT INTO cards VALUES (?1, ?1, ?2, ?3)", rusqlite::params![id, deck, reps]).unwrap();
        };
        let jpop = |values: [&str; 10]| values.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>();
        let lyrics = |pairs: &[(&str, &str)]| {
            lyrics_fields
                .iter()
                .map(|name| pairs.iter().find(|(k, _)| k == name).map_or(String::new(), |(_, v)| (*v).to_owned()))
                .collect::<Vec<_>>()
        };
        add(1, 1, jpop(["夜", "よる", "", "", "", "ヨルシカ「夜行」", "N5", "", "123", "名詞"]), 20, 3);
        add(2, 2, lyrics(&[("Expression", "思い出す"), ("SongTitle", "ユリイカ"), ("Artist", "サカナクション"), ("FreqSort", "400")]), 10, 0);
        add(3, 2, lyrics(&[("Expression", "競り合い"), ("MiscInfo", "ずっと真夜中でいいのに。「ミラーチューン」 0:31")]), 10, 6);
        add(4, 3, vec!["front".into(), "back".into()], 10, 9);
        path
    }

    #[test]
    fn both_note_types_are_read_and_lyrics_cards_get_jlpt_from_the_lookup() {
        let path = collection_with_both_note_types("both");
        let jlpt = |expression: &str| (expression == "思い出す").then(|| "N3".to_owned());
        let mined = load(&path, "", &jlpt).unwrap();
        assert_eq!(mined.note_types, ["JPOP Corpus", "Lyrics"]);
        let got: Vec<(&str, &str, &str, &str, &str, &str, bool)> = mined
            .cards
            .iter()
            .map(|c| (c.expression.as_str(), c.artist.as_str(), c.song.as_str(), c.jlpt.as_str(), c.freq.as_str(), c.deck.as_str(), c.studied))
            .collect();
        assert_eq!(
            got,
            [
                ("夜", "ヨルシカ", "夜行", "N5", "123", "JPOP::ヨルシカ", true),
                ("思い出す", "サカナクション", "ユリイカ", "N3", "400", "JPOP", false),
                ("競り合い", "ずっと真夜中でいいのに。", "ミラーチューン", "", "", "JPOP", true),
            ]
        );

        // 牌组筛选按界面上看到的 `::` 写法
        let only = load(&path, "jpop::ヨルシカ", &jlpt).unwrap();
        assert_eq!(only.cards.len(), 1);
        assert_eq!(only.note_types, ["JPOP Corpus"]);

        assert!(load(&path.with_file_name("nope.anki2"), "", &jlpt).is_err());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn grouped_matches_python_thousands() {
        assert_eq!([grouped(0), grouped(999), grouped(1000), grouped(1234567)], ["0", "999", "1,000", "1,234,567"]);
    }
}
