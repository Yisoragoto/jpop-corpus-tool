//! 最小的 zip 读取器：只支持 Yomitan 词典包实际出现的「不压缩（0）/ deflate（8）」两种方法。
//!
//! 为什么自己写：解压用的 flate2、校验用的 crc32fast 都已在依赖树里，zip 的目录结构本身
//! 只有一百来行；为此再引一个 zip crate（连带一串可选压缩算法）不划算。
//! 用户的 19 个真实词典包实测：没有 zip64、只有这两种方法、非 ASCII 文件名都带 UTF-8 标志。
//! zip64 与不带 UTF-8 标志时按 CP437 解码文件名仍按规范处理（和 Yomitan 用的 zip.js 默认一致），
//! 不认识的压缩方法、加密条目明确报错。
//!
//! 每个文件解压后都核对长度和 CRC32：下载不完整的词典包宁可导入失败，也不能导入半截数据。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result, bail};
use flate2::read::DeflateDecoder;

const EOCD_SIG: u32 = 0x0605_4b50;
const ZIP64_LOCATOR_SIG: u32 = 0x0706_4b50;
const ZIP64_EOCD_SIG: u32 = 0x0606_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const LOCAL_SIG: u32 = 0x0403_4b50;

/// CP437 的 0x80–0xFF（由 Python `bytes(range(128, 256)).decode('cp437')` 生成）。
const CP437_HIGH: &str = concat!(
    "ÇüéâäàåçêëèïîìÄÅ",
    "ÉæÆôöòûùÿÖÜ¢£¥₧ƒ",
    "áíóúñÑªº¿⌐¬½¼¡«»",
    "░▒▓│┤╡╢╖╕╣║╗╝╜╛┐",
    "└┴┬├─┼╞╟╚╔╩╦╠═╬╧",
    "╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀",
    "αßΓπΣσµτΦΘΩδ∞φε∩",
    "≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}",
);

#[derive(Debug, Clone)]
pub struct ZipEntry {
    pub name: String,
    pub uncompressed_size: u64,
    flags: u16,
    method: u16,
    crc32: u32,
    compressed_size: u64,
    local_header_offset: u64,
}

pub struct ZipArchive {
    file: File,
    len: u64,
    entries: Vec<ZipEntry>,
}

