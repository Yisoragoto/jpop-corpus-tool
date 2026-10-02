//! 数据迁移的应用层：库之外的那些文件。
//!
//! 库里的行由 [`jp_import::migrate`] 搬（那一层不依赖 Tauri，能离线测）。
//! 这里管三件它管不了的事：
//!
//! 1. **封面、歌手照片、.lrc** 的文件名里带 song_id，id 变了文件也要跟着改名复制，
//!    然后把新路径写回库里；
//! 2. **`dictionaries.db`**（Tauri 版的词典库）是另一个文件，整份复制；
//! 3. 进度。整库 209 首、两百多万词条，几十秒起步，不能让界面干等。
//!
//! **音频不复制。** `songs.audio_path` 是绝对路径，照旧指向原处——搬几十 GB
//! 不是这个功能该做的事，源目录还在路径就还通。界面上会把这一条写明。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::params;
use serde::Serialize;

/// 一次迁移的结果：库里的数字 + 文件的数字。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateOutcome {
    #[serde(flatten)]
    pub db: jp_import::migrate::MigrateReport,
    pub covers_copied: usize,
    pub lyrics_copied: usize,
    pub artist_photos_copied: usize,
    /// 词典库复制了没有（目标已经有就不动）
    pub dictionaries_copied: bool,
    /// 词典库多大（字节），没复制时是 0
    pub dictionaries_bytes: u64,
    /// 分词词典（Sudachi）搬过来了没有。**振假名和导入分词靠它**，
    /// 而新装的程序那边没有 0.1.x 的 venv，所以这一份必须跟着走。
    pub sudachi_copied: bool,
    /// 分词词典多大（字节），没复制时是 0
    pub sudachi_bytes: u64,
    pub warnings: Vec<String>,
}

/// 源目录看起来像不像一个语料库。
pub fn source_db(root: &Path) -> PathBuf {
    root.join("corpus.db")
}

/// 预览。只读。
pub fn plan(target: &rusqlite::Connection, source_root: &Path) -> Result<jp_import::migrate::MigratePlan> {
    jp_import::migrate::plan(target, &source_db(source_root))
}

/// 搬。库先搬，再按 id 映射复制文件。
///
/// 顺序是刻意的：**库是一个事务**，中途失败什么都没变；文件复制失败只会少几张图，
/// 下次重跑会补上（重跑时歌已经在库里了，走的是「已存在」那条路，不会搬两份）。
pub fn run(
    target: &mut rusqlite::Connection,
    source_root: &Path,
    target_root: &Path,
    options: jp_import::migrate::MigrateOptions,
    mut on_step: impl FnMut(&str),
) -> Result<MigrateOutcome> {
    on_step("正在搬曲目与语料…");
    let db = jp_import::migrate::run(target, &source_db(source_root), options)?;
    let mut outcome = MigrateOutcome {
        db,
        ..Default::default()
    };

    on_step("正在复制封面和歌词文件…");
    copy_per_song_files(target, source_root, target_root, &mut outcome)?;

    on_step("正在复制歌手照片…");
    copy_artist_photos(target, source_root, target_root, &mut outcome)?;

    if options.dictionaries {
        on_step("正在复制词典库…");
        copy_dictionaries(source_root, target_root, &mut outcome)?;
    }

    // 分词词典 207MB，不走 options：没有它振假名直接不可用，而用户看不出是缺这个。
    // 当前库已经有了就不动（`copy_from_library` 自己判断）。
    on_step("正在复制分词词典…");
    match crate::tokenizer::copy_from_library(source_root, target_root) {
        Ok(Some(installed)) => {
            outcome.sudachi_copied = true;
            outcome.sudachi_bytes = installed.bytes;
        }
        Ok(None) => {}
        // 词典搬不过来不该让整件事失败：歌和语料已经进去了，这里只记一笔
        Err(err) => note(&mut outcome, format!("分词词典没搬过来：{err}")),
    }
    Ok(outcome)
}

/// 封面和 .lrc：文件名就是 song_id，跟着新 id 改名复制，再把新路径写回库。
fn copy_per_song_files(
    target: &rusqlite::Connection,
    source_root: &Path,
    target_root: &Path,
    outcome: &mut MigrateOutcome,
) -> Result<()> {
    let src_covers = source_root.join("raw").join("covers");
    let src_lyrics = source_root.join("raw").join("lyrics_lrc");
    let dst_covers = target_root.join("raw").join("covers");
    let dst_lyrics = target_root.join("raw").join("lyrics_lrc");

    // 先把映射克隆出来：下面要往 outcome 里记警告，不能同时借着它迭代
    let id_map = outcome.db.id_map.clone();
    for (old_id, new_id) in &id_map {
        // 封面在 raw/covers/<歌手>/<id>.jpg，歌手目录名不一定和新库一致，所以按文件名找
        if let Some(found) = find_by_stem(&src_covers, old_id) {
            let artist_dir = found
                .parent()
                .and_then(|p| p.file_name())
                .map(PathBuf::from)
                .unwrap_or_default();
            let ext = found.extension().unwrap_or_default().to_string_lossy().to_string();
            let dest_dir = dst_covers.join(artist_dir);
            let dest = dest_dir.join(format!("{new_id}.{ext}"));
            match copy_file(&found, &dest) {
                Ok(()) => {
                    target.execute(
                        "UPDATE songs SET cover_path=?2 WHERE id=?1",
                        params![new_id, dest.display().to_string()],
                    )?;
                    outcome.covers_copied += 1;
                }
                Err(err) => note(outcome, format!("封面复制失败 {old_id}：{err}")),
            }
        }
        let lrc = src_lyrics.join(format!("{old_id}.lrc"));
        if lrc.is_file() {
            let dest = dst_lyrics.join(format!("{new_id}.lrc"));
            match copy_file(&lrc, &dest) {
                Ok(()) => {
                    target.execute(
                        "UPDATE songs SET source_file=?2 WHERE id=?1",
                        params![new_id, dest.display().to_string()],
                    )?;
                    outcome.lyrics_copied += 1;
                }
                Err(err) => note(outcome, format!("歌词复制失败 {old_id}：{err}")),
            }
        }
    }
    Ok(())
}

