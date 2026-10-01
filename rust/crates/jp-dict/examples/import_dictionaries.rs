//! 把真实词典包导入到一个**新建的**词典库文件，统计用时、条数、体积。
//! 只读词典包本身；输出文件必须不存在，绝不覆盖任何东西，也拒绝名为 corpus.db 的路径。
//!
//! ```text
//! cargo run -p jp-dict --release --example import_dictionaries -- <新的 .db> <zip 或目录>... | @<每行一个路径的列表文件>
//! ```

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use jp_dict::import::import_path;
use jp_dict::store::DictionaryStore;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().context("用法: import_dictionaries <新的 .db> <zip 或目录>... | @列表文件")?);
    if out.file_name().is_some_and(|n| n.eq_ignore_ascii_case("corpus.db")) {
        bail!("拒绝写 corpus.db");
    }
    if out.exists() {
        bail!("{} 已存在；为了不覆盖任何东西，请给一个新文件名", out.display());
    }

    let mut paths = Vec::new();
    for arg in args {
        if let Some(list) = arg.strip_prefix('@') {
            let text = std::fs::read_to_string(list).with_context(|| format!("读不了列表文件 {list}"))?;
            paths.extend(text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned));
        } else {
            paths.push(arg);
        }
    }

    let mut store = DictionaryStore::open(&out)?;
    let all = Instant::now();
    let (mut ok, mut failed) = (0, 0);
    for path in &paths {
        let name: String = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut last = Instant::now();
        let result = import_path(&mut store, Path::new(path), &mut |p| {
            if last.elapsed().as_secs() >= 10 {
                eprintln!("  … {name}: {}/{} {}", p.done_files, p.total_files, p.file);
                last = Instant::now();
            }
        });
        match result {
            Ok(s) => {
                ok += 1;
                let meta: Vec<String> = s.term_meta.iter().filter(|(k, _)| *k != "total").map(|(k, v)| format!("{k}:{v}")).collect();
                println!(
                    "{:<24} 条目 {:>7}  meta {:<18} 标签 {:>4}  图片 {:>4}  释义 {:>7.1} MB → {:>6.1} MB ({:>4.1}%)  {:>6.1} s  警告 {}",
                    s.title.chars().take(24).collect::<String>(),
                    s.terms,
                    meta.join(" "),
                    s.tags,
                    s.media,
                    s.glossary_bytes as f64 / 1e6,
                    s.compressed_bytes as f64 / 1e6,
                    if s.glossary_bytes > 0 { s.compressed_bytes as f64 * 100.0 / s.glossary_bytes as f64 } else { 0.0 },
                    s.elapsed_ms as f64 / 1e3,
                    s.warning_count
                );
                for w in s.warnings.iter().take(3) {
                    println!("    ! {w}");
                }
            }
            Err(e) => {
                failed += 1;
                println!("{name}: 导入失败：{e:#}");
            }
        }
    }
    drop(store);
    println!(
        "成功 {ok}，失败 {failed}，总用时 {:.1} s，词典库 {:.1} MB",
        all.elapsed().as_secs_f64(),
        std::fs::metadata(&out)?.len() as f64 / 1e6
    );
    Ok(())
}
