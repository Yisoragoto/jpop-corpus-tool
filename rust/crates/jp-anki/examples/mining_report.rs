//! 挖词报告写成 HTML，和 Python 版 `generate_report.py` 对账。
//!
//! ```text
//! cargo run -p jp-anki --example mining_report -- <collection.anki2> <输出.html> "<生成时间>" [牌组名包含]
//! ```
//!
//! 只读：collection 先复制一份再读。Lyrics 卡的 JLPT 在这里不查（对账用的是 JPOP Corpus 卡）。

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let usage = "用法: mining_report <collection.anki2> <输出.html> <生成时间> [牌组名包含]";
    let collection = args.next().context(usage)?;
    let output = args.next().context(usage)?;
    let now = args.next().context(usage)?;
    let deck = args.next().unwrap_or_default();

    let mined = jp_anki::mining_report::load(std::path::Path::new(&collection), &deck, &|_| None)?;
    let summary = jp_anki::mining_report::summarize(&mined);
    let html = jp_anki::mining_report::render_html(&mined, &now);
    jp_anki::mining_report::write_report(std::path::Path::new(&output), &html)?;
    println!("{summary:?}");
    Ok(())
}
