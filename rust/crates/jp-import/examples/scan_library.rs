//! 拿真实曲库跑一遍扫描，看读出来的东西对不对。
//!
//!     cargo run -p jp-import --example scan_library [目录]
//!
//! 单元测试用的是构造数据；这个例子跑真实文件，
//! 能暴露「tag 读法在这批文件上不适用」这类只有真数据才有的问题。

fn main() -> anyhow::Result<()> {
    let root = std::env::args().nth(1).unwrap_or_else(|| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap()
            .join("raw/audio")
            .display()
            .to_string()
    });

    let started = std::time::Instant::now();
    let tracks = jp_import::scan_dir(std::path::Path::new(&root), 6);
    println!(
        "扫描 {} → {} 个文件，用时 {:?}",
        root,
        tracks.len(),
        started.elapsed()
    );

    let no_title = tracks.iter().filter(|t| !t.has_identity()).count();
    let no_tag = tracks.iter().filter(|t| t.tag_title.is_empty()).count();
    let with_lrc = tracks.iter().filter(|t| t.lyrics_path.is_some()).count();
    let warned = tracks.iter().filter(|t| t.warning.is_some()).count();
    let no_duration = tracks.iter().filter(|t| t.duration_sec.is_none()).count();
    println!("  没有内嵌曲名 tag : {no_tag}");
    println!("  连文件名也定不出 : {no_title}");
    println!("  找到 .lrc        : {with_lrc}");
    println!("  读取有警告       : {warned}");
    println!("  读不出时长       : {no_duration}");

    println!("\n前 6 条：");
    for t in tracks.iter().take(6) {
        println!(
            "  {:<26} | {:<22} | {:>7} | lrc={}",
            truncate(t.title(), 24),
            truncate(t.artist(), 20),
            t.duration_sec
                .map(|d| format!("{d:.0}s"))
                .unwrap_or_else(|| "—".into()),
            if t.lyrics_path.is_some() { "有" } else { "无" }
        );
    }
    if no_title > 0 {
        println!("\n定不出曲名的（前 5）：");
        for t in tracks.iter().filter(|t| !t.has_identity()).take(5) {
            println!("  {}", t.path);
        }
    }
    Ok(())
}

fn truncate(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        s.to_string()
    } else {
        chars[..n].iter().collect::<String>() + "…"
    }
}
