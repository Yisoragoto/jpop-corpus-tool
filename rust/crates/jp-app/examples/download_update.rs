//! 复现「下载更新」那条路：问 Releases、下安装包、校验、改名。
//!
//!     cargo run -p jp-app --example download_update -- <落盘目录> [版本号]
//!
//! 版本号是「假装自己是哪一版」，默认 0.0.1，这样最新的那一版总是算更新。
//! 出错时把整条 anyhow 链打出来——界面上只看得到最外层那句。

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = PathBuf::from(args.first().expect("用法：download_update <目录> [版本号]"));
    let current = args.get(1).map(String::as_str).unwrap_or("0.0.1");

    let status = match jp_app_lib::update::check(current) {
        Ok(status) => status,
        Err(err) => {
            eprintln!("检查更新失败：{err:#}");
            std::process::exit(1);
        }
    };
    let Some(release) = status.latest else {
        eprintln!("没有发布");
        std::process::exit(1);
    };
    let Some(asset) = release.installer else {
        eprintln!("这一版没有安装包");
        std::process::exit(1);
    };
    println!("最新 {} · {} · {} 字节", release.version, asset.name, asset.size);

    let started = std::time::Instant::now();
    let mut last = 0u64;
    match jp_app_lib::update::download_installer(&asset, &dir, |received, total| {
        if received - last > 2_000_000 {
            last = received;
            println!("  {received} / {total}");
        }
    }) {
        Ok(path) => println!("下好了：{} （{:.1}s）", path.display(), started.elapsed().as_secs_f64()),
        Err(err) => {
            // `{:#}` 把整条链都打出来，最后那一截才是操作系统说的话
            eprintln!("失败：{err:#}");
            for (i, cause) in err.chain().enumerate() {
                eprintln!("  [{i}] {cause}");
            }
            std::process::exit(1);
        }
    }
}
