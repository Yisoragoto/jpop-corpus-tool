//! 看 Anki 里的「Lyrics」样式表会被怎么处理：歌曲信息那一块是当前版、没改过的旧版（可以升级）还是用户改过的。
//!
//! ```text
//! cargo run -p jp-anki --example lyrics_styling            # 只读
//! cargo run -p jp-anki --example lyrics_styling -- --apply # 是旧版时换成当前版（和制卡时做的一样）
//! ```

use jp_anki::connect::AnkiConnect;
use jp_scraper::UreqTransport;
use jp_scraper::http::HttpClient;
use jp_anki::lyrics_model::{NOTE_TYPE, SongInfoStyle, plan_styling_upgrade};
use serde_json::json;

fn main() -> anyhow::Result<()> {
    let apply = std::env::args().any(|a| a == "--apply");
    let anki = AnkiConnect::new(HttpClient::new(Box::new(UreqTransport)));
    let styling = anki.call("modelStyling", json!({ "modelName": NOTE_TYPE }))?;
    let css = styling["css"].as_str().unwrap_or_default();
    let (status, upgraded) = plan_styling_upgrade(css);
    println!("Anki 里的样式表 {} 字节：{status:?}", css.len());
    let Some(new_css) = upgraded else { return Ok(()) };

    let common = css.bytes().zip(new_css.bytes()).take_while(|(a, b)| a == b).count();
    let tail = css.bytes().rev().zip(new_css.bytes().rev()).take_while(|(a, b)| a == b).count();
    println!("升级后 {} 字节；前 {common} 字节相同，末尾 {tail} 字节相同", new_css.len());
    println!("换掉的部分从第 {common} 字节开始：{:?}", &css[common..(common + 40).min(css.len())]);
    if apply {
        anki.call("updateModelStyling", json!({ "model": { "name": NOTE_TYPE, "css": new_css } }))?;
        let after = anki.call("modelStyling", json!({ "modelName": NOTE_TYPE }))?;
        let status = plan_styling_upgrade(after["css"].as_str().unwrap_or_default()).0;
        println!("已更新；再读一遍：{status:?}");
        assert_eq!(status, SongInfoStyle::Current);
    }
    Ok(())
}
