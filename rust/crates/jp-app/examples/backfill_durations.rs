//! 命令行跑时长回填。修数据不该非得开着 GUI。
//!
//!     cargo run -p jp-app --example backfill_durations
//!
//! 幂等：只填 `duration_sec` 为空的行，重跑不会覆盖已有值。

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("JPOP_CORPUS_HOME").map(PathBuf::from))
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(3)
                .expect("manifest 路径异常")
                .to_path_buf()
        });

    let db = root.join("corpus.db");
    anyhow::ensure!(db.is_file(), "找不到 {}", db.display());

    let mut corpus = jp_corpus::Corpus::open_writable(&db)?;
    corpus.check_schema()?;

    let report = jp_app_lib::maintenance::backfill_durations(&mut corpus)?;
    println!("{}", report.summary());
    for sample in &report.samples {
        println!("  {sample}");
    }
    Ok(())
}
