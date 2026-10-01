//! 歌词字体：列出本机装的字体，以及项目自带 / 用户导入的字体文件。
//!
//! 对应 Python 版曲库「显示」对话框的主字体、备用字体和「导入字体」（`_font_options`、`_import_font`）。
//!
//! **本机字体问 DirectWrite**（`installed_fonts`）。WebView2 按名字找字体走的就是 DirectWrite 的系统字体集，
//! 它给的族名 CSS 认。除了字体族名（「Yu Gothic UI」），每个字体的 GDI 兼容族名（「Yu Gothic UI Semibold」）也列上，
//! 和 Python 版 Qt 的列表一样：CSS 也认这种名字，挑它就等于挑了字重。先试过自己读字体文件的 name 表（nameID 16 / 1）：在 Edge 里实测，
//! 那样列出的 273 个名字只有 233 个认，而且漏掉 89 个认的（可变字体的具名实例如「Segoe UI Variable Display」、
//! DirectWrite 归并出来的「Eras ITC」、中文名「微软雅黑」）。
//!
//! **字体文件读 name 表**（`font_families`）：自带和导入的字体没装进系统，界面用 FontFace 按文件加载，
//! 只需要一个名字。优先 en-US，nameID 16 和 1 都列；只读表目录和 name 表，不整份读进内存（中日文字体一个二三十 MB）。
//! 这些文件界面要通过 asset 协议加载，由命令在运行时逐个放行，不放行整个目录。

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;

/// 一个字体文件里所有字体的族名（TTC 里有多个），去重、保持顺序
pub fn font_families(path: &Path) -> Result<Vec<String>> {
    let mut file =
        File::open(path).with_context(|| format!("打不开字体文件 {}", path.display()))?;
    let mut head = [0u8; 12];
    file.read_exact(&mut head).context("字体文件太短")?;
    let offsets: Vec<u32> = if &head[..4] == b"ttcf" {
        let count = be32(&head[8..12]).min(64);
        let raw = read_at(&mut file, 12, count as usize * 4)?;
        raw.as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_be_bytes(*c))
            .collect()
    } else if matches!(&head[..4], [0, 1, 0, 0] | b"OTTO" | b"true") {
        vec![0]
    } else {
        bail!("不是 TrueType / OpenType 字体：{}", path.display());
    };

    let mut families = Vec::new();
    for offset in offsets {
        for name in face_families(&mut file, u64::from(offset))? {
            if !families.contains(&name) {
                families.push(name);
            }
        }
    }
    Ok(families)
}

fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn read_at(file: &mut File, offset: u64, len: usize) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf).context("字体文件被截断")?;
    Ok(buf)
}

fn face_families(file: &mut File, offset: u64) -> Result<Vec<String>> {
    let dir = read_at(file, offset, 12)?;
    let tables = be16(&dir[4..6]) as usize;
    let records = read_at(file, offset + 12, tables * 16)?;
    let Some(name) = records
        .as_chunks::<16>()
        .0
        .iter()
        .find(|r| &r[..4] == b"name")
    else {
        return Ok(Vec::new());
    };
    let (table_offset, table_len) = (be32(&name[8..12]), be32(&name[12..16]));
    if table_len > 4 << 20 {
        bail!("name 表大得不正常");
    }
    let table = read_at(file, u64::from(table_offset), table_len as usize)?;
    Ok(families_from_name_table(&table))
}

