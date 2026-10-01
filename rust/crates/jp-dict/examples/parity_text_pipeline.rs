//! 整条预处理 + 活用还原管线的大规模对账（Yomitan 测试输入 + 真实歌词行前缀）。
//!
//! ```text
//! cargo run -p jp-dict --release --example parity_text_pipeline -- <dump目录>/japanese-text-pipeline-full.json
//! ```

use std::time::Instant;

use jp_dict::deinflect::algorithm_deinflections;
use jp_dict::text::japanese_chinese_korean_only_prefix;
use jp_dict::transformer::LanguageTransformer;

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("用法: parity_text_pipeline <japanese-text-pipeline-full.json>");
    let cases: Vec<(String, String, Vec<String>)> = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    let lt = LanguageTransformer::japanese();

    let started = Instant::now();
    let mut lines = 0usize;
    let mut prefix_mismatch = Vec::new();
    let mut mismatched = Vec::new();
    for (input, cjk_only, want) in &cases {
        if japanese_chinese_korean_only_prefix(input) != cjk_only {
            prefix_mismatch.push(input);
        }
        let got: Vec<String> = algorithm_deinflections(&lt, input).iter().map(|d| d.parity_line()).collect();
        lines += got.len();
        if &got != want {
            let at = got.iter().zip(want).position(|(g, w)| g != w).unwrap_or(got.len().min(want.len()));
            mismatched.push((input, want.len(), got.len(), want.get(at).cloned(), got.get(at).cloned()));
        }
    }
    let elapsed = started.elapsed();

    let want_lines: usize = cases.iter().map(|(_, _, w)| w.len()).sum();
    println!("输入 {} 条，Rust {} 行，Yomitan {} 行", cases.len(), lines, want_lines);
    println!("截断一致 {} / {}", cases.len() - prefix_mismatch.len(), cases.len());
    println!("管线逐行一致 {} / {}", cases.len() - mismatched.len(), cases.len());
    for (input, w, g, wl, gl) in mismatched.iter().take(8) {
        println!("  {input:?}: Yomitan {w} 行 / Rust {g} 行\n    Y: {wl:?}\n    R: {gl:?}");
    }
    for input in prefix_mismatch.iter().take(8) {
        println!("  截断不一致: {input:?}");
    }
    println!(
        "用时 {:.0} ms（含格式化），平均 {:.2} ms/条",
        elapsed.as_secs_f64() * 1e3,
        elapsed.as_secs_f64() * 1e3 / cases.len() as f64
    );
    Ok(())
}
