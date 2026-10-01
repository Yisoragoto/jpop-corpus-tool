//! 列出本机字体族名（和 Python 版 `QFontDatabase.families()` 对账用），打印耗时。
//!
//! ```text
//! cargo run --release -p jp-app --example list_fonts -- <输出.json>
//! ```

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    let output = std::env::args()
        .nth(1)
        .context("用法: list_fonts <输出.json>")?;
    let dirs = jp_app_lib::fonts::system_font_dirs();
    let started = std::time::Instant::now();
    let families = jp_app_lib::fonts::installed_fonts()?;
    let elapsed = started.elapsed();
    std::fs::write(&output, serde_json::to_vec_pretty(&families)?)?;
    println!(
        "{} 个字体族，扫描 {:?} 用了 {:.0} ms",
        families.len(),
        dirs,
        elapsed.as_secs_f64() * 1000.0
    );
    Ok(())
}
