//! 更新业务共用 jp-update。
pub use jp_update::*;
pub fn preferences() -> PreferenceView { jp_update::preferences(env!("CARGO_PKG_VERSION")) }
