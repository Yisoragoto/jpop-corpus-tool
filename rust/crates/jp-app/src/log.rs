//! 往文件里记一份日志。
//!
//! **为什么需要**：这是个 GUI 程序，没有控制台，`eprintln!` 等于说给空气听。
//! 用户机器上出的事（更新下载断了、词典装不上、启动时找库找错了）在他那边
//! 只剩一行红条，在我这边什么都没有——这一轮排查 0.2.6 的「改名失败」时
//! 就是靠猜，因为报错把操作系统原话吞了，而且没有任何日志留下来。
//!
//! 做得很克制，只做够用的那一点：
//!
//! - 一个文件 `<语料库目录>/logs/jp-app.log`，追加写；超过 2MB 轮一次，留一份 `.1`；
//! - 写不进去就当没有日志（磁盘满、只读目录、权限）——**日志不能让应用起不来**；
//! - 每行都同时打到 stderr，开发时照旧能看见；
//! - `tail` 给「导出诊断信息」用。
//!
//! 不做的：级别过滤、异步、结构化字段。真需要的时候再说。

use std::fmt::Display;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 轮转阈值。一次启动写不了几行，2MB 够留很久了
const MAX_BYTES: u64 = 2 * 1024 * 1024;

struct Sink {
    path: PathBuf,
    file: Option<std::fs::File>,
}

static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

/// 日志落在哪。语料库目录下面，和 `raw/`、`dictionaries.db` 挨着——
/// 「关于」页已经把这个目录显示给用户了，出事时说得清去哪儿找。
pub fn log_path(root: &Path) -> PathBuf {
    root.join("logs").join("jp-app.log")
}

/// 开日志。应用启动时调一次；调之前的 `note` 只会打到 stderr。
pub fn init(root: &Path) {
    let path = log_path(root);
    let file = open(&path);
    let _ = SINK.set(Mutex::new(Sink { path, file }));
}

fn open(path: &Path) -> Option<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()?;
    // 太大就轮一次：旧的改名成 .1（已有的那份直接盖掉），新的从头开始
    if file.seek(SeekFrom::End(0)).unwrap_or(0) > MAX_BYTES {
        drop(file);
        let old = path.with_extension("1.log");
        let _ = std::fs::remove_file(&old);
        let _ = std::fs::rename(path, &old);
        file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()?;
    }
    Some(file)
}

/// 记一行。`level` 自己写成 `info` / `warn` / `error`。
pub fn note(level: &str, message: impl Display) {
    let line = format!("{} [{level}] {message}", utc_now());
    eprintln!("{line}");
    let Some(sink) = SINK.get() else { return };
    let Ok(mut sink) = sink.lock() else { return };
    if sink.file.is_none() {
        // 上次写失败过，再试一次：用户可能刚把盘腾出来
        sink.file = open(&sink.path.clone());
    }
    if let Some(file) = sink.file.as_mut()
        && writeln!(file, "{line}").is_err()
    {
        // 写不进去就放弃这个句柄，下次再开。**不 panic、不往上报**
        sink.file = None;
    }
}

pub fn info(message: impl Display) {
    note("info", message);
}
pub fn warn(message: impl Display) {
    note("warn", message);
}
pub fn error(message: impl Display) {
    note("error", message);
}

/// 最后 `lines` 行，给「导出诊断信息」用。读不到就空着。
pub fn tail(lines: usize) -> Vec<String> {
    let Some(sink) = SINK.get() else {
        return Vec::new();
    };
    let Ok(sink) = sink.lock() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&sink.path) else {
        return Vec::new();
    };
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

/// `2026-10-03T00:41:07Z`。
///
/// 自己算而不是拉一个日期库进来：只要这一个格式，而且**日志的时间戳错一秒
/// 不影响任何判断**。算法是 Howard Hinnant 的 civil_from_days。
pub fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch_and_a_known_date_come_out_right() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2026-10-03 距 1970-01-01 共 20729 天
        assert_eq!(civil_from_days(20_729), (2026, 10, 3));
        // 闰年的 2 月 29
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn the_timestamp_looks_like_a_timestamp() {
        let now = utc_now();
        assert_eq!(now.len(), 20, "{now}");
        assert!(now.ends_with('Z') && now.contains('T'), "{now}");
    }

    /// 没 init 过也不能炸——命令层到处在调它
    #[test]
    fn logging_before_init_only_goes_to_stderr() {
        note("info", "这一行只该出现在 stderr");
        assert!(tail(5).is_empty());
    }
}
