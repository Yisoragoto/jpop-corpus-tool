//! 下载：断了接着下。
//!
//! 两处在用——装更新的安装包（`update.rs`）和分词词典（`tokenizer.rs`）。本来各写各的，
//! 直到两边都被同一件事咬到：**本机到 GitHub 只有几十 KB/s，几 MB 的文件也会被掐断**。
//! 实测词典 43MB 在 24MB 处 `Peer disconnected`，安装包 6.8MB 同样下不完。
//! 没有续传的话，用户每次都从 0 开始，而下一次多半还是断在半路。
//!
//! 所以这里只做一件事，做对：
//!
//! - `.part` **不删**，它就是断点；下一轮带 `Range: bytes=N-` 接着下；
//! - 服务端不认 Range（回 200 而不是 206）就退回从头下，不会把两段拼错；
//! - 进度报的是**绝对**字节数，界面上那根条不会退回 0；
//! - 退避 1、2、3…最多 8 秒——刚断网时立刻重连多半还是断的；
//! - 最后仍然要校验 SHA-256。续传拼出来的文件是不是完整的，由调用方那一步说了算。

use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result};

/// 断了接着下，最多接这么多次
pub const RESUME_ATTEMPTS: usize = 12;

const USER_AGENT: &str = "jpop-corpus-tool";

/// 下载进度：收到多少 / 一共多少（都是绝对字节）
pub type OnProgress<'a> = &'a mut dyn FnMut(u64, u64);

/// 把 `url` 下到 `part`，断了从断点接着下。
///
/// `expected_total` 是发布时记下的大小，用来判断「下完了没有」和报进度；
/// 服务端给的 `content-length` 只当参考——206 响应里它是**剩下**那段的长度。
pub fn download_resumable(
    url: &str,
    part: &Path,
    expected_total: u64,
    on_progress: OnProgress<'_>,
) -> Result<()> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(15)))
        // 不设总超时：慢网上这些文件本来就要几分钟到二十分钟
        .build()
        .into();

    let mut received = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    // 比发布时记下的还长，说明这个 .part 不是这一份，重来
    if received > expected_total {
        let _ = std::fs::remove_file(part);
        received = 0;
    }
    let mut last_error = String::new();

    for attempt in 0..RESUME_ATTEMPTS {
        if received >= expected_total {
            return Ok(());
        }
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs((attempt as u64).min(8)));
        }

        let mut request = agent.get(url).header("User-Agent", USER_AGENT);
        if received > 0 {
            request = request.header("Range", &format!("bytes={received}-"));
        }
        let mut response = match request.call() {
            Ok(response) => response,
            Err(err) => {
                last_error = err.to_string();
                continue;
            }
        };

        // 206 才是「接着给」；回 200 说明它不认 Range，整个文件又来一遍，只能从头写
        let resumed = response.status().as_u16() == 206;
        if received > 0 && !resumed {
            received = 0;
        }
        let mut file = if received > 0 {
            std::fs::OpenOptions::new()
                .append(true)
                .open(part)
                .with_context(|| format!("打不开 {}", part.display()))?
        } else {
            if let Some(parent) = part.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::File::create(part).with_context(|| format!("建不了 {}", part.display()))?
        };

        let mut reader = response.body_mut().as_reader();
        let mut buffer = vec![0u8; 256 * 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    file.write_all(&buffer[..read]).context("写入失败")?;
                    received += read as u64;
                    on_progress(received, expected_total);
                }
                Err(err) => {
                    // 断在半路：已经收到的刷到盘上，下一轮从这儿接着下
                    last_error = err.to_string();
                    break;
                }
            }
        }
        file.flush().ok();
        drop(file);
        // 以盘上的真实长度为准
        received = std::fs::metadata(part).map(|m| m.len()).unwrap_or(received);
    }

    if received >= expected_total {
        return Ok(());
    }
    anyhow::bail!(
        "下载中断，续了 {RESUME_ATTEMPTS} 次还是没下完（{:.1} / {:.1} MB）。已经下到的部分留着了，再试一次会从断点接着下。最后一次的报错：{last_error}",
        received as f64 / 1024.0 / 1024.0,
        expected_total as f64 / 1024.0 / 1024.0
    )
}

/// 把临时文件变成最终文件。
///
/// 刚写完的 exe 可能还被杀软扫着，`rename` 会吃一个「拒绝访问」；retry 几次通常就过了。
/// 真过不去就复制一份——复制开的是新句柄，很多锁得住改名的情况它能过。
/// **错误里带上操作系统说的那句话**：只写「改名失败：<路径>」的话，排查时等于什么都没说。
pub fn finish(part: &Path, dest: &Path) -> Result<()> {
    let mut last: Option<std::io::Error> = None;
    for attempt in 0..10 {
        match std::fs::rename(part, dest) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last = Some(err);
                std::thread::sleep(std::time::Duration::from_millis(100 * (attempt + 1)));
            }
        }
    }
    match std::fs::copy(part, dest) {
        Ok(_) => {
            let _ = std::fs::remove_file(part);
            Ok(())
        }
        Err(copy_err) => {
            let _ = std::fs::remove_file(part);
            let rename_err = last
                .map(|e| e.to_string())
                .unwrap_or_else(|| "（没有记录）".to_string());
            anyhow::bail!(
                "落盘失败：{}\n改名：{rename_err}\n复制：{copy_err}",
                dest.display()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jp-net-{}-{:?}-{tag}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_already_complete_part_file_needs_no_network() {
        let dir = dir("complete");
        let part = dir.join("x.part");
        std::fs::write(&part, vec![0u8; 1024]).unwrap();
        // 地址是假的：真去连就会失败，说明它没走网络那条路
        download_resumable("https://example.invalid/x", &part, 1024, &mut |_, _| {}).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_part_file_longer_than_expected_is_thrown_away() {
        let dir = dir("too-long");
        let part = dir.join("x.part");
        std::fs::write(&part, vec![0u8; 4096]).unwrap();
        // 比发布时记下的长 → 删掉重来 → 然后连不上假地址，报错
        let err = download_resumable("https://example.invalid/x", &part, 1024, &mut |_, _| {})
            .unwrap_err()
            .to_string();
        assert!(err.contains("下载中断"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finish_moves_the_temp_file_into_place_even_if_the_target_exists() {
        let dir = dir("finish");
        let part = dir.join("x.part");
        let dest = dir.join("x.exe");
        std::fs::write(&part, b"new").unwrap();
        std::fs::write(&dest, b"old").unwrap();
        finish(&part, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        assert!(!part.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
