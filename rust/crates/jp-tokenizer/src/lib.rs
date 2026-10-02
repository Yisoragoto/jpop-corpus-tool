//! 日语分词 + UPOS 映射。
//!
//! 用 sudachi.rs 复现 Python 侧 SudachiPy 的行为。验收标准是
//! **逐 token 一致**——同一个词典、同一份配置、同一个 SplitMode，
//! 差得多就说明哪里配错了。见 `docs/tokenizer.md`。
//!
//! 关键：必须复用 SudachiPy 的那份 `sudachi.json`，因为里面的
//! `inputTextPlugin`（尤其是把 `〜` 归一成 `ー` 的 ProlongedSoundMarkPlugin）
//! 会改变分词结果。只给同一个 `system.dic` 是不够的。

pub mod furigana;
pub mod pos_map;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sudachi::analysis::Tokenize;
use sudachi::analysis::stateless_tokenizer::StatelessTokenizer;
use sudachi::config::ConfigBuilder;
use sudachi::dic::dictionary::JapaneseDictionary;
use sudachi::prelude::Mode;

pub use pos_map::{DEFAULT_UPOS, is_content_word, is_ignored, to_upos};

/// 一个词元。字段和 SudachiPy 的 `Morpheme` 一一对应，方便逐条比对。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Token {
    pub surface: String,
    /// 对应 SudachiPy 的 `dictionary_form()`
    pub lemma: String,
    /// 对应 SudachiPy 的 `normalized_form()`
    pub normalized: String,
    /// 对应 SudachiPy 的 `reading_form()`
    pub reading: String,
    pub pos: Vec<String>,
    pub upos: String,
}

/// 分词器。持有词典，`analyze` 可反复调用。
pub struct Analyzer {
    dict: JapaneseDictionary,
}

impl Analyzer {
    /// 从 SudachiPy 的安装目录构造——直接复用它的配置和词典，
    /// 这是保证两边一致最省事也最可靠的做法。
    ///
    /// * `resource_dir` —— 含 `sudachi.json` / `char.def` / `unk.def` 的目录
    ///   （`site-packages/sudachipy/resources`）
    /// * `system_dict` —— `site-packages/sudachidict_core/resources/system.dic`
    pub fn from_sudachipy(resource_dir: &Path, system_dict: &Path) -> Result<Self> {
        let config_path = resource_dir.join("sudachi.json");
        let config = ConfigBuilder::from_file(&config_path)
            .with_context(|| format!("读不到配置 {}", config_path.display()))?
            .resource_path(resource_dir)
            .system_dict(system_dict)
            .build();
        let dict = JapaneseDictionary::from_cfg(&config)
            .map_err(|e| anyhow::anyhow!("加载词典失败: {e}"))?;
        Ok(Self { dict })
    }

    /// 按 SplitMode C 分词。C 是最长单位，和 Python 侧用的一致。
    pub fn analyze(&self, text: &str) -> Result<Vec<Token>> {
        let tokenizer = StatelessTokenizer::new(&self.dict);
        let morphemes = tokenizer
            .tokenize(text, Mode::C, false)
            .map_err(|e| anyhow::anyhow!("分词失败: {e}"))?;

        Ok(morphemes
            .iter()
            .map(|m| {
                let pos: Vec<String> = m.part_of_speech().to_vec();
                let upos = to_upos(&pos).to_string();
                Token {
                    surface: m.surface().to_string(),
                    lemma: m.dictionary_form().to_string(),
                    normalized: m.normalized_form().to_string(),
                    reading: m.reading_form().to_string(),
                    pos,
                    upos,
                }
            })
            .collect())
    }
}

/// 语料库目录里自带的那一份词典放在哪（`locate_sudachipy` 的第二条路）。
pub const BUNDLED_DIR: &str = "sudachi";

/// 资源目录里必须有的那几个文件。只有 `system.dic` 大（207MB），其余都是几 KB。
pub const RESOURCE_FILES: [&str; 4] = ["sudachi.json", "char.def", "unk.def", "rewrite.def"];

