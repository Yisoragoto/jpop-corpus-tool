//! 两个客户端共用的歌词管理：版本、文件、候选与可取消升级队列。
mod job;
pub mod lyrics;
pub mod timed_lyrics;
pub mod lyric_upgrade;
pub mod management;
pub mod sources;

use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel { Warn, Error }

static LOGGER: OnceLock<fn(LogLevel, &str)> = OnceLock::new();

/// 接上客户端日志，只认第一次；FFI 默认写 stderr。
pub fn set_logger(logger: fn(LogLevel, &str)) {
    let _ = LOGGER.set(logger);
}

fn log(level: LogLevel, message: impl std::fmt::Display) {
    let text = message.to_string();
    match LOGGER.get() {
        Some(logger) => logger(level, &text),
        None => eprintln!("[jp-lyrics {level:?}] {text}"),
    }
}

fn log_warn(message: impl std::fmt::Display) { log(LogLevel::Warn, message); }
fn log_error(message: impl std::fmt::Display) { log(LogLevel::Error, message); }

#[cfg(test)]
mod logging_tests {
    use super::*;
    static MESSAGES: std::sync::Mutex<Vec<(LogLevel, String)>> = std::sync::Mutex::new(Vec::new());

    #[test]
    fn logger_preserves_levels_first_hook_and_panic_without_callback() {
        set_logger(|level, message| MESSAGES.lock().unwrap_or_else(|e| e.into_inner()).push((level, message.into())));
        set_logger(|_, _| panic!("只能接上第一个日志钩子"));
        log_warn("歌词日志测试 warn");
        log_error("歌词日志测试 error");
        assert!(std::thread::spawn(|| {
            let job = std::sync::Arc::new(crate::job::Job::default());
            let _guard = job.start().unwrap();
            panic!("日志测试");
        }).join().is_err());
        let messages = MESSAGES.lock().unwrap();
        assert!(messages.contains(&(LogLevel::Warn, "歌词日志测试 warn".into())));
        assert!(messages.contains(&(LogLevel::Error, "歌词日志测试 error".into())));
        assert!(messages.contains(&(LogLevel::Error, "后台作业线程 panic 了，运行标志已经放开".into())));
    }
}
