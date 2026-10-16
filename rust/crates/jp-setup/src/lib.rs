//! 客户端共用的「准备工作」：断点续传下载、Sudachi 分词词典的定位 / 下载 / 复制。
//!
//! 和界面框架无关。Windows 的 Tauri 客户端（`jp-app`）和 macOS 的原生客户端（经 `jp-ffi`）
//! 走的是同一份代码：词典放在语料库目录的 `sudachi/`、下的是同一个发布、校验同一个哈希，
//! 所以两边分出来的词一模一样。

pub mod net;
pub mod sudachi;

use std::path::Path;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// 文件的 SHA-256（小写十六进制）。下载完的安装包、词典都靠它验。
pub fn sha256_of(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("打不开 {}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .with_context(|| format!("读不完 {}", path.display()))?;
    Ok(format!("{:x}", hasher.finalize()))
}