/// 那四个几 KB 的配置文件**直接编进程序**（一共 15KB）。
///
/// 这样「补一份词典」只剩下载 `system.dic` 这一件事：不用打包格式、不用解压目录结构，
/// 下回来一个文件、把这四个写出去就齐了。
///
/// 它们必须和 `system.dic` 配套——`sudachi.json` 里的 `inputTextPlugin`
/// （尤其是把 `〜` 归一成 `ー` 的那个）会改变分词结果，换一份就对不上账了。
pub const EMBEDDED_RESOURCES: [(&str, &[u8]); 4] = [
    ("sudachi.json", include_bytes!("../resources/sudachi.json")),
    ("char.def", include_bytes!("../resources/char.def")),
    ("unk.def", include_bytes!("../resources/unk.def")),
    ("rewrite.def", include_bytes!("../resources/rewrite.def")),
];

/// 把编进程序的那四个配置文件写进 `dir`。
pub fn write_embedded_resources(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, bytes) in EMBEDDED_RESOURCES {
        std::fs::write(dir.join(name), bytes)?;
    }
    Ok(())
}

/// 一个目录是不是「资源目录 + system.dic 都在这儿」的那种布局。
fn layout(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let dict = dir.join("system.dic");
    (dir.join("sudachi.json").is_file() && dict.is_file()).then(|| (dir.to_path_buf(), dict))
}

/// 语料库目录里的那一份（不含程序自带的）。
///
/// 两条路，按顺序试：
///
/// 1. `venv/Lib/site-packages/…`：和 0.1.x 共用同一份词典，也顺带保证了比对的是
///    同一个数据源；
/// 2. `sudachi/`：语料库目录里自带的一份（迁移时搬过来的，或者设置里导入的）。
pub fn locate_in_library(project_root: &Path) -> Option<(PathBuf, PathBuf)> {
    let site = project_root.join("venv/Lib/site-packages");
    let venv = (
        site.join("sudachipy/resources"),
        site.join("sudachidict_core/resources/system.dic"),
    );
    if venv.0.join("sudachi.json").is_file() && venv.1.is_file() {
        return Some(venv);
    }
    layout(&project_root.join(BUNDLED_DIR))
}

/// 随程序一起装的那一份：`<exe 目录>/resources/sudachi/`（Tauri 的 `bundle.resources`）。
///
/// **振假名不该要用户自己准备词典**——0.1.x 的词典就在项目目录里，装了就有。
/// 开发树上跑 `cargo run` 时 exe 旁边没有 resources，这时返回 `None`，
/// 由上面那条 venv 的路兜住。
pub fn bundled_with_app() -> Option<(PathBuf, PathBuf)> {
    let exe = std::env::current_exe().ok()?;
    layout(&exe.parent()?.join("resources").join(BUNDLED_DIR))
}

/// 用哪一份词典。语料库目录里的优先——用户自己导过就按他导的来；
/// 都没有才用程序自带的那一份。
pub fn locate_sudachipy(project_root: &Path) -> Option<(PathBuf, PathBuf)> {
    locate_in_library(project_root).or_else(bundled_with_app)
}

/// 从用户选的目录里找出可以复制过来的那一份词典。
///
/// 选什么都认：0.1.x 的项目目录、它的 `venv`、`site-packages`、
/// `sudachidict_core` 本身，或者另一个语料库目录的 `sudachi/`。
/// 找不齐就返回 `None`——**不猜**，让界面照实说「这个目录里没有」。
///
/// 返回 `(资源目录, system.dic)`；资源目录里一定有 `sudachi.json`。
pub fn find_sudachipy_source(picked: &Path) -> Option<(PathBuf, PathBuf)> {
    // 先把「这个目录在哪一层」归一成若干个候选的 site-packages / 资源目录
    let roots = [
        picked.to_path_buf(),
        picked.join("venv/Lib/site-packages"),
        picked.join("Lib/site-packages"),
        picked.join("site-packages"),
    ];

    let mut resources: Option<PathBuf> = None;
    let mut dict: Option<PathBuf> = None;
    for root in &roots {
        if resources.is_none() {
            for candidate in [root.join("sudachipy/resources"), root.join("resources"), root.clone()] {
                if candidate.join("sudachi.json").is_file() {
                    resources = Some(candidate);
                    break;
                }
            }
        }
        if dict.is_none() {
            for candidate in [
                root.join("sudachidict_core/resources/system.dic"),
                root.join("resources/system.dic"),
                root.join("system.dic"),
            ] {
                if candidate.is_file() {
                    dict = Some(candidate);
                    break;
                }
            }
        }
    }
    Some((resources?, dict?))
}

