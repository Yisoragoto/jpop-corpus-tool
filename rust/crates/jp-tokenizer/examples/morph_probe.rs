//! 看几句话的完整形态信息（细分词性、活用型 / 活用形、读音、正规化表记）。只读。
//!
//! ```text
//! cargo run -p jp-tokenizer --release --example morph_probe -- <sudachi 目录> 君がいた きみの声 キミと僕 走れ 行こう
//! ```

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = std::path::PathBuf::from(args.next().expect("sudachi 目录"));
    let analyzer = jp_tokenizer::Analyzer::from_sudachipy(&dir, &dir.join("system.dic"))?;
    for text in args {
        println!("── {text}");
        for t in analyzer.analyze(&text)? {
            println!(
                "  {:<8} lemma={:<8} norm={:<8} reading={:<10} upos={:<6} pos={}",
                t.surface,
                t.lemma,
                t.normalized,
                t.reading,
                t.upos,
                t.pos.join(",")
            );
        }
    }
    Ok(())
}
