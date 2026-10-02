//! 分词词典（Sudachi）放在哪、怎么补上。
//!
//! 振假名、导入时的分词、实时分词都要 `system.dic`（207MB）。0.1.x 的那一份躺在
//! 项目目录的 `venv/Lib/site-packages` 里，所以在开发树上一直都有；新装的程序把库
//! 建在 `%LOCALAPPDATA%\JPOP Corpus Tool`，那里没有 venv——**迁移完数据还是没有
//! 振假名**，就是这个原因。
//!
//! **要用振假名的时候程序自己去下**（`download`）：从本仓库的发布里取
//! `system.dic.xz`（43MB，解开 207MB），校验、解压进语料库目录的 `sudachi/`。
//!
//! 为什么不打进安装包：词典和版本无关，打进去的话安装包从 6.8MB 变 52MB，而且**每次
//! 更新都要重下这 52MB**；单独下的话一辈子只下一次，以后更新还是 6.8MB。
//!
//! 为什么只下一个文件：另外那四个配置文件一共 15KB，直接编进了程序
//! （`jp_tokenizer::EMBEDDED_RESOURCES`），所以不需要任何打包格式。
//!
//! 还有两件事：照实说现在用的是哪一份（`status`），以及把别处的那一份**复制**进当前
//! 语料库目录（`install`，给手上已经有 0.1.x venv 的人省掉这 43MB）。
//! 复制而不是记路径：记路径的话，源目录一删一移，振假名第二天就没了，
//! 而用户不会想到是这个原因。

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
    /// 哪儿来的：`app` 随程序自带、`library` 语料库目录里的 `sudachi/`、`venv` 0.1.x 的那份
    pub source: &'static str,
    /// 导入进来的那一份会放在哪，给界面说明用
    pub bundled_dir: String,
}

pub fn status(root: &Path) -> TokenizerStatus {
    let bundled_dir = root.join(jp_tokenizer::BUNDLED_DIR);
    let in_library = jp_tokenizer::locate_in_library(root);
    let source = if in_library.is_none() {
        "app"
    } else if in_library.as_ref().is_some_and(|(_, dict)| dict.starts_with(&bundled_dir)) {
        "library"
    } else {
        "venv"
    };
    match in_library.or_else(jp_tokenizer::bundled_with_app) {
        Some((_, dict)) => TokenizerStatus {
            ready: true,
            dict_bytes: std::fs::metadata(&dict).map(|m| m.len()).unwrap_or(0),
            source,
            dict_path: Some(dict.display().to_string()),
            bundled_dir: bundled_dir.display().to_string(),
        },
        None => TokenizerStatus {
            ready: false,
            dict_path: None,
            dict_bytes: 0,
            source: "none",
            bundled_dir: bundled_dir.display().to_string(),
        },
    }
}

/// 词典发在哪。
///
/// 单独一个 tag，和应用版本解耦：换了应用版本这个地址不变，已经下好的那一份照用。
pub const DICT_URL: &str = concat!(
    "https://github.com/Yisoragoto/jpop-corpus-tool/releases/download/",
    "sudachi-dict-core/system.dic.xz"
);
/// 压缩包多大（字节），界面上要先告诉用户「这要下 43MB」
pub const DICT_DOWNLOAD_BYTES: u64 = 45_128_544;
/// 压缩包的 SHA-256
pub const DICT_XZ_SHA256: &str = "5ba334f1ac976ae6f77f8f92d26cef9bc44a9c2cf0d723545432fcd43df2db5b";
/// **解压之后**那个 `system.dic` 的 SHA-256。
///
/// 两个都校验：压缩包那个是省事（下错了早点说），这一个才是要紧的——
/// 分词结果和 Python 侧逐 token 对过账，用的就是这一份字节。
pub const DICT_SHA256: &str = "a4c0cfea5674e1e9cfcbd3cf05bb45576b533c3944e60b7dbbea022423a1866a";
/// 解开有多大
pub const DICT_BYTES: u64 = 217_203_456;

/// 下载进度。`stage` 是 `downloading` / `extracting` / `done`。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub received: u64,
    pub total: u64,
    pub stage: &'static str,
}

