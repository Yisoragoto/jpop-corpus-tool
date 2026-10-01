//! 语料库在哪。
//!
//! 装好的程序第一次启动时，机器上还没有任何库——这时在
//! `%LOCALAPPDATA%\JPOP Corpus Tool` 建一个空的。但很多人**已经有一个库**
//! （比如从 0.1.x 用过来的 `D:\jp_corpus`），不该逼他们去配环境变量：
//! 设置页里选一个目录，记在这个模块管的 `settings.json` 里，下次启动就用它。
//!
//! 查找顺序（`locate`）：
//!
//! 1. `JPOP_CORPUS_HOME` 环境变量——临时指定、跑测试用，优先级最高
//! 2. `settings.json` 里记下的目录——用户在设置页选的
//! 3. 从可执行文件往上找（开发时 exe 在 `rust/target/release/`，库在仓库根）
//! 4. 当前工作目录
//! 5. 默认数据目录，没有就在那里新建一个空库
//!
//! 2、3、4 都要求那里**真的是一个库**（见 [`looks_like_library`]）：
//! SQLite 打开不存在的路径时会凭空生成一个 0 表的空文件，
//! 装在 `E:\JPOP Corpus Tool\` 的 0.2.0 就是被自己留下的那个空文件绊住的——
//! 每次启动都认准它，然后因为缺表直接退出，双击桌面图标毫无反应。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 库是从哪儿找到的。设置页要如实告诉用户现在用的是哪一个
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RootSource {
    /// `JPOP_CORPUS_HOME` 环境变量
    Env,
    /// 设置页里选的
    Settings,
    /// 可执行文件旁边（开发时就是仓库根）
    NextToExe,
    /// 当前工作目录
    WorkingDir,
    /// 默认数据目录（新装的程序就是这种）
    Default,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Settings {
    /// 用户选的语料库目录。没选过就是 None
    #[serde(skip_serializing_if = "Option::is_none")]
    library_root: Option<String>,
}

/// 默认数据目录：`%LOCALAPPDATA%\JPOP Corpus Tool`。
///
/// **不能用安装目录**：卸载会把安装目录整个删掉，用户的歌、歌词和语料会跟着没。
pub fn default_data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("JPOP Corpus Tool")
}

fn settings_path() -> PathBuf {
    default_data_dir().join("settings.json")
}

fn read_settings() -> Settings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// 这个目录里是不是一个**能用的**语料库。
///
/// 光看 `corpus.db` 在不在不够：那个文件可能是 SQLite 自己生成的 0 表空壳。
/// 所以真的去看一眼有没有 `songs` 表。
pub fn looks_like_library(dir: &Path) -> bool {
    let db = dir.join("corpus.db");
    if !db.is_file() {
        return false;
    }
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return false;
    };
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='songs'",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

/// 找到语料库所在目录，并说清楚是怎么找到的。
pub fn locate() -> (PathBuf, RootSource) {
    if let Some(home) = std::env::var_os("JPOP_CORPUS_HOME") {
        let path = PathBuf::from(home);
        if looks_like_library(&path) {
            return (path, RootSource::Env);
        }
    }
    if let Some(chosen) = read_settings().library_root {
        let path = PathBuf::from(chosen);
        // 选过的目录即使还空着也认：用户就是要在那里建库。
        // 但目录本身得在——盘拔了就退回默认，而不是开不起来
        if path.is_dir() {
            return (path, RootSource::Settings);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        for dir in exe.ancestors().skip(1).take(6) {
            if looks_like_library(dir) {
                return (dir.to_path_buf(), RootSource::NextToExe);
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir()
        && looks_like_library(&cwd)
    {
        return (cwd, RootSource::WorkingDir);
    }
    let fallback = default_data_dir();
    // 建不出来也不要在这里倒下：后面打开库时会报一个说得清楚的错
    let _ = std::fs::create_dir_all(&fallback);
    (fallback, RootSource::Default)
}

/// 记下用户选的目录。传 `None` 清掉，回到自动查找。
///
/// 只写设置，不动当前这次运行——换库要重启（数据库连接、asset 放行范围
/// 都是启动时定下的，半路换等于整个 AppState 重建）。
pub fn remember(root: Option<&Path>) -> anyhow::Result<()> {
    let dir = default_data_dir();
    std::fs::create_dir_all(&dir)?;
    let settings = Settings {
        library_root: root.map(|p| p.display().to_string()),
    };
    let text = serde_json::to_string_pretty(&settings)?;
    std::fs::write(settings_path(), text)?;
    Ok(())
}

/// 用户选的那个目录现在记的是什么（没选过是 None）
pub fn remembered() -> Option<PathBuf> {
    read_settings().library_root.map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 装好之后第一次启动，0.2.0 在安装目录留下过一个 0 表的空库；
    /// 那个文件不能被当成「这里有一个语料库」，否则每次启动都认准它然后缺表退出
    #[test]
    fn an_empty_database_file_is_not_a_library() {
        let dir = std::env::temp_dir().join(format!("jp-empty-lib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("corpus.db");
        rusqlite::Connection::open(&db).unwrap(); // 和 SQLite 自己生成的一样：0 张表
        assert!(db.is_file());
        assert!(!looks_like_library(&dir), "空文件不该算语料库");

        let corpus = jp_corpus::Corpus::open_writable(&db).unwrap();
        corpus.ensure_schema().unwrap();
        drop(corpus);
        assert!(looks_like_library(&dir), "建完表就该算了");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_default_data_dir_is_not_the_install_dir() {
        // 卸载会把安装目录删掉，用户的库不能放在那里
        let dir = default_data_dir();
        assert!(dir.ends_with("JPOP Corpus Tool"), "{dir:?}");
        assert!(dir.is_absolute(), "{dir:?}");
    }

    #[test]
    fn a_missing_directory_is_not_a_library() {
        assert!(!looks_like_library(Path::new("Z:/这个目录不存在")));
    }
}