fn u16_at(b: &[u8], o: usize) -> Result<u16> {
    let s = b.get(o..o + 2).context("zip 结构被截断")?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], o: usize) -> Result<u32> {
    let s = b.get(o..o + 4).context("zip 结构被截断")?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u64_at(b: &[u8], o: usize) -> Result<u64> {
    let s = b.get(o..o + 8).context("zip 结构被截断")?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Ok(u64::from_le_bytes(a))
}

fn decode_name(bytes: &[u8], utf8: bool) -> Result<String> {
    if utf8 || bytes.is_ascii() {
        return String::from_utf8(bytes.to_vec()).context("zip 文件名不是合法的 UTF-8");
    }
    let high: Vec<char> = CP437_HIGH.chars().collect();
    Ok(bytes
        .iter()
        .map(|&b| if b < 0x80 { char::from(b) } else { high[usize::from(b - 0x80)] })
        .collect())
}

impl ZipArchive {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path).with_context(|| format!("打不开 {}", path.display()))?;
        let len = file.metadata()?.len();
        if len < 22 {
            bail!("{} 不是 zip 文件（太短）", path.display());
        }
        // 目录结尾记录 22 字节，后面最多跟 65535 字节注释
        let tail_len = len.min(22 + 0xFFFF);
        file.seek(SeekFrom::Start(len - tail_len))?;
        let mut tail = vec![0u8; usize::try_from(tail_len)?];
        file.read_exact(&mut tail)?;
        let eocd = (0..=tail.len() - 22)
            .rev()
            .find(|&i| u32_at(&tail, i).ok() == Some(EOCD_SIG))
            .with_context(|| format!("{} 不是 zip 文件（找不到中央目录）", path.display()))?;

        let mut count = u64::from(u16_at(&tail, eocd + 10)?);
        let mut cd_size = u64::from(u32_at(&tail, eocd + 12)?);
        let mut cd_offset = u64::from(u32_at(&tail, eocd + 16)?);
        if eocd >= 20 && u32_at(&tail, eocd - 20)? == ZIP64_LOCATOR_SIG {
            let record_offset = u64_at(&tail, eocd - 20 + 8)?;
            file.seek(SeekFrom::Start(record_offset))?;
            let mut record = [0u8; 56];
            file.read_exact(&mut record)?;
            if u32_at(&record, 0)? != ZIP64_EOCD_SIG {
                bail!("{} 的 zip64 目录结尾损坏", path.display());
            }
            count = u64_at(&record, 32)?;
            cd_size = u64_at(&record, 40)?;
            cd_offset = u64_at(&record, 48)?;
        }
        if cd_offset.checked_add(cd_size).is_none_or(|end| end > len) {
            bail!("{} 的中央目录越界，文件可能不完整", path.display());
        }

        file.seek(SeekFrom::Start(cd_offset))?;
        let mut cd = vec![0u8; usize::try_from(cd_size)?];
        file.read_exact(&mut cd)?;

        let mut entries = Vec::new();
        let mut p = 0usize;
        for _ in 0..count {
            if u32_at(&cd, p)? != CENTRAL_SIG {
                bail!("{} 的中央目录损坏", path.display());
            }
            let flags = u16_at(&cd, p + 8)?;
            let method = u16_at(&cd, p + 10)?;
            let crc32 = u32_at(&cd, p + 16)?;
            let mut compressed_size = u64::from(u32_at(&cd, p + 20)?);
            let mut uncompressed_size = u64::from(u32_at(&cd, p + 24)?);
            let name_len = usize::from(u16_at(&cd, p + 28)?);
            let extra_len = usize::from(u16_at(&cd, p + 30)?);
            let comment_len = usize::from(u16_at(&cd, p + 32)?);
            let mut local_header_offset = u64::from(u32_at(&cd, p + 42)?);

            let name_start = p + 46;
            let name_end = name_start + name_len;
            let name_bytes = cd.get(name_start..name_end).context("zip 文件名被截断")?;
            let name = decode_name(name_bytes, flags & 0x0800 != 0)?;

            let extra = cd.get(name_end..name_end + extra_len).context("zip 扩展字段被截断")?;
            let mut q = 0;
            while q + 4 <= extra.len() {
                let id = u16_at(extra, q)?;
                let size = usize::from(u16_at(extra, q + 2)?);
                let body = extra.get(q + 4..q + 4 + size).context("zip 扩展字段被截断")?;
                if id == 0x0001 {
                    // zip64：只有 32 位字段写满 0xFFFFFFFF 的那几项才出现，顺序固定
                    let mut r = 0;
                    if uncompressed_size == 0xFFFF_FFFF {
                        uncompressed_size = u64_at(body, r)?;
                        r += 8;
                    }
                    if compressed_size == 0xFFFF_FFFF {
                        compressed_size = u64_at(body, r)?;
                        r += 8;
                    }
                    if local_header_offset == 0xFFFF_FFFF {
                        local_header_offset = u64_at(body, r)?;
                    }
                }
                q += 4 + size;
            }

            entries.push(ZipEntry {
                name,
                uncompressed_size,
                flags,
                method,
                crc32,
                compressed_size,
                local_header_offset,
            });
            p = name_end + extra_len + comment_len;
        }
        Ok(Self { file, len, entries })
    }

    /// 中央目录里的顺序（Yomitan 按这个顺序处理 term_bank，影响条目 id 的先后）。
    pub fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }

    pub fn read(&mut self, name: &str) -> Result<Vec<u8>> {
        let entry = self
            .entries
            .iter()
            .find(|e| e.name == name)
            .cloned()
            .with_context(|| format!("zip 里没有 {name}"))?;
        self.read_entry(&entry)
    }

    fn read_entry(&mut self, entry: &ZipEntry) -> Result<Vec<u8>> {
        if entry.flags & 0x0001 != 0 {
            bail!("{} 是加密的，不支持", entry.name);
        }
        self.file.seek(SeekFrom::Start(entry.local_header_offset))?;
        let mut header = [0u8; 30];
        self.file.read_exact(&mut header)?;
        if u32_at(&header, 0)? != LOCAL_SIG {
            bail!("{} 的本地文件头损坏", entry.name);
        }
        let data_start = entry.local_header_offset
            + 30
            + u64::from(u16_at(&header, 26)?)
            + u64::from(u16_at(&header, 28)?);
        if data_start.checked_add(entry.compressed_size).is_none_or(|end| end > self.len) {
            bail!("{} 的数据越界，词典包可能不完整", entry.name);
        }
        self.file.seek(SeekFrom::Start(data_start))?;

        let capacity = usize::try_from(entry.uncompressed_size).context("文件太大")?;
        let mut out = Vec::with_capacity(capacity.min(1 << 30));
        let raw = (&mut self.file).take(entry.compressed_size);
        match entry.method {
            0 => {
                let mut raw = raw;
                raw.read_to_end(&mut out)?;
            }
            8 => {
                DeflateDecoder::new(raw)
                    .read_to_end(&mut out)
                    .with_context(|| format!("解压 {} 失败，词典包可能已损坏", entry.name))?;
            }
            m => bail!("{} 用了不支持的压缩方法 {m}", entry.name),
        }
        if out.len() as u64 != entry.uncompressed_size {
            bail!(
                "{} 解压后长度不对（{} ≠ {}），词典包可能不完整",
                entry.name,
                out.len(),
                entry.uncompressed_size
            );
        }
        if crc32fast::hash(&out) != entry.crc32 {
            bail!("{} 校验和不对，词典包可能已损坏", entry.name);
        }
        Ok(out)
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::DeflateEncoder;

    pub struct Item<'a> {
        pub name: &'a [u8],
        pub data: &'a [u8],
        pub deflate: bool,
        pub utf8_flag: bool,
    }

    /// 手工拼一个 zip（测试用）。`corrupt_crc` 为真时故意写错校验和。
    pub fn build(items: &[Item<'_>], corrupt_crc: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for item in items {
            let payload = if item.deflate {
                let mut e = DeflateEncoder::new(Vec::new(), Compression::default());
                e.write_all(item.data).unwrap();
                e.finish().unwrap()
            } else {
                item.data.to_vec()
            };
            let mut crc = crc32fast::hash(item.data);
            if corrupt_crc {
                crc ^= 1;
            }
            let method: u16 = if item.deflate { 8 } else { 0 };
            let flags: u16 = if item.utf8_flag { 0x0800 } else { 0 };
            let offset = out.len() as u32;

            out.extend_from_slice(&super::LOCAL_SIG.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&flags.to_le_bytes());
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0; 4]); // 时间日期
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&(item.data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(item.name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(item.name);
            out.extend_from_slice(&payload);

            central.extend_from_slice(&super::CENTRAL_SIG.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&flags.to_le_bytes());
            central.extend_from_slice(&method.to_le_bytes());
            central.extend_from_slice(&[0; 4]);
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            central.extend_from_slice(&(item.data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(item.name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 4]); // extra、comment 长度
            central.extend_from_slice(&[0; 8]); // 磁盘号、内部属性、外部属性
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(item.name);
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&super::EOCD_SIG.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(items.len() as u16).to_le_bytes());
        out.extend_from_slice(&(items.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    /// 写到系统临时目录下的唯一文件，drop 时删掉。
    pub struct TempFile(pub std::path::PathBuf);

    impl TempFile {
        pub fn new(tag: &str, bytes: &[u8]) -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "jp_dict_test_{}_{}_{tag}.zip",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::write(&path, bytes).unwrap();
            Self(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{Item, TempFile, build};
    use super::*;

    #[test]
    fn stored_and_deflated_entries_read_back_in_directory_order() {
        let big = "打ち込む".repeat(2000);
        let bytes = build(
            &[
                Item { name: b"index.json", data: br#"{"title":"t"}"#, deflate: false, utf8_flag: false },
                Item { name: b"term_bank_1.json", data: big.as_bytes(), deflate: true, utf8_flag: false },
                Item { name: "gaiji/一.svg".as_bytes(), data: b"<svg/>", deflate: true, utf8_flag: true },
            ],
            false,
        );
        let tmp = TempFile::new("ok", &bytes);
        let mut zip = ZipArchive::open(&tmp.0).unwrap();
        let names: Vec<&str> = zip.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["index.json", "term_bank_1.json", "gaiji/一.svg"]);
        assert_eq!(zip.read("term_bank_1.json").unwrap(), big.as_bytes());
        assert_eq!(zip.read("index.json").unwrap(), br#"{"title":"t"}"#);
        assert_eq!(zip.read("gaiji/一.svg").unwrap(), b"<svg/>");
        assert!(zip.read("missing.json").is_err());
    }

    #[test]
    fn a_checksum_mismatch_is_an_error_not_silent_garbage() {
        let bytes = build(&[Item { name: b"a.json", data: b"[1,2,3]", deflate: true, utf8_flag: false }], true);
        let tmp = TempFile::new("crc", &bytes);
        let err = ZipArchive::open(&tmp.0).unwrap().read("a.json").unwrap_err();
        assert!(err.to_string().contains("校验和"), "{err}");
    }

    #[test]
    fn a_truncated_archive_is_rejected() {
        let bytes = build(&[Item { name: b"a.json", data: b"[1,2,3]", deflate: false, utf8_flag: false }], false);
        let tmp = TempFile::new("trunc", &bytes[..bytes.len() - 30]);
        assert!(ZipArchive::open(&tmp.0).is_err());
    }

    #[test]
    fn names_without_the_utf8_flag_decode_as_cp437_like_zip_js() {
        assert_eq!(CP437_HIGH.chars().count(), 128);
        assert_eq!(decode_name(&[0x80, b'a', 0xE1, 0xFF], false).unwrap(), "Çaß\u{a0}");
        assert_eq!(decode_name("一".as_bytes(), true).unwrap(), "一");
    }
}
