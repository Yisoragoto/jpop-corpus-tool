//! 「Lyrics」笔记类型：照 Lapis 做的，背面右侧是歌曲封面，封面右边竖排歌名、歌手、专辑。
//!
//! 模板在 `data/lyrics/`：`front.html` `back.html` `styling.css` 由 `tools/make_lyrics_templates.py`
//! 从 Lapis（GPL-3.0）生成，只改了根元素的 class 和右侧图片区；歌曲信息的样式是手写的 `song-info.css`，
//! 接在 Lapis 样式表后面。字段是 Lapis 的 22 个再加 `SongTitle` `Artist` `Album`，
//! 字段名和 Lapis 一致，所以 Lapis 的用户设置（CSS 变量）、以后 Lapis 的更新都能照搬。
//!
//! 和「JPOP Corpus」笔记类型不同：**已存在时不整体刷新模板和样式**。Lapis 的用户习惯直接改样式表顶上的设置变量，
//! 整体覆盖会把这些改动冲掉。只做两件事：缺字段时补字段（只增不删）；样式表末尾的歌曲信息那一块
//! 如果和以前发过的某一版逐字相同（说明用户没改过），换成新版，块外的内容一个字不动。

use std::ops::Range;

use serde_json::{Value, json};

use crate::connect::{AnkiConnect, Result};

pub const NOTE_TYPE: &str = "Lyrics";
const TEMPLATE_NAME: &str = "Mining";

pub const FIELDS: &[&str] = &[
    "Expression",
    "ExpressionFurigana",
    "ExpressionReading",
    "ExpressionAudio",
    "SelectionText",
    "MainDefinition",
    "DefinitionPicture",
    "Sentence",
    "SentenceFurigana",
    "SentenceAudio",
    "Picture",
    "Glossary",
    "Hint",
    "IsWordAndSentenceCard",
    "IsClickCard",
    "IsSentenceCard",
    "IsAudioCard",
    "PitchPosition",
    "PitchCategories",
    "Frequency",
    "FreqSort",
    "MiscInfo",
    "SongTitle",
    "Artist",
    "Album",
];

/// 这几个是 Lyrics 在 Lapis 之外加的字段
pub const SONG_FIELDS: &[&str] = &["SongTitle", "Artist", "Album"];

const FRONT: &str = include_str!("../data/lyrics/front.html");
const BACK: &str = include_str!("../data/lyrics/back.html");
pub(crate) const CSS: &str = concat!(include_str!("../data/lyrics/styling.css"), include_str!("../data/lyrics/song-info.css"));

/// 歌曲信息样式，当前版
const SONG_INFO_CSS: &str = include_str!("../data/lyrics/song-info.css");
/// 以前发过的歌曲信息样式，老的在前。改 `song-info.css` 之前先把旧版存进 `song-info-history/`、加到这里，
/// 否则已经建了 Lyrics 的用户拿不到新样式
const SONG_INFO_HISTORY: &[&str] = &[
    // 第 1 版：信息在封面左边横排右对齐
    include_str!("../data/lyrics/song-info-history/1.css"),
    // 第 2 版：封面右边竖排，三列居中（上下错开）
    include_str!("../data/lyrics/song-info-history/2.css"),
];
const SONG_INFO_START: &str = "/* ---------- Lyrics：";
/// 第 1 版没有结束标记，那一块一直到样式表末尾
const SONG_INFO_END: &str = "/* ---------- Lyrics 结束 ---------- */";

/// `ensure` 做了什么
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ensured {
    Created,
    Existing(SongInfoStyle),
}

/// 已有的 Lyrics 里歌曲信息样式的情况
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SongInfoStyle {
    /// 已经是当前版
    Current,
    /// 是以前的版本，换成了当前版
    Upgraded,
    /// 用户改过或删掉了，没动
    Customized,
}

fn templates() -> Value {
    json!([{ "Name": TEMPLATE_NAME, "Front": FRONT, "Back": BACK }])
}

/// 没有就建；有了只补缺的字段、升级没改过的歌曲信息样式。
pub fn ensure(anki: &AnkiConnect) -> Result<Ensured> {
    if !anki.model_names()?.iter().any(|n| n == NOTE_TYPE) {
        anki.call(
            "createModel",
            json!({
                "modelName": NOTE_TYPE,
                "inOrderFields": FIELDS,
                "css": CSS,
                "isCloze": false,
                "cardTemplates": templates(),
            }),
        )?;
        return Ok(Ensured::Created);
    }
    let have = anki.model_field_names(NOTE_TYPE)?;
    for field in FIELDS {
        if !have.iter().any(|n| n == field) {
            anki.call("modelFieldAdd", json!({ "modelName": NOTE_TYPE, "fieldName": field }))?;
        }
    }
    let styling = anki.call("modelStyling", json!({ "modelName": NOTE_TYPE }))?;
    let css = styling.get("css").and_then(Value::as_str).unwrap_or_default();
    let (status, upgraded) = plan_styling_upgrade(css);
    if let Some(css) = upgraded {
        anki.call("updateModelStyling", json!({ "model": { "name": NOTE_TYPE, "css": css } }))?;
    }
    Ok(Ensured::Existing(status))
}

