//! 分词词典（Sudachi）放在哪、怎么补上。
//!
//! 振假名、导入时的分词、实时分词都要 `system.dic`（207MB）。0.1.x 的那一份躺在
//! 项目目录的 `venv/Lib/site-packages` 里，所以在开发树上一直都有；新装的程序把库
//! 建在 `%LOCALAPPDATA%\JPOP Corpus Tool`，那里没有 venv——**迁移完数据还是没有
//! 振假名**，就是这个原因。
//!
//! 这里做两件事：照实说现在有没有（`status`），以及把别处的那一份**复制**进当前
//! 语料库目录的 `sudachi/`（`install`）。复制而不是记路径：记路径的话，源目录一删
//! 一移，振假名第二天就没了，而用户不会想到是这个原因。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

/// 当前语料库目录里分词词典的状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenizerStatus {
    /// 词典在不在（在 = 振假名、实时分词可用）
    pub ready: bool,
    /// 用的是哪一份：`system.dic` 的完整路径；没有时为 `None`
    pub dict_path: Option<String>,
    /// 这一份有多大，字节
    pub dict_bytes: u64,
    /// 是不是语料库目录自带的那一份（`sudachi/`）。false 表示用的是 0.1.x 的 venv
    pub bundled: bool,
    /// 自带的那一份会放在哪，给界面说明用
    pub bundled_dir: String,
}

pub fn status(root: &Path) -> TokenizerStatus {
    let bundled_dir = root.join(jp_tokenizer::BUNDLED_DIR);
    match jp_tokenizer::locate_sudachipy(root) {
        Some((_, dict)) => TokenizerStatus {
            ready: true,
            dict_bytes: std::fs::metadata(&dict).map(|m| m.len()).unwrap_or(0),
            bundled: dict.starts_with(&bundled_dir),
            dict_path: Some(dict.display().to_string()),
            bundled_dir: bundled_dir.display().to_string(),
        },
        None => TokenizerStatus {
            ready: false,
            dict_path: None,
            dict_bytes: 0,
            bundled: false,
            bundled_dir: bundled_dir.display().to_string(),
        },
    }
}

/// 复制结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    /// 复制了多少字节（主要是 system.dic）
    pub bytes: u64,
    /// 落在哪
    pub dir: String,
}

/// 把 `from` 目录里的那一份 Sudachi 词典复制进 `root/sudachi/`。
///
/// `from` 选哪一层都行，见 [`jp_tokenizer::find_sudachipy_source`]。
/// 先写 `.part` 再改名：207MB 复制到一半断电/取消时，留下的是一个半截的 `.part`，
/// 而不是一个「看起来齐了、加载时才炸」的 `system.dic`。
pub fn install(root: &Path, from: &Path) -> Result<Installed> {
    let (resources, dict) = jp_tokenizer::find_sudachipy_source(from).with_context(|| {
        format!(
            "{} 里没有 Sudachi 词典（要找的是 sudachi.json 和 system.dic）",
            from.display()
        )
    })?;

    let dest = root.join(jp_tokenizer::BUNDLED_DIR);
    std::fs::create_dir_all(&dest).with_context(|| format!("建不了 {}", dest.display()))?;

    let mut bytes = 0u64;
    for name in jp_tokenizer::RESOURCE_FILES {
        let src = resources.join(name);
        // rewrite.def 之外都是必需的；缺了就让 copy 自己报错，别假装成功
        if !src.is_file() && name == "rewrite.def" {
            continue;
        }
        bytes += copy_atomically(&src, &dest.join(name))?;
    }
    bytes += copy_atomically(&dict, &dest.join("system.dic"))?;

    // 复制完立刻验一次：加载不起来就把这份删掉，免得它顶掉下次的查找
    if let Err(err) = jp_tokenizer::Analyzer::from_sudachipy(&dest, &dest.join("system.dic")) {
        let _ = std::fs::remove_dir_all(&dest);
        anyhow::bail!("复制过来的词典加载不了，已删除：{err}");
    }

    Ok(Installed {
        bytes,
        dir: dest.display().to_string(),
    })
}

fn copy_atomically(src: &Path, dest: &Path) -> Result<u64> {
    let part = dest.with_extension("part");
    let bytes = std::fs::copy(src, &part)
        .with_context(|| format!("复制 {} 失败", src.display()))?;
    std::fs::rename(&part, dest).with_context(|| format!("改名到 {} 失败", dest.display()))?;
    Ok(bytes)
}

/// 迁移时顺带把源库的那一份搬过来。
///
/// 当前库已经有了就不动（返回 `Ok(None)`）——和「词典库已经有了就不覆盖」一个道理。
pub fn copy_from_library(source_root: &Path, target_root: &Path) -> Result<Option<Installed>> {
    if jp_tokenizer::locate_sudachipy(target_root).is_some() {
        return Ok(None);
    }
    let Some((resources, _)) = jp_tokenizer::locate_sudachipy(source_root) else {
        return Ok(None);
    };
    install(target_root, &resources_parent(&resources)).map(Some)
}

/// `install` 认得资源目录本身，但 0.1.x 的资源目录和 system.dic 不在一起
/// （`sudachipy/resources` vs `sudachidict_core/resources`），要从 site-packages 那一层给它。
fn resources_parent(resources: &Path) -> PathBuf {
    // …/site-packages/sudachipy/resources → …/site-packages
    match resources.parent().and_then(|p| p.parent()) {
        Some(site) if site.join("sudachidict_core/resources/system.dic").is_file() => {
            site.to_path_buf()
        }
        _ => resources.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jp-tok-{}-{:?}-{tag}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_empty_library_reports_not_ready_and_says_where_it_would_go() {
        let root = dir("empty");
        let s = status(&root);
        assert!(!s.ready);
        assert!(s.dict_path.is_none());
        assert!(s.bundled_dir.ends_with("sudachi"));
    }

    #[test]
    fn a_directory_without_a_dictionary_is_refused_by_name() {
        let root = dir("refuse-root");
        let from = dir("refuse-from");
        let err = install(&root, &from).unwrap_err().to_string();
        assert!(err.contains("没有 Sudachi 词典"), "{err}");
        // 失败不留垃圾目录
        assert!(!root.join("sudachi").exists());
    }

    /// 真词典在开发树里；没有就跳过（CI 上没有 venv）
    #[test]
    fn installing_the_real_dictionary_makes_the_library_ready() {
        let source = Path::new(r"D:\jp_corpus");
        if jp_tokenizer::locate_sudachipy(source).is_none() {
            eprintln!("跳过：本机没有 0.1.x 的 venv");
            return;
        }
        let root = dir("install-real");
        let installed = install(&root, source).unwrap();
        assert!(installed.bytes > 100_000_000, "{} 字节太小", installed.bytes);

        let s = status(&root);
        assert!(s.ready && s.bundled);
        // 装完真的能分词
        let (res, dict) = jp_tokenizer::locate_sudachipy(&root).unwrap();
        let analyzer = jp_tokenizer::Analyzer::from_sudachipy(&res, &dict).unwrap();
        let tokens = analyzer.analyze("夜行列車").unwrap();
        assert!(!tokens.is_empty());

        // 已经有了就不重复搬
        assert!(copy_from_library(source, &root).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
