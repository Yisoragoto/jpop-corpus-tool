//! 音频、封面路径的解析。
//!
//! 库里存的是**导入那台机器上的绝对路径**（`E:\cloud music\...\x.flac`）。
//! 同一个 `corpus.db` 拿到另一台机器、另一个系统（比如 macOS 客户端）上，这些路径全部失效。
//! 这里按顺序找：
//!
//! 1. 原路径还在，就用它；
//! 2. 相对路径按语料库目录解析；
//! 3. 按路径的最后几段在语料库目录和音频根目录里找；
//! 4. 音频再按**文件名**找（递归，限深度）。图片不这样找：封面文件名是编号，会撞名。
//!
//! Windows 风格的路径在 macOS 上用 `Path` 拆不出文件名（`\` 不是分隔符），所以文件名自己拆。

use std::path::{Path, PathBuf};

/// 递归找文件时最多下几层。`raw/audio/{歌手}/{id}.flac` 是两层，网易云下载目录一般一两层
const MAX_DEPTH: usize = 4;

/// 不管是 `\` 还是 `/` 分隔的路径，取最后一段
pub fn file_name_of(stored: &str) -> &str {
    stored.rsplit(['\\', '/']).next().unwrap_or(stored)
}

pub fn resolve(stored: &str, library_root: Option<&Path>, search_roots: &[PathBuf]) -> Option<PathBuf> {
    let stored = stored.trim();
    if stored.is_empty() {
        return None;
    }
    let direct = PathBuf::from(stored);
    if direct.is_file() {
        return Some(direct);
    }
    if let Some(root) = library_root
        && direct.is_relative()
    {
        let joined = root.join(stored.replace('\\', "/"));
        if joined.is_file() {
            return Some(joined);
        }
    }
    let name = file_name_of(stored);
    if name.is_empty() {
        return None;
    }
    let roots: Vec<PathBuf> = library_root.map(Path::to_path_buf).into_iter().chain(search_roots.iter().cloned()).collect();
    // 先按路径的尾巴找：封面是 `raw/covers/{歌手}/001.jpg`，每个歌手目录里都有一张 001.jpg，
    // 只按文件名找会拿到别人的封面。从长到短试最后 4…2 段，都对不上才退回只看文件名
    let parts: Vec<&str> = stored.split(['\\', '/']).filter(|p| !p.is_empty()).collect();
    for keep in (2..=4.min(parts.len())).rev() {
        let tail: PathBuf = parts[parts.len() - keep..].iter().collect();
        for root in &roots {
            let candidate = root.join(&tail);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    // 图片不按文件名找：封面、歌手照片的文件名是 `001.jpg` 这种编号，每个歌手目录里都有一张，
    // 只看文件名会拿到别人的图（在 Mac 上打开从 Windows 拷来的库时，专辑页和曲目显示成别的歌的封面）。
    // 尾巴对不上就照实说找不到，让「在线补齐封面」重新下
    if is_image(name) {
        return None;
    }
    for root in roots {
        if let Some(found) = find_by_name(&root, name, MAX_DEPTH) {
            return Some(found);
        }
    }
    None
}

fn is_image(name: &str) -> bool {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "tif" | "tiff" | "heic" | "avif")
}

fn find_by_name(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    let candidate = dir.join(name);
    if candidate.is_file() {
        return Some(candidate);
    }
    if depth == 0 {
        return None;
    }
    let mut subdirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();
    subdirs.sort();
    subdirs.into_iter().find_map(|sub| find_by_name(&sub, name, depth - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_split_on_both_separators() {
        assert_eq!(file_name_of(r"E:\cloud music\a\b - c.flac"), "b - c.flac");
        assert_eq!(file_name_of("/Users/x/a.flac"), "a.flac");
        assert_eq!(file_name_of("a.flac"), "a.flac");
    }

    #[test]
    fn a_moved_file_is_found_under_a_search_root() {
        let dir = std::env::temp_dir().join(format!("jp-media-{}", std::process::id()));
        let nested = dir.join("歌手").join("专辑");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("001.flac"), b"x").unwrap();
        let found = resolve(r"D:\old\place\001.flac", None, std::slice::from_ref(&dir));
        assert_eq!(found, Some(nested.join("001.flac")));
        assert_eq!(resolve(r"D:\old\missing.flac", None, std::slice::from_ref(&dir)), None);
        assert_eq!(resolve("", None, std::slice::from_ref(&dir)), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn same_named_covers_are_told_apart_by_their_folders() {
        let root = std::env::temp_dir().join(format!("jp-media-covers-{}", std::process::id()));
        for artist in ["Aimer", "Vaundy"] {
            let d = root.join("raw").join("covers").join(artist);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("001.jpg"), artist.as_bytes()).unwrap();
        }
        // Windows 那台机器上的绝对路径，拷到别处之后按目录尾巴对上
        let found = resolve(r"C:\Users\x\AppData\Local\JPOP Corpus Tool\raw\covers\Vaundy\001.jpg", Some(&root), &[]);
        assert_eq!(found, Some(root.join("raw").join("covers").join("Vaundy").join("001.jpg")));
        // 这个歌手的目录不在：不能退回按文件名拿到 Aimer 的 001.jpg
        let missing = resolve(r"C:\Users\x\AppData\Local\JPOP Corpus Tool\raw\covers\サカナクション\001.jpg", Some(&root), &[]);
        assert_eq!(missing, None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
