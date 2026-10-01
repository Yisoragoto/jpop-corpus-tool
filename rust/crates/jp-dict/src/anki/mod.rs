//! 制卡：把查词结果渲染成 Yomitan 格式的卡片字段（Lapis 等笔记类型直接用这些字段）。
//!
//! 从 Yomitan 移植，见各子模块头部。和 Yomitan 自带的制卡期望结果逐字对账。

mod css;
mod dom;
pub mod note;
pub mod structured;

pub use note::{EntryKind, NoteContext, NoteRenderer};
pub use structured::{MediaResolver, escape};
