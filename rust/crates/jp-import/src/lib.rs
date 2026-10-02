//! 曲库扫描与导入。
//!
//! 分层：
//!
//! | 模块 | 职责 | 能否离线测试 |
//! |---|---|---|
//! | `filename` | 文件名 / 目录结构 → artist·title·音轨号 | ✅ |
//! | `lrc` | LRC 解析（歌词行 + 作词作曲信用） | ✅ |
//! | `scan` | 遍历目录、读 tag → ScannedTrack | ✅（构造数据） |
//! | `plan` | 和现有曲库比对 → 每个文件打算怎么处置 | ✅ |
//! | `import` | 执行计划，写库 | ✅（内存库） |
//!
//! 前三层**只读取和推断，不写库**，所以能在没有数据库的情况下完整测试；
//! `import` 是唯一写库的一层，一首歌一个事务。

pub mod filename;
pub mod import;
pub mod lrc;
pub mod maintain;
pub mod migrate;
pub mod metadata_csv;
pub mod plan;
pub mod scan;

pub use lrc::{ParsedLrc, parse_file as parse_lrc_file};
pub use import::{
    ImportReport, ImportedTrack, Outcome, Stats, add_credit, attach_lyrics, execute,
    execute_with_progress, get_or_create_person,
};
pub use plan::{Action, ImportPlan, LibraryIndex, PlanSummary, PlannedTrack, plan};
pub use scan::{ScannedTrack, is_audio_file, scan_dir, scan_file};