#[cfg(test)]
mod locate_tests {
    use super::*;

    /// 每个用例一个空目录（跟工作区里其他测试一个写法，不为此多拉一个依赖）
    fn dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jp-sudachi-{}-{:?}-{tag}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 造一份「看起来像」的目录：文件内容无所谓，这里测的是找不找得到。
    fn fake(dir: &Path, resources: &str, dict: &str) {
        std::fs::create_dir_all(dir.join(resources)).unwrap();
        std::fs::write(dir.join(resources).join("sudachi.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.join(dict).parent().unwrap()).unwrap();
        std::fs::write(dir.join(dict), "dic").unwrap();
    }

    #[test]
    fn the_venv_shared_with_0_1_x_is_still_the_first_choice() {
        let tmp = dir("venv");
        fake(
            &tmp,
            "venv/Lib/site-packages/sudachipy/resources",
            "venv/Lib/site-packages/sudachidict_core/resources/system.dic",
        );
        let (resources, dict) = locate_in_library(&tmp).unwrap();
        assert!(resources.ends_with("sudachipy/resources"));
        assert!(dict.ends_with("system.dic"));
    }

    #[test]
    fn a_library_without_a_venv_uses_the_copy_sitting_in_sudachi() {
        let tmp = dir("bundled");
        fake(&tmp, BUNDLED_DIR, "sudachi/system.dic");
        let (resources, dict) = locate_in_library(&tmp).unwrap();
        assert_eq!(resources, tmp.join(BUNDLED_DIR));
        assert_eq!(dict, tmp.join(BUNDLED_DIR).join("system.dic"));
    }

    #[test]
    fn an_empty_library_has_none_and_says_so() {
        let tmp = dir("empty");
        // 库里没有。`locate_sudachipy` 这时会退到「程序自带」那一份，
        // 所以问的是 `locate_in_library`——这里测的是库里有没有。
        assert!(locate_in_library(&tmp).is_none());
        assert!(find_sudachipy_source(&tmp).is_none());
    }

    #[test]
    fn the_library_copy_wins_over_the_one_shipped_with_the_app() {
        let tmp = dir("prefers-library");
        fake(&tmp, BUNDLED_DIR, "sudachi/system.dic");
        // 自带的那一份在 exe 旁边，测试里没有；有也该让库里的赢
        let (resources, _) = locate_sudachipy(&tmp).unwrap();
        assert_eq!(resources, tmp.join(BUNDLED_DIR));
    }

    #[test]
    fn the_source_can_be_picked_at_any_of_the_usual_levels() {
        let tmp = dir("source");
        fake(
            &tmp,
            "venv/Lib/site-packages/sudachipy/resources",
            "venv/Lib/site-packages/sudachidict_core/resources/system.dic",
        );
        // 选项目目录、选 venv、选 site-packages，三种都认
        for picked in [
            tmp.clone(),
            tmp.join("venv"),
            tmp.join("venv/Lib/site-packages"),
        ] {
            let (resources, dict) = find_sudachipy_source(&picked)
                .unwrap_or_else(|| panic!("{} 没找到", picked.display()));
            assert!(resources.join("sudachi.json").is_file());
            assert!(dict.is_file());
        }
    }

    #[test]
    fn another_librarys_sudachi_folder_is_a_valid_source() {
        let tmp = dir("other-library");
        fake(&tmp, BUNDLED_DIR, "sudachi/system.dic");
        let (resources, dict) = find_sudachipy_source(&tmp.join(BUNDLED_DIR)).unwrap();
        assert_eq!(resources, tmp.join(BUNDLED_DIR));
        assert_eq!(dict, tmp.join(BUNDLED_DIR).join("system.dic"));
    }
}
