//! 一键制卡：逻辑在 `jp_anki::lyric_mine`（macOS 客户端经 `jp-ffi` 用同一份），这里只接上应用状态。

use anyhow::{Context, Result};
use jp_anki::mine::MineOutcome;
use jp_dict::store::DictionaryStore;

pub use jp_anki::lyric_mine::{MineCheck, MineRequest, align_token_offset, align_tokens, check};

use crate::state::AppState;

pub fn mine(state: &AppState, request: &MineRequest) -> Result<MineOutcome> {
    // 单开一条连接：切音频、传文件要一两秒，不占着界面查词用的那条
    let store = DictionaryStore::open(state.dictionaries_path()).context("打不开词典库")?;
    // 语料库只在取例句时拿一下锁
    let prepared = {
        let corpus = state.corpus();
        jp_anki::lyric_mine::prepare(&corpus, &store, state.translator(), request)?
    };
    jp_anki::lyric_mine::send(&crate::anki::client(), &store, &prepared, request)
}