/// 同一个进程里只下一份：设置里点一次、歌词那条横幅上再点一次，不能下两遍。
static DOWNLOADING: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn sha256_of(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("打不开 {}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).with_context(|| format!("读不完 {}", path.display()))?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// 把词典下到当前语料库目录的 `sudachi/`。
///
/// 下 → 校验压缩包 → 解压 → 校验解开的 `system.dic` → 写那四个配置文件 → 改名就位。
/// 每一步都写在 `.part` 上，中途断了留下的是半截的临时文件，而不是一个
/// 「看起来齐了、加载时才炸」的词典。
pub fn download(root: &Path, mut on_progress: impl FnMut(DownloadProgress)) -> Result<Installed> {
    let _queued = DOWNLOADING.lock().unwrap_or_else(|err| err.into_inner());

    let dest = root.join(jp_tokenizer::BUNDLED_DIR);
    std::fs::create_dir_all(&dest).with_context(|| format!("建不了 {}", dest.display()))?;
    let dic = dest.join("system.dic");
    // 已经下好过就别再下（两条路同时点，第二条走这儿）
    if dic.is_file() && sha256_of(&dic).ok().as_deref() == Some(DICT_SHA256) {
        jp_tokenizer::write_embedded_resources(&dest)?;
        return Ok(Installed { bytes: DICT_BYTES, dir: dest.display().to_string() });
    }

    let xz = dest.join("system.dic.xz.part");
    {
        use std::io::{Read, Write};
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(std::time::Duration::from_secs(10)))
            .build()
            .into();
        let mut response = agent
            .get(DICT_URL)
            .header("User-Agent", crate::update::USER_AGENT)
            .call()
            .with_context(|| format!("下载失败：{DICT_URL}"))?;
        let total = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DICT_DOWNLOAD_BYTES);
        let mut reader = response.body_mut().as_reader();
        let mut file = std::fs::File::create(&xz)
            .with_context(|| format!("建不了 {}", xz.display()))?;
        let mut buffer = vec![0u8; 256 * 1024];
        let mut received = 0u64;
        loop {
            let read = reader.read(&mut buffer).context("下载中断")?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read]).context("写入失败")?;
            received += read as u64;
            on_progress(DownloadProgress { received, total, stage: "downloading" });
        }
        file.flush().ok();
    }

    let got = sha256_of(&xz)?;
    if !got.eq_ignore_ascii_case(DICT_XZ_SHA256) {
        let _ = std::fs::remove_file(&xz);
        anyhow::bail!("下回来的压缩包校验不过（期望 {DICT_XZ_SHA256}，实际 {got}），已删掉");
    }

    on_progress(DownloadProgress { received: 0, total: DICT_BYTES, stage: "extracting" });
    let part = dest.join("system.dic.part");
    {
        let mut input = std::io::BufReader::new(
            std::fs::File::open(&xz).with_context(|| format!("打不开 {}", xz.display()))?,
        );
        let mut output = std::io::BufWriter::new(
            std::fs::File::create(&part).with_context(|| format!("建不了 {}", part.display()))?,
        );
        lzma_rs::xz_decompress(&mut input, &mut output).map_err(|err| {
            anyhow::anyhow!("解压失败：{err}")
        })?;
        use std::io::Write;
        output.flush().ok();
    }
    let _ = std::fs::remove_file(&xz);

    let got = sha256_of(&part)?;
    if !got.eq_ignore_ascii_case(DICT_SHA256) {
        let _ = std::fs::remove_file(&part);
        anyhow::bail!("解开的词典校验不过（期望 {DICT_SHA256}，实际 {got}），已删掉");
    }

    jp_tokenizer::write_embedded_resources(&dest)?;
    std::fs::rename(&part, &dic).with_context(|| format!("改名到 {} 失败", dic.display()))?;

    // 装完当场加载一次，装不上就把这份删掉
    if let Err(err) = jp_tokenizer::Analyzer::from_sudachipy(&dest, &dic) {
        let _ = std::fs::remove_dir_all(&dest);
        anyhow::bail!("下回来的词典加载不了，已删除：{err}");
    }
    on_progress(DownloadProgress { received: DICT_BYTES, total: DICT_BYTES, stage: "done" });
    Ok(Installed { bytes: DICT_BYTES, dir: dest.display().to_string() })
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
    // 已经有能用的词典（库里的，或者随程序自带的那一份）就别搬了——
    // 平白多占 207MB，而且用户看不出区别
    if jp_tokenizer::locate_sudachipy(target_root).is_some() {
        return Ok(None);
    }
    let Some((resources, _)) = jp_tokenizer::locate_in_library(source_root) else {
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
        // 开发树上跑测试时 exe 旁边没有 resources，所以这里是「什么都没有」
        if s.ready {
            assert_eq!(s.source, "app");
        } else {
            assert!(s.dict_path.is_none());
        }
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
        assert!(s.ready && s.source == "library");
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