/// 从 name 表里取族名：nameID 16 和 nameID 1 各挑一个（语言按偏好），相同的只留一个
fn families_from_name_table(table: &[u8]) -> Vec<String> {
    if table.len() < 6 {
        return Vec::new();
    }
    let count = be16(&table[2..4]) as usize;
    let storage = be16(&table[4..6]) as usize;
    // [nameID 16 的最佳, nameID 1 的最佳]
    let mut best: [Option<(u32, String)>; 2] = [None, None];
    for i in 0..count {
        let at = 6 + i * 12;
        let Some(record) = table.get(at..at + 12) else {
            break;
        };
        let (platform, encoding, language, name_id) = (
            be16(&record[0..2]),
            be16(&record[2..4]),
            be16(&record[4..6]),
            be16(&record[6..8]),
        );
        let (len, off) = (
            be16(&record[8..10]) as usize,
            be16(&record[10..12]) as usize,
        );
        if name_id != 16 && name_id != 1 {
            continue;
        }
        let Some(bytes) = table.get(storage + off..storage + off + len) else {
            continue;
        };
        let text = match (platform, encoding) {
            (3, 0 | 1 | 10) | (0, _) => {
                let units: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_be_bytes(*c))
                    .collect();
                String::from_utf16_lossy(&units)
            }
            (1, 0) => bytes
                .iter()
                .map(|&b| if b.is_ascii() { b as char } else { '\u{FFFD}' })
                .collect(),
            _ => continue,
        };
        let text = text.trim().to_owned();
        if text.is_empty() || text.contains('\u{FFFD}') {
            continue;
        }
        // 分数越小越好：Windows 平台 en-US 优先
        let score = match (platform, language) {
            (3, 0x0409) => 0,
            (3, _) => 1,
            (0, _) => 2,
            _ => 3,
        };
        let slot = &mut best[usize::from(name_id != 16)];
        if slot.as_ref().is_none_or(|(s, _)| score < *s) {
            *slot = Some((score, text));
        }
    }
    let mut names: Vec<String> = Vec::new();
    for (_, name) in best.into_iter().flatten() {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// 本机装的一个字体族
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledFont {
    /// CSS 里用的名字：英文名，没有英文名时取第一个
    pub name: String,
    /// 中文、日文里的名字（和 `name` 不同的），列表里一起显示，方便认
    pub aliases: Vec<String>,
}

/// 从 DirectWrite 给的（语言, 名字）里挑出主名和中日文别名
fn pick_names(localized: &[(String, String)]) -> Option<InstalledFont> {
    let name = localized
        .iter()
        .find(|(locale, _)| locale.eq_ignore_ascii_case("en-us"))
        .or_else(|| localized.first())?
        .1
        .clone();
    let mut aliases: Vec<String> = Vec::new();
    for (locale, alias) in localized {
        let locale = locale.to_ascii_lowercase();
        if (locale.starts_with("zh") || locale.starts_with("ja"))
            && *alias != name
            && !aliases.contains(alias)
        {
            aliases.push(alias.clone());
        }
    }
    Some(InstalledFont { name, aliases })
}

/// 本机装的字体，按名字排序
#[cfg(windows)]
pub fn installed_fonts() -> Result<Vec<InstalledFont>> {
    use windows::Win32::Graphics::DirectWrite::{
        DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_SIMULATIONS_NONE,
        DWRITE_INFORMATIONAL_STRING_WIN32_FAMILY_NAMES, DWriteCreateFactory, IDWriteFactory,
        IDWriteFontCollection, IDWriteLocalizedStrings,
    };

    // SAFETY: DirectWrite 的只读查询；缓冲区按它报的长度 +1（结尾的 0）分配
    unsafe fn localized(names: &IDWriteLocalizedStrings) -> Result<Vec<(String, String)>> {
        fn text(buffer: &[u16]) -> String {
            let end = buffer.iter().position(|&u| u == 0).unwrap_or(buffer.len());
            String::from_utf16_lossy(&buffer[..end])
        }
        let mut out = Vec::new();
        unsafe {
            for j in 0..names.GetCount() {
                let mut locale = vec![0u16; names.GetLocaleNameLength(j)? as usize + 1];
                names.GetLocaleName(j, &mut locale)?;
                let mut name = vec![0u16; names.GetStringLength(j)? as usize + 1];
                names.GetString(j, &mut name)?;
                out.push((text(&locale), text(&name)));
            }
        }
        Ok(out)
    }

    let mut fonts = Vec::new();
    // SAFETY: 同上，全是只读查询
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let mut collection: Option<IDWriteFontCollection> = None;
        factory.GetSystemFontCollection(&mut collection, false)?;
        let collection = collection.context("DirectWrite 没给出系统字体集")?;
        for i in 0..collection.GetFontFamilyCount() {
            let family = collection.GetFontFamily(i)?;
            if let Some(font) = pick_names(&localized(&family.GetFamilyNames()?)?) {
                fonts.push(font);
            }
            // 每个字重的 GDI 兼容族名；粗体、斜体模拟出来的不算
            for k in 0..family.GetFontCount() {
                let font = family.GetFont(k)?;
                if font.GetSimulations() != DWRITE_FONT_SIMULATIONS_NONE {
                    continue;
                }
                let mut strings: Option<IDWriteLocalizedStrings> = None;
                let mut exists = windows::core::BOOL::default();
                font.GetInformationalStrings(
                    DWRITE_INFORMATIONAL_STRING_WIN32_FAMILY_NAMES,
                    &mut strings,
                    &mut exists,
                )?;
                if let (true, Some(strings)) = (exists.as_bool(), strings)
                    && let Some(win32) = pick_names(&localized(&strings)?)
                {
                    fonts.push(win32);
                }
            }
        }
    }
    fonts.sort_by_key(|f| f.name.to_lowercase());
    // 同名的合并别名
    let mut merged: Vec<InstalledFont> = Vec::with_capacity(fonts.len());
    for font in fonts {
        match merged.last_mut() {
            Some(last) if last.name == font.name => {
                for alias in font.aliases {
                    if !last.aliases.contains(&alias) {
                        last.aliases.push(alias);
                    }
                }
            }
            _ => merged.push(font),
        }
    }
    Ok(merged)
}

