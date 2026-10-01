//! 词典图片：类型与尺寸。
//!
//! 类型表逐项照抄 Yomitan `ext/js/media/media-util.js`（GPL-3.0-or-later）。
//! 尺寸只读文件头、不解码像素。Yomitan 在浏览器里解码图片取宽高；这里读不出的格式
//! （AVIF、TIFF、ICO、没写尺寸的 SVG）返回 `None`，导入时记 0，由界面按图片本身大小显示。

/// JS `getFileNameExtension`：`/\.[^./\\]*$/`，含点。
fn file_name_extension(path: &str) -> &str {
    match path.rfind('.') {
        Some(i) if !path[i..].contains(['/', '\\']) => &path[i..],
        _ => "",
    }
}

pub fn image_media_type_from_file_name(path: &str) -> Option<&'static str> {
    Some(match file_name_extension(path).to_ascii_lowercase().as_str() {
        ".apng" => "image/apng",
        ".avif" => "image/avif",
        ".bmp" => "image/bmp",
        ".gif" => "image/gif",
        ".ico" | ".cur" => "image/x-icon",
        ".jpg" | ".jpeg" | ".jfif" | ".pjpeg" | ".pjp" => "image/jpeg",
        ".png" => "image/png",
        ".svg" => "image/svg+xml",
        ".tif" | ".tiff" => "image/tiff",
        ".webp" => "image/webp",
        _ => return None,
    })
}

fn be16(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes(b.get(o..o + 2)?.try_into().ok()?)))
}
fn be32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn le16(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?)))
}
fn le24(b: &[u8], o: usize) -> Option<u32> {
    let s = b.get(o..o + 3)?;
    Some(u32::from(s[0]) | u32::from(s[1]) << 8 | u32::from(s[2]) << 16)
}
fn le32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// 从文件头读宽高。
pub fn image_dimensions(bytes: &[u8], media_type: &str) -> Option<(u32, u32)> {
    match media_type {
        "image/png" | "image/apng" => png(bytes),
        "image/gif" => gif(bytes),
        "image/jpeg" => jpeg(bytes),
        "image/webp" => webp(bytes),
        "image/bmp" => bmp(bytes),
        "image/svg+xml" => svg(bytes),
        _ => None,
    }
}

fn png(b: &[u8]) -> Option<(u32, u32)> {
    if b.get(..8)? != b"\x89PNG\r\n\x1a\n" || b.get(12..16)? != b"IHDR" {
        return None;
    }
    Some((be32(b, 16)?, be32(b, 20)?))
}

fn gif(b: &[u8]) -> Option<(u32, u32)> {
    let sig = b.get(..6)?;
    if sig != b"GIF87a" && sig != b"GIF89a" {
        return None;
    }
    Some((le16(b, 6)?, le16(b, 8)?))
}

fn bmp(b: &[u8]) -> Option<(u32, u32)> {
    if b.get(..2)? != b"BM" {
        return None;
    }
    let w = le32(b, 18)? as i32;
    let h = le32(b, 22)? as i32;
    Some((w.unsigned_abs(), h.unsigned_abs()))
}

fn jpeg(b: &[u8]) -> Option<(u32, u32)> {
    if b.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let marker = b[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0x01 || (0xD0..=0xD8).contains(&marker) {
            i += 2;
            continue;
        }
        let len = be16(b, i + 2)? as usize;
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            return Some((be16(b, i + 7)?, be16(b, i + 5)?));
        }
        i += 2 + len;
    }
    None
}

