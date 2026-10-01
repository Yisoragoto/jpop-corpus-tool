//! 查词引擎：Yomitan 式活用还原、文本变体、词典导入与检索。
//!
//! 算法和规则数据移植自 [Yomitan](https://github.com/yomidevs/yomitan)
//! （GPL-3.0-or-later），存储思路参考 [hoshidicts](https://github.com/Manhhao/hoshidicts)
//! （GPL-3.0-or-later）。每个模块头部注明了对应的源文件。
//!
//! 移植原则：能拿 Yomitan 源码跑出基准的地方都拿来逐项对账，
//! 规则表从源码导出而不是手抄。

pub mod anki;
pub mod deinflect;
pub mod furigana;
pub mod import;
pub mod occurrence;
pub mod media;
pub mod store;
pub mod text;
pub mod transformer;
pub mod translator;
pub mod variants;
pub mod zip;

pub use transformer::{LanguageTransformer, TraceFrame, TransformedText, conditions_match};
