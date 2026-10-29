//! 在一个库（**用副本**）上升结构、补全部形态信息，打印对齐统计和耗时。会写库。
//!
//! ```text
//! cargo run -p jp-import --release --example annotate_copy -- <库副本 corpus.db> <sudachi 目录>
//! ```
//!
//! 补完之后用 `scripts/annotation_reconcile.py` 拿 SudachiPy 独立对账。

use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let db = args.next().expect("corpus.db 副本");
    let dir = std::path::PathBuf::from(args.next().expect("sudachi 目录"));
    let mut corpus = jp_corpus::Corpus::open_writable(&db)?;
    println!("{}", corpus.migrate()?.summary());
    let analyzer = jp_tokenizer::Analyzer::from_sudachipy(&dir, &dir.join("system.dic"))?;
    let t = Instant::now();
    let tx = corpus.connection_mut().transaction()?;
    let stats = jp_import::annotate::annotate_missing(&tx, &analyzer)?;
    tx.commit()?;
    println!("{stats:?}  用时 {:?}", t.elapsed());
    let left: i64 = corpus.connection().query_row("SELECT COUNT(*) FROM tokens WHERE pos_ja IS NULL", [], |r| r.get(0))?;
    println!("还没标的 token：{left}");
    Ok(())
}