/// 非 Windows：扫字体目录读 name 表（没有 DirectWrite）
#[cfg(not(windows))]
pub fn installed_fonts() -> Result<Vec<InstalledFont>> {
    Ok(scan_families(&system_font_dirs())
        .into_iter()
        .map(|name| InstalledFont {
            name,
            aliases: Vec::new(),
        })
        .collect())
}

/// 本机字体目录：系统的和当前用户装的
pub fn system_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(windir) = std::env::var_os("WINDIR") {
        dirs.push(PathBuf::from(windir).join("Fonts"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(
            PathBuf::from(local)
                .join("Microsoft")
                .join("Windows")
                .join("Fonts"),
        );
    }
    dirs
}

fn is_font_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc"))
}

/// 这些目录里所有字体的族名，排好序。读不了的文件跳过（系统目录里有 .fon 之类的点阵字体）
pub fn scan_families(dirs: &[PathBuf]) -> Vec<String> {
    let mut families = BTreeSet::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if is_font_file(&path)
                && let Ok(names) = font_families(&path)
            {
                families.extend(names);
            }
        }
    }
    let mut sorted: Vec<String> = families.into_iter().collect();
    sorted.sort_by_key(|name| name.to_lowercase());
    sorted
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FontFile {
    pub family: String,
    pub path: String,
    /// 项目自带（`assets/fonts`）还是用户导入的
    pub bundled: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FontCatalog {
    /// 本机装好的字体
    pub installed: Vec<InstalledFont>,
    /// 要按文件加载的字体
    pub files: Vec<FontFile>,
    /// 导入过、现在读不到的文件和原因
    pub problems: Vec<String>,
}

/// 项目自带的字体文件（`assets/fonts`）+ 用户导入的文件
pub fn font_files(bundled_dir: &Path, imported: &[String]) -> (Vec<FontFile>, Vec<String>) {
    let mut files = Vec::new();
    let mut problems = Vec::new();
    let mut bundled: Vec<PathBuf> = std::fs::read_dir(bundled_dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| is_font_file(p))
                .collect()
        })
        .unwrap_or_default();
    bundled.sort();
    let candidates = bundled
        .into_iter()
        .map(|p| (p, true))
        .chain(imported.iter().map(|p| (PathBuf::from(p), false)));
    for (path, is_bundled) in candidates {
        match font_families(&path) {
            Ok(names) if !names.is_empty() => {
                for family in names {
                    if !files.iter().any(|f: &FontFile| f.family == family) {
                        files.push(FontFile {
                            family,
                            path: path.display().to_string(),
                            bundled: is_bundled,
                        });
                    }
                }
            }
            Ok(_) => problems.push(format!("{}：读不出字体名", path.display())),
            Err(err) => problems.push(format!("{err:#}")),
        }
    }
    (files, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 拼一个最小的 name 表
    fn name_table(records: &[(u16, u16, u16, u16, &[u8])]) -> Vec<u8> {
        let mut head = Vec::new();
        let mut storage = Vec::new();
        head.extend_from_slice(&0u16.to_be_bytes());
        head.extend_from_slice(&(records.len() as u16).to_be_bytes());
        head.extend_from_slice(&((6 + records.len() * 12) as u16).to_be_bytes());
        for (platform, encoding, language, name_id, bytes) in records {
            for v in [
                *platform,
                *encoding,
                *language,
                *name_id,
                bytes.len() as u16,
                storage.len() as u16,
            ] {
                head.extend_from_slice(&v.to_be_bytes());
            }
            storage.extend_from_slice(bytes);
        }
        head.extend_from_slice(&storage);
        head
    }

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
    }

    #[test]
    fn both_typographic_and_legacy_names_are_listed_in_english_first() {
        let ja = utf16("游ゴシック");
        let legacy = utf16("Yu Gothic UI Semilight");
        let typographic = utf16("Yu Gothic UI");
        let table = name_table(&[
            (3, 1, 0x0411, 16, &ja),
            (3, 1, 0x0409, 1, &legacy),
            (3, 1, 0x0409, 16, &typographic),
        ]);
        assert_eq!(
            families_from_name_table(&table),
            ["Yu Gothic UI", "Yu Gothic UI Semilight"]
        );
    }

    #[test]
    fn same_names_are_listed_once_and_localized_names_are_used_when_there_is_no_english() {
        let meiryo = utf16("Meiryo UI");
        assert_eq!(
            families_from_name_table(&name_table(&[
                (3, 1, 0x0409, 1, &meiryo),
                (3, 1, 0x0409, 16, &meiryo)
            ])),
            ["Meiryo UI"]
        );
        let zh = utf16("优设标题黑");
        assert_eq!(
            families_from_name_table(&name_table(&[(3, 1, 0x0804, 1, &zh)])),
            ["优设标题黑"]
        );
        assert_eq!(
            families_from_name_table(&name_table(&[(1, 0, 0, 1, b"Klee One")])),
            ["Klee One"]
        );
        assert!(
            families_from_name_table(&name_table(&[(3, 1, 0x0409, 4, &meiryo)])).is_empty(),
            "只有全名不算族名"
        );
    }

    #[test]
    fn truncated_tables_do_not_panic() {
        let table = name_table(&[(3, 1, 0x0409, 1, &utf16("Meiryo"))]);
        for len in 0..table.len() {
            let _ = families_from_name_table(&table[..len]);
        }
    }

    /// 项目自带的两个字体（没有就跳过）
    #[test]
    fn bundled_fonts_are_read() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../assets/fonts");
        if !dir.is_dir() {
            eprintln!("跳过：没有 assets/fonts");
            return;
        }
        let (files, problems) = font_files(&dir, &["不存在的字体.ttf".to_owned()]);
        let families: Vec<&str> = files.iter().map(|f| f.family.as_str()).collect();
        assert!(families.contains(&"Klee One"), "{families:?}");
        assert!(families.contains(&"LXGW WenKai"), "{families:?}");
        assert!(files.iter().all(|f| f.bundled));
        assert_eq!(problems.len(), 1, "{problems:?}");
    }

    #[test]
    fn english_name_is_the_css_name_and_cjk_names_are_aliases() {
        let pairs = |items: &[(&str, &str)]| {
            items
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            pick_names(&pairs(&[
                ("zh-cn", "微软雅黑"),
                ("en-us", "Microsoft YaHei"),
                ("ko-kr", "마이크로소프트"),
                ("zh-tw", "微軟雅黑")
            ])),
            Some(InstalledFont {
                name: "Microsoft YaHei".into(),
                aliases: vec!["微软雅黑".into(), "微軟雅黑".into()]
            })
        );
        assert_eq!(
            pick_names(&pairs(&[("zh-cn", "优设标题黑")])),
            Some(InstalledFont {
                name: "优设标题黑".into(),
                aliases: vec![]
            })
        );
        assert_eq!(pick_names(&[]), None);
    }

    /// 本机真实字体（Windows 上走 DirectWrite）：常见的日文字体都应该在
    #[test]
    fn installed_fonts_include_the_usual_japanese_fonts() {
        let fonts = installed_fonts().unwrap();
        if fonts.is_empty() {
            eprintln!("跳过：没有系统字体");
            return;
        }
        let names: Vec<&str> = fonts.iter().map(|f| f.name.as_str()).collect();
        #[cfg(windows)]
        for expected in ["Meiryo", "Yu Gothic", "MS Gothic"] {
            assert!(names.contains(&expected), "缺 {expected}");
        }
        assert!(
            names
                .windows(2)
                .all(|w| w[0].to_lowercase() <= w[1].to_lowercase()),
            "要排好序"
        );
    }

    #[test]
    fn non_font_files_are_rejected() {
        let path =
            std::env::temp_dir().join(format!("jp-app-not-a-font-{}.ttf", std::process::id()));
        std::fs::write(&path, b"hello world, not a font").unwrap();
        assert!(font_families(&path).is_err());
        let _ = std::fs::remove_file(path);
    }
}
