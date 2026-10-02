//! 把分词词典下回来，和应用里点那个按钮走的是同一条路。
//!
//!     cargo run -p jp-app --release --example fetch_dict -- <语料库目录>
//!
//! 下 43MB、解开 207MB、两道校验、写那四个配置文件，最后真的加载一次分词器。

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(
        std::env::args().nth(1).expect("用法：fetch_dict <语料库目录>"),
    );
    std::fs::create_dir_all(&root)?;

    let started = std::time::Instant::now();
    let mut last_stage = "";
    let mut last_mb = 0u64;
    let installed = jp_app_lib::tokenizer::download(&root, |progress| {
        let mb = progress.received / 1024 / 1024;
        if progress.stage != last_stage || mb >= last_mb + 5 {
            last_stage = progress.stage;
            last_mb = mb;
            println!("  {} {mb} / {} MB", progress.stage, progress.total / 1024 / 1024);
        }
    })?;
    println!(
        "装好了：{} MB → {}（{:.1}s）",
        installed.bytes / 1024 / 1024,
        installed.dir,
        started.elapsed().as_secs_f64()
    );

    // 真的能分词才算数
    let (resources, dict) =
        jp_tokenizer::locate_sudachipy(&root).ok_or_else(|| anyhow::anyhow!("装完还是找不到"))?;
    let analyzer = jp_tokenizer::Analyzer::from_sudachipy(&resources, &dict)?;
    let tokens = analyzer.analyze("夜行列車に乗って街を出た")?;
    println!(
        "分词：{}",
        tokens.iter().map(|t| t.surface.as_str()).collect::<Vec<_>>().join(" / ")
    );
    Ok(())
}
