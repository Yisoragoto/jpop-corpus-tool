//! 拿真实曲名 / 歌手 / 专辑 / 人名，对比 Rust 和 Python 两边的
//! `matching_key` 结果。
//!
//!     cargo run -p jp-import --example compare_keys -- <keys.tsv>
//!
//! 输入是 Python 侧导出的 `原串<TAB>python_key`。
//!
//! 为什么要对：这个键决定「两条记录是不是同一首歌 / 同一个人」。
//! 两边算得不一样，就会出现 Python 认为重复、Rust 认为是新的这种
//! 分裂——迁移期两边共用一个库，这种分裂会直接把库弄脏。

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("用法: compare_keys <keys.tsv>");
        std::process::exit(2)
    });
    let text = std::fs::read_to_string(&path)?;

    let mut total = 0usize;
    let mut diffs: Vec<(String, String, String)> = Vec::new();
    for line in text.lines() {
        let Some((raw, py)) = line.split_once('\t') else { continue };
        total += 1;
        let rs = jp_import::filename::matching_key(raw);
        if rs != py {
            diffs.push((raw.to_string(), py.to_string(), rs));
        }
    }

    println!("对比 {total} 个字符串");
    println!("  一致 : {}", total - diffs.len());
    println!("  不同 : {}", diffs.len());
    for (raw, py, rs) in diffs.iter().take(20) {
        println!("    {raw:?}\n      python={py:?}\n      rust  ={rs:?}");
    }
    Ok(())
}
