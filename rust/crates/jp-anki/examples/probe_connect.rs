//! 对着真的 AnkiConnect 探一遍。**只发只读动作**：version / deckNames / modelNames /
//! modelFieldNames / findNotes。不新建、不修改任何东西。
//!
//!     cargo run -p jp-anki --example probe_connect

use jp_anki::{AnkiConnect, NOTE_TYPE};
use jp_scraper::UreqTransport;
use jp_scraper::http::HttpClient;

fn main() {
    let anki = AnkiConnect::new(HttpClient::new(Box::new(UreqTransport)));

    match anki.ping() {
        Ok(v) => println!("AnkiConnect v{v}"),
        Err(e) => {
            println!("连不上：{}", e.advice());
            return;
        }
    }

    match anki.deck_names() {
        Ok(decks) => {
            println!("牌组 {} 个：", decks.len());
            for d in decks.iter().take(15) {
                println!("  {d}");
            }
        }
        Err(e) => println!("读牌组失败：{}", e.advice()),
    }

    match anki.model_names() {
        Ok(models) => {
            let ready = models.iter().any(|m| m == NOTE_TYPE);
            println!("笔记类型 {} 个，「{NOTE_TYPE}」{}", models.len(), if ready { "已就位" } else { "不存在" });
            if ready {
                match anki.model_field_names(NOTE_TYPE) {
                    Ok(fields) => println!("  现有字段：{}", fields.join(" | ")),
                    Err(e) => println!("  读字段失败：{}", e.advice()),
                }
            }
        }
        Err(e) => println!("读笔记类型失败：{}", e.advice()),
    }

    match anki.find_notes(&format!("note:\"{NOTE_TYPE}\"")) {
        Ok(ids) => println!("已有 {} 张 JPOP Corpus 笔记", ids.len()),
        Err(e) => println!("查笔记失败：{}", e.advice()),
    }
}