/// 歌手照片：`artists.image_path` 指向源库里的文件，复制过来再写回。
fn copy_artist_photos(
    target: &rusqlite::Connection,
    source_root: &Path,
    target_root: &Path,
    outcome: &mut MigrateOutcome,
) -> Result<()> {
    let rows: Vec<(String, String)> = {
        let mut stmt = match target.prepare(
            "SELECT name, image_path FROM artists WHERE image_path IS NOT NULL AND image_path <> ''",
        ) {
            Ok(stmt) => stmt,
            // 老库没有 artists 表时直接跳过
            Err(_) => return Ok(()),
        };
        let mapped = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        mapped.collect::<Result<_, _>>()?
    };
    // 迁移时 image_path 写的是空串，所以这里要从源库那边找：按歌手名在 raw/artists 下找同名文件
    let src_dir = source_root.join("raw").join("artists");
    let dst_dir = target_root.join("raw").join("artists");
    let _ = rows; // 目标库里的那几行 image_path 为空，真正的来源在下面这一遍

    let Ok(entries) = std::fs::read_dir(&src_dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name() else { continue };
        let dest = dst_dir.join(name);
        match copy_file(&path, &dest) {
            Ok(()) => {
                outcome.artist_photos_copied += 1;
                // 文件名就是歌手名（刮削那边这么存的），按它写回
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let _ = target.execute(
                        "UPDATE artists SET image_path=?2 WHERE name=?1 AND (image_path IS NULL OR image_path='')",
                        params![stem, dest.display().to_string()],
                    );
                }
            }
            Err(err) => note(outcome, format!("歌手照片复制失败 {}：{err}", path.display())),
        }
    }
    Ok(())
}

/// Tauri 版的词典库是单独一个文件。目标已经有就不动——两份词典库合并是另一件事。
fn copy_dictionaries(
    source_root: &Path,
    target_root: &Path,
    outcome: &mut MigrateOutcome,
) -> Result<()> {
    let src = source_root.join("dictionaries.db");
    let dst = target_root.join("dictionaries.db");
    if !src.is_file() {
        return Ok(());
    }
    if dst.exists() {
        note(
            outcome,
            "当前库已经有 dictionaries.db，没有覆盖；要用旧词典请先在词典页删掉现有的，或者手动替换这个文件".into(),
        );
        return Ok(());
    }
    std::fs::copy(&src, &dst).with_context(|| format!("复制词典库失败：{}", src.display()))?;
    outcome.dictionaries_copied = true;
    outcome.dictionaries_bytes = std::fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
    Ok(())
}

/// 在 `dir`（含一层子目录）里找主文件名是 `stem` 的文件
fn find_by_stem(dir: &Path, stem: &str) -> Option<PathBuf> {
    let direct = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry in direct.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
        } else if path.file_stem().and_then(|s| s.to_str()) == Some(stem) {
            return Some(path);
        }
    }
    for sub in subdirs {
        if let Ok(entries) = std::fs::read_dir(&sub) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.file_stem().and_then(|s| s.to_str()) == Some(stem) {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn copy_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(from, to)?;
    Ok(())
}

/// 单个文件出问题不该中断整次迁移，但也不能默不作声——前几条报出来
fn note(outcome: &mut MigrateOutcome, message: String) {
    if outcome.warnings.len() < 8 {
        outcome.warnings.push(message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_are_found_under_the_artist_folder_and_at_the_top() {
        let dir = std::env::temp_dir().join(format!("jp-migrate-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("ヨルシカ")).unwrap();
        std::fs::write(dir.join("ヨルシカ").join("001.jpg"), b"x").unwrap();
        std::fs::write(dir.join("007.png"), b"x").unwrap();

        assert_eq!(
            find_by_stem(&dir, "001").unwrap().file_name().unwrap(),
            "001.jpg"
        );
        assert_eq!(
            find_by_stem(&dir, "007").unwrap().file_name().unwrap(),
            "007.png"
        );
        assert!(find_by_stem(&dir, "999").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