/// 样式表里歌曲信息那一块：从开始标记到结束标记（含）；没有结束标记就到末尾
fn song_info_range(css: &str) -> Option<Range<usize>> {
    let start = css.find(SONG_INFO_START)?;
    let end = css[start..].find(SONG_INFO_END).map_or(css.len(), |i| start + i + SONG_INFO_END.len());
    Some(start..end)
}

/// 比较时不计换行符和末尾空白：Anki 里存的是建的时候发过去的原样，可能是 CRLF
fn normalized(block: &str) -> String {
    block.replace("\r\n", "\n").trim_end().to_owned()
}

/// Anki 里现有的 Lyrics 样式表该怎么处理；要换时连同新样式表一起返回。不连 Anki
pub fn plan_styling_upgrade(css: &str) -> (SongInfoStyle, Option<String>) {
    upgrade_song_info(css, SONG_INFO_CSS, SONG_INFO_HISTORY)
}

/// 歌曲信息那一块该不该换；要换时连同新样式表一起返回
fn upgrade_song_info(css: &str, current: &str, history: &[&str]) -> (SongInfoStyle, Option<String>) {
    let Some(range) = song_info_range(css) else {
        return (SongInfoStyle::Customized, None);
    };
    let block = normalized(&css[range.clone()]);
    if block == normalized(current) {
        return (SongInfoStyle::Current, None);
    }
    if !history.iter().any(|old| block == normalized(old)) {
        return (SongInfoStyle::Customized, None);
    }
    let newline = if css.contains("\r\n") { "\r\n" } else { "\n" };
    // 先归一到 LF 再换成目标换行符：`current` 是 `include_str!` 进来的，git 按 CRLF
    // 签出时它本身就是 CRLF，直接 replace 会写出 `\r\r\n`。
    let replacement = normalized(current).replace('\n', newline);
    let rest = &css[range.end..];
    let rest = if rest.is_empty() { newline } else { rest };
    (SongInfoStyle::Upgraded, Some(format!("{}{replacement}{rest}", &css[..range.start])))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::connect::testing::{FakeAnki, client};

    const LAPIS_CSS: &str = include_str!("../data/lyrics/styling.css");

    /// 转成 CRLF。**先归一到 LF 再转**：源文件被 git 按 CRLF 签出时
    /// （Windows runner 默认 `autocrlf=true`）直接 replace 会得到 `\r\r\n`，
    /// 于是「本机全过、CI 挂」——v0.2.2~v0.2.7 的 CI 就是这么一直红的。
    /// 仓库里另外用 `.gitattributes` 把这些数据文件钉成 LF，这里是第二道。
    fn crlf(text: &str) -> String {
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    }

    #[test]
    fn a_missing_note_type_is_created_with_expression_first_and_the_song_fields() {
        let fake = Arc::new(FakeAnki::new(&[("modelNames", json!(["Lapis"])), ("createModel", json!({}))]));
        assert_eq!(ensure(&client(fake.clone())).unwrap(), Ensured::Created);
        let params = fake.params_of("createModel").unwrap();
        assert_eq!(params["modelName"], "Lyrics");
        assert_eq!(params["inOrderFields"][0], "Expression");
        assert_eq!(params["inOrderFields"].as_array().unwrap().len(), 25);
        let back = params["cardTemplates"][0]["Back"].as_str().unwrap();
        assert!(back.contains("{{Artist}}") && back.contains("{{SongTitle}}") && back.contains("{{Picture}}"));
        assert!(back.contains("donkuri/lapis"), "要保留 Lapis 的出处");
        let css = params["css"].as_str().unwrap();
        assert!(css.starts_with(LAPIS_CSS) && css.ends_with(SONG_INFO_CSS));
    }

    #[test]
    fn an_existing_note_type_only_gains_missing_fields_and_keeps_its_templates() {
        let mut old: Vec<&str> = FIELDS.to_vec();
        old.retain(|f| *f != "Album");
        let fake = Arc::new(FakeAnki::new(&[
            ("modelNames", json!(["Lyrics"])),
            ("modelFieldNames", json!(old)),
            ("modelFieldAdd", Value::Null),
            ("modelStyling", json!({ "css": CSS })),
        ]));
        assert_eq!(ensure(&client(fake.clone())).unwrap(), Ensured::Existing(SongInfoStyle::Current));
        let added: Vec<Value> = fake.all_params_of("modelFieldAdd");
        assert_eq!(added, vec![json!({ "modelName": "Lyrics", "fieldName": "Album" })]);
        let actions = fake.actions();
        assert!(!actions.iter().any(|a| a.starts_with("updateModel") || a == "createModel"), "{actions:?}");
    }

    /// 用户建 Lyrics 时拿到的是第 1 版（CRLF），之后改了 Lapis 的设置变量：只换歌曲信息那一块
    #[test]
    fn an_unedited_old_song_info_style_is_upgraded_and_the_users_lapis_settings_are_kept() {
        let user_lapis = crlf(LAPIS_CSS).replacen("--pc-info-font-size: 23px;", "--pc-info-font-size: 18px;", 1);
        assert_ne!(user_lapis, crlf(LAPIS_CSS));
        let in_anki = format!("{user_lapis}{}", crlf(SONG_INFO_HISTORY[0]));
        let fake = Arc::new(FakeAnki::new(&[
            ("modelNames", json!(["Lyrics"])),
            ("modelFieldNames", json!(FIELDS)),
            ("modelStyling", json!({ "css": in_anki })),
        ]));
        assert_eq!(ensure(&client(fake.clone())).unwrap(), Ensured::Existing(SongInfoStyle::Upgraded));

        let params = fake.params_of("updateModelStyling").unwrap();
        assert_eq!(params["model"]["name"], "Lyrics");
        let css = params["model"]["css"].as_str().unwrap();
        assert_eq!(css, format!("{user_lapis}{}", crlf(SONG_INFO_CSS)));
        assert!(!css.replace("\r\n", "").contains('\n'), "换行符要跟 Anki 里原来的一致");
        assert!(!fake.actions().iter().any(|a| a == "updateModelTemplates"), "模板不动");
    }

    #[test]
    fn an_edited_song_info_style_is_left_alone() {
        let edited = SONG_INFO_HISTORY[0].replacen("gap: 14px;", "gap: 30px;", 1);
        assert_ne!(edited, SONG_INFO_HISTORY[0]);
        let fake = Arc::new(FakeAnki::new(&[
            ("modelNames", json!(["Lyrics"])),
            ("modelFieldNames", json!(FIELDS)),
            ("modelStyling", json!({ "css": format!("{LAPIS_CSS}{edited}") })),
        ]));
        assert_eq!(ensure(&client(fake.clone())).unwrap(), Ensured::Existing(SongInfoStyle::Customized));
        assert!(fake.params_of("updateModelStyling").is_none());
    }

    #[test]
    fn a_deleted_song_info_style_is_not_put_back() {
        assert_eq!(upgrade_song_info(LAPIS_CSS, SONG_INFO_CSS, SONG_INFO_HISTORY), (SongInfoStyle::Customized, None));
    }

    #[test]
    fn rules_the_user_added_after_the_end_marker_survive_an_upgrade() {
        let old = "/* ---------- Lyrics：旧 */\n.a {}\n/* ---------- Lyrics 结束 ---------- */\n";
        let new = "/* ---------- Lyrics：新 */\n.b {}\n/* ---------- Lyrics 结束 ---------- */\n";
        let css = format!("p {{}}\n\n{old}.mine {{}}\n");
        let (status, upgraded) = upgrade_song_info(&css, new, &[old]);
        assert_eq!(status, SongInfoStyle::Upgraded);
        assert_eq!(upgraded.unwrap(), format!("p {{}}\n\n{new}.mine {{}}\n"));
    }

    #[test]
    fn an_old_block_with_rules_appended_after_it_counts_as_edited() {
        // 第 1 版没有结束标记，后面加的规则算在块里：分不清，就不动
        let css = format!("{LAPIS_CSS}{}.mine {{}}\n", SONG_INFO_HISTORY[0]);
        assert_eq!(upgrade_song_info(&css, SONG_INFO_CSS, SONG_INFO_HISTORY), (SongInfoStyle::Customized, None));
    }

    #[test]
    fn the_shipped_song_info_files_are_well_formed() {
        assert!(SONG_INFO_CSS.starts_with(SONG_INFO_START));
        assert!(SONG_INFO_CSS.trim_end().ends_with(SONG_INFO_END));
        assert_eq!(CSS.matches(SONG_INFO_START).count(), 1);
        for old in SONG_INFO_HISTORY {
            assert!(old.starts_with(SONG_INFO_START));
            assert_ne!(normalized(old), normalized(SONG_INFO_CSS), "当前版不能和历史版本相同");
        }
        // 刚建出来的样式表，下次 ensure 时就是当前版
        assert_eq!(upgrade_song_info(CSS, SONG_INFO_CSS, SONG_INFO_HISTORY).0, SongInfoStyle::Current);
        // 第 1 版就是用户 Anki 里的样子：Lapis 部分 + 第 1 版块，到末尾
        let v1 = format!("{LAPIS_CSS}{}", SONG_INFO_HISTORY[0]);
        assert_eq!(song_info_range(&v1).unwrap(), LAPIS_CSS.len()..v1.len());
    }
}
