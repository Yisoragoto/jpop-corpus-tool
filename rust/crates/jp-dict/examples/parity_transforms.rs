//! 和 Yomitan 逐项对账 `transform()` 的全部输出（文字、条件位、trace、先后顺序）。
//!
//! 基准由 `tools/dump-yomitan-transforms.mjs` 用 Node 跑 Yomitan 源码生成：
//!
//! ```text
//! cargo run -p jp-dict --release --example parity_transforms -- <dump目录>/japanese-transforms-full.json
//! ```

use std::time::Instant;

use jp_dict::transformer::LanguageTransformer;

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("用法: parity_transforms <japanese-transforms-full.json>");
    let expected: Vec<(String, Vec<String>)> = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    let lt = LanguageTransformer::japanese();

    let started = Instant::now();
    let mut results = 0usize;
    let mut mismatched = Vec::new();
    for (source, want) in &expected {
        let got: Vec<String> = lt
            .transform(source)
            .iter()
            .map(|r| {
                let trace = r
                    .trace
                    .iter()
                    .map(|f| format!("{}:{}", lt.transforms()[f.transform].id, f.rule_index))
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{}\t{}\t{}", r.text, r.conditions, trace)
            })
            .collect();
        results += got.len();
        if &got != want {
            mismatched.push((source, want.len(), got.len()));
        }
    }
    let elapsed = started.elapsed();

    println!(
        "输入 {} 条，Rust 输出 {} 项，Yomitan 输出 {} 项",
        expected.len(),
        results,
        expected.iter().map(|(_, w)| w.len()).sum::<usize>()
    );
    println!("逐项一致 {} / {}", expected.len() - mismatched.len(), expected.len());
    for (source, want, got) in mismatched.iter().take(10) {
        println!("  不一致: {source}  Yomitan {want} 项 / Rust {got} 项");
    }
    println!("用时 {:.1} ms（含格式化），平均 {:.1} µs/条", elapsed.as_secs_f64() * 1e3,
        elapsed.as_secs_f64() * 1e6 / expected.len() as f64);
    Ok(())
}
