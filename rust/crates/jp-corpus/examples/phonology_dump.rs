//! 每行歌词按发音拆拍后的结果，一行一条 TSV：`utterance_id \t 歌词 \t 发音（片假名）`。
//! 拆不了的词（英文等）在发音里记成 `{表层形}`。用来和 UniDic 的发音字段逐行对账。
//!
//! `cargo run -p jp-corpus --example phonology_dump -- <corpus.db 副本>`

use std::io::Write;

use jp_corpus::Corpus;
use jp_corpus::phonology::{Word, kana_of, pronounce_words};

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("要给 corpus.db 副本的路径");
    let corpus = Corpus::open(&path)?;
    let conn = corpus.connection();
    let mut stmt = conn.prepare(
        "SELECT u.id, u.text, COALESCE(t.reading,''), t.surface, COALESCE(t.pos,''), COALESCE(t.pos_ja,''), COALESCE(t.cform,'') \
         FROM live_utterances u JOIN tokens t ON t.utterance_id = u.id \
         ORDER BY u.song_id, u.line_idx, u.id, t.token_idx",
    )?;
    let mut rows = stmt.query([])?;
    let mut lines: Vec<(i64, String, Vec<[String; 5]>)> = Vec::new();
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        if lines.last().map(|l| l.0) != Some(id) {
            lines.push((id, r.get(1)?, Vec::new()));
        }
        let pos: String = r.get(4)?;
        if pos == "PUNCT" || pos == "SYM" {
            continue;
        }
        lines.last_mut().unwrap().2.push([r.get(2)?, r.get(3)?, pos, r.get(5)?, r.get(6)?]);
    }
    let out = std::io::stdout();
    let mut out = out.lock();
    for (id, text, toks) in &lines {
        let words: Vec<Word> = toks
            .iter()
            .map(|t| Word { reading: &t[0], surface: &t[1], pos: &t[2], pos_ja: &t[3], cform: &t[4] })
            .collect();
        let kana: String = pronounce_words(&words)
            .iter()
            .zip(toks)
            .map(|(m, t)| match m {
                Some(ms) => kana_of(ms),
                None => format!("{{{}}}", t[1]),
            })
            .collect();
        writeln!(out, "{id}\t{}\t{kana}", text.replace(['\t', '\n'], " "))?;
    }
    Ok(())
}
