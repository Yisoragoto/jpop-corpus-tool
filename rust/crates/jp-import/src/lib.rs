//! 曲库扫描与导入。
//!
//! 分层：
//!
//! | 模块 | 职责 | 能否离线测试 |
//! |---|---|---|
//! | `filename` | 文件名 / 目录结构 → artist·title·音轨号 | ✅ |
//! | `lrc` | LRC 解析（歌词行 + 作词作曲信用） | ✅ |
//! | `scan` | 遍历目录、读 tag → ScannedTrack | ✅（构造数据） |
//! | `embedded` | 音频文件里内嵌的歌词、封面 | ✅（夹具写 tag） |
//! | `plan` | 和现有曲库比对 → 每个文件打算怎么处置 | ✅ |
//! | `watch` | 库里的歌都在哪些文件夹 → 那里还有哪些文件没导入 | ✅（临时目录） |
//! | `review` | 扫一批路径 + 计划 → 给人复核的那份（两个客户端共用） | ✅ |
//! | `import` | 执行计划，写库 | ✅（内存库） |
//!
//! 前三层**只读取和推断，不写库**，所以能在没有数据库的情况下完整测试；
//! `import` 是唯一写库的一层，一首歌一个事务。

pub mod annotate;
pub mod embedded;
pub mod filename;
pub mod import;
pub mod lrc;
pub mod maintain;
pub mod migrate;
pub mod metadata_csv;
pub mod plan;
pub mod review;
pub mod scan;
pub mod srt;
pub mod watch;

pub use lrc::{ParsedLrc, parse_file as parse_lrc_file};
pub use import::{
    ImportReport, ImportedTrack, LyricSource, Outcome, Stats, add_credit, attach_lines, attach_lyrics, attach_lyrics_text, execute,
    execute_cancellable, execute_with_progress, get_or_create_person,
};
pub use review::{PlanItemView, ScanResult, review, scan_targets};
pub use plan::{Action, ImportPlan, LibraryIndex, PlanSummary, PlannedTrack, plan};
pub use embedded::{EmbeddedCover, EmbeddedLyrics};
pub use scan::{ArtistSource, LyricsPick, ScannedTrack, is_audio_file, list_audio, scan_dir, scan_file, scan_file_path};
pub use watch::{UnknownAudio, WatchReason, WatchedFolder, unknown_audio, watched_folders};
