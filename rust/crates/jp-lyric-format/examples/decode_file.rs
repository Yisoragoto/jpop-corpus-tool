//! 解析一个歌词文件，打印判出的格式、行数、带逐字时间的行数和字数。
//!
//! ```text
//! cargo run -p jp-lyric-format --example decode_file -- <文件>… [--hint 扩展名]
//! ```
//!
//! 不给 `--hint` 就看内容判断（内嵌歌词那条路）。

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hint = args.iter().position(|a| a == "--hint").and_then(|i| args.get(i + 1)).cloned();
    for path in args.iter().filter(|a| a.as_str() != "--hint" && Some(*a) != hint.as_ref()) {
        let bytes = std::fs::read(path)?;
        match jp_lyric_format::decode(&bytes, hint.as_deref()) {
            Ok(decoded) => {
                let lines = &decoded.parsed.lines;
                let worded = lines.iter().filter(|l| !l.words.is_empty()).count();
                let words: usize = lines.iter().map(|l| l.words.len()).sum();
                println!(
                    "{path}\tformat={}\tlines={}\tword_timed_lines={worded}\twords={words}\ttranslations={}",
                    decoded.format.name(),
                    lines.len(),
                    decoded.parsed.translations.len()
                );
            }
            Err(e) => println!("{path}\tERROR {e:#}"),
        }
    }
    Ok(())
}
