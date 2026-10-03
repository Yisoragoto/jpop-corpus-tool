//! Anki 卡片导出与学习状态同步。
//!
//! 架构图上那个 Anki 盒子。两个方向：
//!
//! | 方向 | 途径 | 模块 |
//! |---|---|---|
//! | 语料 → Anki | AnkiConnect HTTP（`127.0.0.1:8765`） | `connect` `model` `card` `export` |
//! | Anki → 语料 | 直接读 `collection.anki2` | `learning` |
//!
//! **除了 AnkiConnect 本身，全程不联网。** 释义、音高、词频、JLPT
//! 都在 `corpus.db` 里现成的表里（249 万条词典条目），例句来自语料本身。
//! 这正是这个项目的价值所在：卡片上的例句是用户自己听的歌。

pub mod audio;
pub mod card;
pub mod connect;
pub mod dict;
pub mod export;
pub mod learning;
pub mod lyrics_model;
pub mod mine;
pub mod mining_report;
pub mod mp3;
pub mod model;

pub use connect::{AnkiConnect, AnkiError};
pub use card::{Card, CardOptions, Example, WordFields, word_fields};
pub use dict::{Definition, WordInfo};
pub use export::{
    DupMode, DupScope, ExportOptions, ExportOutcome, PickOptions, RefreshOutcome,
    RefreshScope, RefreshTarget, WordCandidate, export_word, missing_deck_message, pick_words,
    refresh_target, refresh_targets, update_word,
};
pub use learning::{LearningState, WordStatus};
pub use model::{FIELDS, NOTE_TYPE};
