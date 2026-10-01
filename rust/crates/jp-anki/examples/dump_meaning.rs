//! 把一批词的读音和 Meaning HTML 导成 JSON，和 Python 的 `_build_meaning_html` 逐字对账。
//!
//!     cargo run --release -p jp-anki --example dump_meaning -- <词表.json> <输出.json>
//!
//! 词表是 JSON 字符串数组。**只读**打开 corpus.db。
//!
//! 为什么要对到逐字：「刷新旧牌组」会重写用户已有卡片的 Meaning。
//! 渲染只要差一点，刷新之后所有卡片的样子都会变。

use std::collections::BTreeMap;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        eprintln!("用法: dump_meaning <词表.json> <输出.json>");
        std::process::exit(2);
    };

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();
    let conn = rusqlite::Connection::open_with_flags(
        root.join("corpus.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;

    let words: Vec<String> = serde_json::from_str(&std::fs::read_to_string(&input)?)?;
    let mut out = BTreeMap::new();
    for word in &words {
        let (reading, definitions) = jp_anki::dict::lookup_definitions(&conn, word)?;
        out.insert(
            word.clone(),
            serde_json::json!({
                "reading": reading,
                "meaning": jp_anki::card::meaning_html(&definitions),
            }),
        );
    }
    std::fs::write(&output, serde_json::to_string(&out)?)?;
    println!("{} 个词 → {output}", out.len());
    Ok(())
}
