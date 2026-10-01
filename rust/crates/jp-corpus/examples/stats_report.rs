//! 统计报告和统计表导出成 JSON，和 Python 版 `CorpusReportWorker` / `StatsWorker` 对账（只读打开库）。
//! 给了第三个参数时，再把三份报告按 `_export_report_txt` 的格式写成 TXT，和 Python 导出的文件逐字节比。
//!
//! ```text
//! cargo run --release -p jp-corpus --example stats_report -- <corpus.db> <输出.json> [TXT 目录]
//! ```

use anyhow::Context;
use jp_corpus::stats::{StatsFilter, corpus_report, report_text, word_frequency_table};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let usage = "用法: stats_report <corpus.db> <输出.json> [TXT 目录]";
    let db = args.next().context(usage)?;
    let output = args.next().context(usage)?;
    let txt_dir = args.next();
    let corpus = jp_corpus::Corpus::open(&db)?;
    let conn = corpus.connection();
    let yorushika: i64 = conn.query_row("SELECT id FROM people WHERE name = 'ヨルシカ'", [], |r| r.get(0))?;

    let started = std::time::Instant::now();
    let all = StatsFilter::default();
    let artist = StatsFilter { performer_ids: vec![yorushika], jp_only: false };
    let jp = StatsFilter { performer_ids: vec![], jp_only: true };
    let reports = [
        ("report_all", corpus_report(conn, &all)?, Vec::<String>::new(), false),
        ("report_yorushika", corpus_report(conn, &artist)?, vec!["ヨルシカ".to_owned()], false),
        ("report_jp_only", corpus_report(conn, &jp)?, Vec::new(), true),
    ];
    let out = serde_json::json!({
        "report_all": reports[0].1,
        "report_yorushika": reports[1].1,
        "report_jp_only": reports[2].1,
        "stats_all": word_frequency_table(conn, None, &all, 300)?,
        "stats_yorushika_noun": word_frequency_table(conn, Some("NOUN"), &artist, 300)?,
        "stats_jp_only": word_frequency_table(conn, None, &jp, 300)?,
    });
    let elapsed = started.elapsed();
    std::fs::write(&output, serde_json::to_vec_pretty(&out)?)?;
    println!("三份报告 + 三张统计表，共 {:.0} ms", elapsed.as_secs_f64() * 1000.0);

    if let Some(dir) = txt_dir {
        for (name, report, performers, jp_only) in &reports {
            let path = std::path::Path::new(&dir).join(format!("{name}_rs.txt"));
            std::fs::write(&path, report_text(report, performers, *jp_only))?;
            println!("写出 {}", path.display());
        }
    }
    Ok(())
}
