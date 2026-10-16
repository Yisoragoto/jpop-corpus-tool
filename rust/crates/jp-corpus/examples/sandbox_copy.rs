//! 把语料库拷一份出来试跑新版。`scripts/try-dev.ps1` 调它，一般不用手动跑。
//!
//!   cargo run --release -p jp-corpus --example sandbox_copy -- <原库目录> <副本目录> [--dictionaries]
//!
//! * `corpus.db` 每次都重拷；`dictionaries.db`（几百 MB）和 `sudachi/` 只在副本里没有、
//!   或者给了 `--dictionaries` 时才拷。
//! * 数据库用 `VACUUM INTO` 拷，不直接复制文件：装好的那一份开着时库处在 WAL 模式，
//!   直接复制 `.db` 会漏掉还在 `-wal` 里的事务，复制到一半有写入还会撕裂。
//!   `VACUUM INTO` 在一个读事务里做，拿到的是一致的快照，原库照常用。
//! * 封面、歌手照片、外挂歌词记的是**绝对路径**，指着原库目录。不改的话副本里的程序
//!   既读不到（asset 放行的是副本目录），也可能把东西写回原库。所以拷完把这些前缀换成副本目录。
//! * `raw/audio` 不拷：音频只读，副本照旧指向原来的文件。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};

/// 存了原库目录下绝对路径的列。音频不在里面：它不拷，路径照旧
const PATH_COLUMNS: [(&str, &str); 4] = [
    ("songs", "cover_path"),
    ("songs", "source_file"),
    ("albums", "artwork_path"),
    ("artists", "image_path"),
];

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let positional: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [src, dst] = positional.as_slice() else {
        bail!("用法：sandbox_copy <原库目录> <副本目录> [--dictionaries]");
    };
    // 路径要原样（只补成绝对路径）：`canonicalize` 会改大小写、加 `\\?\` 前缀，
    // 改写进库的路径和程序启动时放行的目录就对不上了。canonicalize 只拿来判断两个目录是不是同一个
    let src = std::path::absolute(src)?;
    let dst = std::path::absolute(dst)?;
    let with_dictionaries = args.iter().any(|a| a == "--dictionaries");

    if !src.join("corpus.db").is_file() {
        bail!("{} 里没有 corpus.db", src.display());
    }
    std::fs::create_dir_all(&dst)?;
    let (real_src, real_dst) = (std::fs::canonicalize(&src)?, std::fs::canonicalize(&dst)?);
    if real_dst == real_src || real_dst.starts_with(&real_src) {
        bail!("副本目录不能是原库目录或它的子目录：{}", dst.display());
    }

    let started = std::time::Instant::now();
    snapshot(&src.join("corpus.db"), &dst.join("corpus.db"))?;
    let rewritten = rewrite_paths(&dst.join("corpus.db"), &src.display().to_string(), &dst.display().to_string())?;
    println!("corpus.db：拷好了，改了 {rewritten} 个路径（{:.1} 秒）", started.elapsed().as_secs_f64());

    let dict = src.join("dictionaries.db");
    if dict.is_file() && (with_dictionaries || !dst.join("dictionaries.db").is_file()) {
        let started = std::time::Instant::now();
        snapshot(&dict, &dst.join("dictionaries.db"))?;
        println!("dictionaries.db：拷好了（{:.1} 秒）", started.elapsed().as_secs_f64());
    }
    if src.join("sudachi").is_dir() && (with_dictionaries || !dst.join("sudachi").is_dir()) {
        let n = copy_dir(&src.join("sudachi"), &dst.join("sudachi"), &[])?;
        println!("sudachi/：拷了 {n} 个文件");
    }
    if src.join("raw").is_dir() {
        // 每次都重拷，和 corpus.db 里的路径对得上；音频不拷
        let _ = std::fs::remove_dir_all(dst.join("raw"));
        let n = copy_dir(&src.join("raw"), &dst.join("raw"), &["audio"])?;
        println!("raw/：拷了 {n} 个文件（封面、歌手照片、歌词）");
    }
    Ok(())
}

/// 一致快照。目标已存在就先删掉——`VACUUM INTO` 不肯覆盖。
fn snapshot(src: &Path, dst: &Path) -> Result<()> {
    for suffix in ["", "-wal", "-shm"] {
        let mut name = dst.as_os_str().to_owned();
        name.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(name));
    }
    let conn = Connection::open(src).with_context(|| format!("打不开 {}", src.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    conn.execute("VACUUM INTO ?1", params![dst.to_string_lossy()])
        .with_context(|| format!("拷不出 {}", src.display()))?;
    Ok(())
}

fn rewrite_paths(db: &Path, from: &str, to: &str) -> Result<usize> {
    let conn = Connection::open(db)?;
    let mut total = 0;
    for (table, column) in PATH_COLUMNS {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2",
            params![table, column],
            |r| r.get(0),
        )?;
        if exists == 0 {
            continue;
        }
        // substr 比 replace 稳：只换开头那一段，路径中间碰巧出现同样的字串不会被误改
        total += conn.execute(
            &format!(
                "UPDATE {table} SET {column} = ?2 || substr({column}, length(?1) + 1) \
                 WHERE substr({column}, 1, length(?1)) = ?1 COLLATE NOCASE"
            ),
            params![from, to],
        )?;
    }
    Ok(total)
}

fn copy_dir(src: &Path, dst: &Path, skip: &[&str]) -> Result<usize> {
    std::fs::create_dir_all(dst)?;
    let mut n = 0;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if skip.iter().any(|s| name.eq_ignore_ascii_case(s)) {
            continue;
        }
        let target = dst.join(&name);
        if entry.file_type()?.is_dir() {
            n += copy_dir(&entry.path(), &target, &[])?;
        } else {
            std::fs::copy(entry.path(), &target)
                .with_context(|| format!("拷不了 {}", entry.path().display()))?;
            n += 1;
        }
    }
    Ok(n)
}
