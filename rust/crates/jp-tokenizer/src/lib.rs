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

/// 在项目里定位 SudachiPy 的资源目录和词典。
///
/// 迁移期两边共用同一份词典，省掉「Rust 侧再下一份 207MB 词典」的麻烦，
/// 也顺带保证了比对的是同一个数据源。
pub fn locate_sudachipy(project_root: &Path) -> Option<(PathBuf, PathBuf)> {
    let site = project_root.join("venv/Lib/site-packages");
    let resources = site.join("sudachipy/resources");
    let dict = site.join("sudachidict_core/resources/system.dic");
    (resources.is_dir() && dict.is_file()).then_some((resources, dict))
}
