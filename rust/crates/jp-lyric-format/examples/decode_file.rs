//! 解析一个歌词文件，打印判出的格式、行数、带逐字时间的行数和字数。
//!
//! ```text
//! cargo run -p jp-lyric-format --example decode_file -- <文件>… [--hint 扩展名] [--show]
//! ```
//!
//! 不给 `--hint` 就看内容判断（内嵌歌词那条路）。`--show` 再把每行正文和译文打出来。

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let show = args.iter().any(|a| a == "--show");
    let hint = args.iter().position(|a| a == "--hint").and_then(|i| args.get(i + 1)).cloned();
    let paths = args
        .iter()
        .filter(|a| a.as_str() != "--hint" && a.as_str() != "--show" && Some(*a) != hint.as_ref());
    for path in paths {
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
                if show {
                    for line in lines {
                        println!("  {:?}\twords={}\t{}", line.time_sec, line.words.len(), line.text);
                    }
                    for line in &decoded.parsed.translations {
                        println!("  T {:?}\t{}", line.time_sec, line.text);
                    }
                }
            }
            Err(e) => println!("{path}\tERROR {e:#}"),
        }
    }
    Ok(())
}
