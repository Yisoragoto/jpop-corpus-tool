//! 对着真实 `corpus.db` 跑的集成测试。
//!
//! 用真库而不是造数据，是因为这一层的价值全在「SQL 对不对」上——
//! 内存里造几行假数据能通过的查询，在 209 首歌 / 58,628 个 token 上
//! 未必对（GROUP BY 漏字段、JOIN 放大、索引没走上，都只在真数据上暴露）。
//!
//! 库不存在或还没迁移时整体跳过，并说清楚原因。跳过也算通过，所以**同一份断言**
//! 还在 `fixture_corpus.rs` 里对着合成库跑一遍，那一份哪台机器都跑——断言本身在 `invariants/`。
//!
//! **这里只读不写。** 以前有一条播放历史的测试会往真库里插一条再删掉，中途失败就留在用户的库里了；
//! 它现在只在合成库上跑。

use std::path::{Path, PathBuf};

#[macro_use]
mod invariants;

use jp_corpus::Corpus;

fn db_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("manifest 路径异常")
        .join("corpus.db")
}

/// 每个测试各开一个连接。
///
/// 不做成全局缓存是因为 `rusqlite::Connection` 不是 `Sync`——它内部有
/// 语句缓存的 `RefCell`。这正是 SQLite 的线程模型：连接不跨线程共享。
/// 打开本身很便宜，真正的开销在页缓存上。
fn open_corpus() -> Option<Corpus> {
    let path = db_path();
    if !path.exists() {
        eprintln!("[skip] 找不到 {}", path.display());
        return None;
    }
    let corpus = match Corpus::open(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[skip] {e}");
            return None;
        }
    };
    if let Err(e) = corpus.check_schema() {
        eprintln!("[skip] {e}
  这组测试要作者本机那个真实的 corpus.db，克隆仓库的人没有它");
        return None;
    }
    Some(corpus)
}

// 真库不是为这些断言造的，前提不成立时可以跳过（`false`）
invariant_tests!(open_corpus().map(|corpus| (corpus, false)));
