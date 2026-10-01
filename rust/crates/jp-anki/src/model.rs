//! 「JPOP Corpus」note type：字段、模板、样式。
//!
//! **和 PyQt 版逐字一致。** 两边往同一个牌组里加卡，字段名对不上就
//! 变成两种笔记类型，用户的复习历史会被拆成两半；样式对不上则是
//! 同一个牌组里长相不同的两套卡。
//!
//! 字段顺序也不能动：Anki 用第一个字段做查重键，而
//! `collection.anki2` 里字段是按顺序用 `\x1f` 拼在一列里存的
//! （[`crate::learning`] 读它的时候按下标取）。

use serde_json::{Value, json};

use crate::connect::{AnkiConnect, Result};

pub const NOTE_TYPE: &str = "JPOP Corpus";

/// 卡片样式。**逐字照搬 PyQt 版**——同一个牌组里新旧卡片
/// 长相必须一致，否则复习时会觉得是两套东西。
pub const CSS: &str = r##"\
.card { font-family:"Meiryo","Yu Gothic UI",sans-serif; font-size:18px;
        text-align:center; padding:16px; }
.word { font-size:2.4em; font-weight:bold; margin:14px 0 4px; }
.reading { font-size:1.1em; opacity:.7; margin-bottom:4px; }
.badges { display:flex; justify-content:center; gap:6px; margin:4px 0 8px; flex-wrap:wrap; }
.jlpt  { display:inline-block; padding:2px 10px; border-radius:4px;
         font-size:.82em; font-weight:bold; color:#fff; background:#3498db; }
.pitch { display:inline-block; padding:2px 10px; border-radius:4px;
         font-size:.82em; font-weight:bold; color:#fff; background:#27ae60; }
.freq  { display:inline-block; padding:2px 10px; border-radius:4px;
         font-size:.82em; color:#fff; background:#8e44ad; }
hr { border:none; border-top:1px solid rgba(128,128,128,.35); margin:14px 0; }
.defs { text-align:left; max-width:980px; margin:0 auto; }
.dict-group { margin:10px 0 16px; padding:10px 12px 12px;
              background:rgba(127,127,127,.07); border-radius:8px; }
.dict-hdr { display:inline-block; font-size:.75em; font-weight:bold; color:#fff;
            padding:3px 12px; border-radius:999px; margin-bottom:8px;
            letter-spacing:0; }
.dict-hdr-明鏡   { background:#c0392b; }
.dict-hdr-小学館 { background:#2471a3; }
.dict-hdr-other  { background:#7f8c8d; }
.meaning { margin:0; }
.ym-item { margin:8px 0 10px; line-height:1.55; }
.ym-sense { display:block; }
.ym-index { display:inline-block; min-width:1.45em; margin-right:.35em;
            color:#dfe7ef; font-weight:700; }
.ym-num { display:inline-block; min-width:1.45em; margin-right:.35em;
          color:#dfe7ef; font-weight:700; }
.ym-tag { color:#d36b16; font-weight:700; margin-right:.35em; }
.ym-gloss { color:#f0f2f4; }
.ym-note { display:block; color:#d2d6dc; font-size:.92em; line-height:1.55; }
.ym-note-label { color:#f0c35b; font-weight:700; margin-right:.35em; }
.ym-note-body { opacity:.9; }
.ym-example { margin:.18em 0 .18em 1.75em; padding-left:.7em;
              border-left:2px solid rgba(150,160,170,.24); font-size:.94em; }
.ym-ja { color:#1f9d32; margin-right:.75em; }
.ym-zh { color:#3f8fd2; }
.ym-term-ref { color:#9aa7b2; font-size:.9em; margin:.1em 0 .2em; }
.sent { text-align:left; margin:6px 0; padding:8px 14px;
        background:rgba(192,57,43,.08); border-left:3px solid #c0392b;
        border-radius:0 4px 4px 0; font-size:.95em; }
.sent-src { font-size:.75em; opacity:.6; margin-left:8px; }
.src { font-size:.75em; opacity:.55; margin-top:10px; }
.pos-badge { display:inline-block; padding:2px 10px; border-radius:4px;
             font-size:.82em; color:#fff; background:#e67e22; }
"##;

/// 字段名和顺序。**不能改。**
///
/// * 第一个字段是 Anki 的查重键
/// * `collection.anki2` 里字段按这个顺序用 `\x1f` 拼成一列
/// * PyQt 版已经建过这个 note type，改名等于另起一套
pub const FIELDS: &[&str] = &[
    "Expression",
    "Reading",
    "Meaning",
    "Sentence",
    "SentenceAudio",
    "Source",
    "JLPT",
    "Pitch",
    "Freq",
    "PartOfSpeech",
];

pub const TEMPLATE_NAME: &str = "JPOP Corpus Card";

/// 正面：词 + 例句。先看句子猜词义，这是这套卡的用法。
pub const FRONT: &str = concat!(
    r#"<div class="word">{{Expression}}</div>"#,
    "{{Sentence}}"
);

/// 背面：读音、词性/JLPT/音高/词频徽章、例句、来源、释义。
pub const BACK: &str = concat!(
    r#"<div class="word">{{Expression}}</div>"#,
    r#"<div class="reading">{{Reading}}</div>"#,
    r#"<div class="badges">"#,
    r#"{{#PartOfSpeech}}<span class="pos-badge">{{PartOfSpeech}}</span>{{/PartOfSpeech}}"#,
    r#"{{#JLPT}}<span class="jlpt">{{JLPT}}</span>{{/JLPT}}"#,
    r#"{{#Pitch}}<span class="pitch">{{Pitch}}</span>{{/Pitch}}"#,
    r#"{{#Freq}}<span class="freq">JPDB {{Freq}}</span>{{/Freq}}"#,
    r#"</div>"#,
    "<hr>",
    "{{Sentence}}",
    "{{SentenceAudio}}",
    r#"{{#Source}}<div class="src">🎵 {{Source}}</div>{{/Source}}"#,
    "<hr>",
    "{{#Meaning}}{{Meaning}}{{/Meaning}}"
);

fn templates() -> Value {
    json!([{
        "Name": TEMPLATE_NAME,
        "Front": FRONT,
        "Back": BACK,
    }])
}

/// 建 note type，或者把已有的更新到最新。
///
/// 幂等：已存在时只补缺失的字段、刷新样式和模板，**不动已有的笔记**。
/// 返回是不是新建的。
pub fn ensure(anki: &AnkiConnect) -> Result<bool> {
    let existing = anki.model_names()?;
    if !existing.iter().any(|n| n == NOTE_TYPE) {
        anki.call(
            "createModel",
            json!({
                "modelName": NOTE_TYPE,
                "inOrderFields": FIELDS,
                "css": CSS,
                "cardTemplates": templates(),
            }),
        )?;
        return Ok(true);
    }

    // 已存在：补字段。**只增不改**——改名或删字段会让老笔记丢内容。
    let have = anki.model_field_names(NOTE_TYPE)?;
    for field in FIELDS {
        if !have.iter().any(|n| n == field) {
            anki.call(
                "modelFieldAdd",
                json!({ "modelName": NOTE_TYPE, "fieldName": field }),
            )?;
        }
    }
    anki.call(
        "updateModelStyling",
        json!({ "model": { "name": NOTE_TYPE, "css": CSS } }),
    )?;
    anki.call(
        "updateModelTemplates",
        json!({
            "model": {
                "name": NOTE_TYPE,
                "templates": { TEMPLATE_NAME: { "Front": FRONT, "Back": BACK } }
            }
        }),
    )?;
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::testing::FakeAnki;
    use jp_scraper::http::HttpClient;
    use std::sync::Arc;

    fn client(fake: Arc<FakeAnki>) -> AnkiConnect {
        struct Shared(Arc<FakeAnki>);
        impl jp_scraper::http::Transport for Shared {
            fn get(
                &self,
                u: &str,
                h: &[(String, String)],
                t: jp_scraper::http::Timeouts,
            ) -> std::result::Result<jp_scraper::http::HttpResponse, jp_scraper::ProviderError>
            {
                self.0.get(u, h, t)
            }
            fn post(
                &self,
                u: &str,
                b: &[u8],
                h: &[(String, String)],
                t: jp_scraper::http::Timeouts,
            ) -> std::result::Result<jp_scraper::http::HttpResponse, jp_scraper::ProviderError>
            {
                self.0.post(u, b, h, t)
            }
        }
        AnkiConnect::new(HttpClient::new(Box::new(Shared(fake))))
    }

    #[test]
    fn a_missing_note_type_is_created_with_every_field() {
        let fake = Arc::new(FakeAnki::new(&[("modelNames", json!(["Basic"]))]));
        assert!(ensure(&client(fake.clone())).unwrap(), "该是新建");
        let params = fake.params_of("createModel").expect("该调 createModel");
        let fields: Vec<&str> = params["inOrderFields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(fields, FIELDS);
        assert_eq!(params["modelName"], NOTE_TYPE);
    }

    #[test]
    fn an_existing_note_type_is_only_topped_up() {
        // 已有笔记的字段不能删不能改名，否则内容会丢
        let fake = Arc::new(FakeAnki::new(&[
            ("modelNames", json!(["JPOP Corpus"])),
            // 老版本缺后四个字段
            (
                "modelFieldNames",
                json!(["Expression", "Reading", "Meaning", "Sentence", "SentenceAudio", "Source"]),
            ),
        ]));
        assert!(!ensure(&client(fake.clone())).unwrap(), "不该是新建");
        let actions = fake.actions();
        assert!(!actions.iter().any(|a| a == "createModel"), "{actions:?}");
        // 缺的四个各补一次
        assert_eq!(
            actions.iter().filter(|a| *a == "modelFieldAdd").count(),
            4,
            "{actions:?}"
        );
        assert!(actions.iter().any(|a| a == "updateModelStyling"));
        assert!(actions.iter().any(|a| a == "updateModelTemplates"));
    }

    #[test]
    fn a_complete_note_type_adds_no_fields() {
        let fake = Arc::new(FakeAnki::new(&[
            ("modelNames", json!(["JPOP Corpus"])),
            ("modelFieldNames", json!(FIELDS)),
        ]));
        ensure(&client(fake.clone())).unwrap();
        assert!(!fake.actions().iter().any(|a| a == "modelFieldAdd"));
    }

    #[test]
    fn expression_is_first_because_anki_dedupes_on_it() {
        assert_eq!(FIELDS[0], "Expression");
    }

    #[test]
    fn the_template_references_only_declared_fields() {
        // 模板里写了个不存在的字段，Anki 会渲染成空白而不报错——
        // 这种错只有复习到那张卡才会发现
        let combined = format!("{FRONT}{BACK}");
        let re = regex::Regex::new(r"\{\{[#/]?([A-Za-z]+)\}\}").unwrap();
        for caps in re.captures_iter(&combined) {
            let name = &caps[1];
            assert!(
                FIELDS.contains(&name),
                "模板引用了未声明的字段 {name}"
            );
        }
    }
}