fn webp(b: &[u8]) -> Option<(u32, u32)> {
    if b.get(..4)? != b"RIFF" || b.get(8..12)? != b"WEBP" {
        return None;
    }
    match b.get(12..16)? {
        b"VP8 " => {
            if b.get(23..26)? != [0x9D, 0x01, 0x2A] {
                return None;
            }
            Some((le16(b, 26)? & 0x3FFF, le16(b, 28)? & 0x3FFF))
        }
        b"VP8L" => {
            if *b.get(20)? != 0x2F {
                return None;
            }
            let bits = le32(b, 21)?;
            Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        b"VP8X" => Some((le24(b, 24)? + 1, le24(b, 27)? + 1)),
        _ => None,
    }
}

/// 只认 `<svg>` 上的 `width`/`height`（纯数字或 px），其次 `viewBox`。
fn svg(b: &[u8]) -> Option<(u32, u32)> {
    let text = std::str::from_utf8(b).ok()?;
    let start = text.find("<svg")?;
    let end = start + text[start..].find('>')?;
    let tag = &text[start..end];

    let number = |value: &str| -> Option<f64> {
        let v = value.trim();
        let v = v.strip_suffix("px").unwrap_or(v);
        v.parse::<f64>().ok().filter(|n| n.is_finite() && *n > 0.0)
    };
    if let (Some(w), Some(h)) = (attribute(tag, "width").and_then(number), attribute(tag, "height").and_then(number)) {
        return Some((w.round() as u32, h.round() as u32));
    }
    let view_box: Vec<f64> = attribute(tag, "viewBox")?
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    match view_box[..] {
        [_, _, w, h] if w > 0.0 && h > 0.0 => Some((w.round() as u32, h.round() as u32)),
        _ => None,
    }
}

/// 属性名前面必须是空白，避免把 `stroke-width` 当成 `width`。
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(pos) = tag[from..].find(name) {
        let at = from + pos;
        from = at + name.len();
        let before_ok = tag[..at].chars().next_back().is_some_and(char::is_whitespace);
        let rest = tag[from..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let rest = rest[1..].trim_start();
        let quote = rest.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let body = &rest[1..];
        return body.find(quote).map(|close| &body[..close]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_types_follow_yomitan_including_odd_extensions() {
        assert_eq!(image_media_type_from_file_name("gaiji/一.SVG"), Some("image/svg+xml"));
        assert_eq!(image_media_type_from_file_name("a.jfif"), Some("image/jpeg"));
        assert_eq!(image_media_type_from_file_name("a.cur"), Some("image/x-icon"));
        assert_eq!(image_media_type_from_file_name("dir.png/file"), None);
        assert_eq!(image_media_type_from_file_name("noext"), None);
        assert_eq!(image_media_type_from_file_name("a.txt"), None);
    }

    #[test]
    fn dimensions_come_from_the_file_header() {
        let mut png_bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png_bytes.extend_from_slice(&350u32.to_be_bytes());
        png_bytes.extend_from_slice(&120u32.to_be_bytes());
        assert_eq!(image_dimensions(&png_bytes, "image/png"), Some((350, 120)));

        let gif_bytes = [b'G', b'I', b'F', b'8', b'9', b'a', 0x10, 0x00, 0x20, 0x00];
        assert_eq!(image_dimensions(&gif_bytes, "image/gif"), Some((16, 32)));

        // 一个最小的 JPEG：SOI + APP0（长度 4）+ SOF0（高 2、宽 3）
        let jpeg_bytes = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x02,
            0x00, 0x03, 0x01, 0x01, 0x11, 0x00,
        ];
        assert_eq!(image_dimensions(&jpeg_bytes, "image/jpeg"), Some((3, 2)));

        assert_eq!(image_dimensions(b"garbage", "image/png"), None);
        assert_eq!(image_dimensions(b"", "image/avif"), None);
    }

    #[test]
    fn svg_size_prefers_width_height_then_view_box() {
        let s = br#"<svg xmlns="http://www.w3.org/2000/svg" stroke-width="3" width="24px" height="12">"#;
        assert_eq!(image_dimensions(s, "image/svg+xml"), Some((24, 12)));
        let s = br#"<?xml version="1.0"?><svg viewBox="0 0 100 50"><path/></svg>"#;
        assert_eq!(image_dimensions(s, "image/svg+xml"), Some((100, 50)));
        let s = br#"<svg width="1em" height="1em">"#;
        assert_eq!(image_dimensions(s, "image/svg+xml"), None);
    }
}
