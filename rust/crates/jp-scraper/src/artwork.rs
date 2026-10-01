//! 封面与艺人照片：下载、校验、落盘。
//!
//! 落盘布局和 `raw/audio` 保持一致，Python 侧建的文件能直接对上：
//!
//! ```text
//! raw/covers/{歌手}/{song_id}.jpg
//! raw/artists/{歌手}.jpg
//! ```
//!
//! 两条硬要求：
//!
//! 1. **不留半截文件。** 先写 `.part` 再原子改名，下载中断不会留下
//!    一个能打开但显示一半的 jpg。
//! 2. **校验确实是图片。** Python 侧只要 HTTP 200 就原样写盘，
//!    provider 返回一个 HTML 错误页也会被存成 `.jpg`，之后每次
//!    渲染都失败而没人知道为什么。这里看文件头。

use std::path::{Path, PathBuf};

use crate::error::ProviderError;
use crate::http::HttpClient;

/// 认得的图片格式。够用就行——provider 给的封面只会是这三种。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Jpeg,
    Png,
    Webp,
}

impl ImageKind {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
        }
    }
}

/// 看文件头判断是不是图片。
///
/// 只认前几个字节，不解码——这里要挡的是「HTML 错误页被当成图片存下来」，
/// 不是要验证图片本身完整。
pub fn sniff_image(data: &[u8]) -> Option<ImageKind> {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(ImageKind::Jpeg);
    }
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(ImageKind::Png);
    }
    if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        return Some(ImageKind::Webp);
    }
    None
}

/// 歌手名转成安全的目录名，和 `raw/audio` 的处理保持一致。
pub fn safe_name(text: &str) -> String {
    let cleaned: String = text
        .trim()
        .chars()
        .map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c })
        .collect();
    // 连续的非法字符折成一个下划线，和 Python 的 `[...]+` 一致
    let mut out = String::with_capacity(cleaned.len());
    let mut last_underscore = false;
    for c in cleaned.chars() {
        if c == '_' {
            if !last_underscore {
                out.push('_');
            }
            last_underscore = true;
        } else {
            out.push(c);
            last_underscore = false;
        }
    }
    let trimmed = out.trim_matches(['.', ' ']).to_string();
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed
    }
}

/// 封面落盘位置：`raw/covers/{歌手}/{song_id}.jpg`。
pub fn cover_path_for(covers_dir: &Path, song_id: &str, artist: &str) -> PathBuf {
    covers_dir
        .join(safe_name(artist))
        .join(format!("{song_id}.jpg"))
}

/// 艺人照片位置：`raw/artists/{歌手}.jpg`。
pub fn artist_image_path_for(artists_dir: &Path, name: &str) -> PathBuf {
    artists_dir.join(format!("{}.jpg", safe_name(name)))
}

/// 下载结果。
#[derive(Debug, Clone, PartialEq)]
pub enum SaveOutcome {
    /// 下载并写盘成功
    Saved { bytes: usize, kind: ImageKind },
    /// 目标文件已存在且非空，没有重新下载
    AlreadyThere,
}

/// 图片的宽高。**只读文件头，不解码**——判断一张封面是不是方的，
/// 用不着把整张图展开。
///
/// 只认 JPEG 和 PNG。WebP 返回 `None`：这条链路上的两个源（iTunes、
/// Cover Art Archive）都不发 WebP，为了它多写一套分支不值得，
/// 而读不出尺寸的图按「照收」处理（见 [`looks_square`]），不会因此丢封面。
pub fn image_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    match sniff_image(data)? {
        ImageKind::Png => {
            // IHDR 固定在第 16 字节起：宽 4 字节、高 4 字节，大端
            let w = u32::from_be_bytes(data.get(16..20)?.try_into().ok()?);
            let h = u32::from_be_bytes(data.get(20..24)?.try_into().ok()?);
            Some((w, h))
        }
        ImageKind::Jpeg => {
            // 从 SOI 之后顺着段走，找 SOF（帧头），宽高在里面
            let mut i = 2usize;
            while i + 4 <= data.len() {
                if data[i] != 0xFF {
                    i += 1; // 段之间可能有填充的 0xFF
                    continue;
                }
                let marker = data[i + 1];
                // 这些标记不带长度字段
                if matches!(marker, 0x01 | 0xD0..=0xD9) {
                    i += 2;
                    continue;
                }
                let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
                // SOF0..SOF15，跳过 DHT(C4)、JPG(C8)、DAC(CC) 这三个不是帧头的
                if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                    let h = u16::from_be_bytes([*data.get(i + 5)?, *data.get(i + 6)?]);
                    let w = u16::from_be_bytes([*data.get(i + 7)?, *data.get(i + 8)?]);
                    return Some((u32::from(w), u32::from(h)));
                }
                if len < 2 {
                    return None; // 长度字段坏了，别越走越乱
                }
                i += 2 + len;
            }
            None
        }
        ImageKind::Webp => None,
    }
}

/// 宽高比在 4:5 和 5:4 之间算方的。
///
/// 留这么宽是因为实拍的碟面扫描常常差几个像素（`500x481` 是正经封面），
/// 而要挡的那种是 `500x281` 的 16:9 视频截图。**读不出尺寸的算方的**：
/// 不认识的格式不等于不好，宁可收下也不要丢封面。
pub fn looks_square(data: &[u8]) -> bool {
    match image_dimensions(data) {
        Some((w, h)) if w > 0 && h > 0 => (0.8..=1.25).contains(&(f64::from(w) / f64::from(h))),
        _ => true,
    }
}

/// 下载并确认是图片。不写盘。
fn fetch_image(client: &HttpClient, url: &str) -> Result<(Vec<u8>, ImageKind), ProviderError> {
    if url.is_empty() {
        return Err(ProviderError::new(
            crate::ErrorType::DownloadFailed,
            "候选没有封面 URL",
        ));
    }
    let data = client.get_bytes(url)?;
    let Some(kind) = sniff_image(&data) else {
        return Err(ProviderError::new(
            crate::ErrorType::DownloadFailed,
            format!(
                "下载到的不是图片（{} 字节，开头 {:02X?}）",
                data.len(),
                &data[..data.len().min(8)]
            ),
        ));
    };
    Ok((data, kind))
}

/// 先写 `.part` 再原子改名：下载中断不会留下一个显示一半的图。
fn write_atomic(dest: &Path, data: &[u8]) -> Result<(), ProviderError> {
    let temp = dest.with_extension(format!(
        "{}.part",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("jpg")
    ));
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ProviderError::new(crate::ErrorType::DownloadFailed, format!("建目录失败: {e}"))
        })?;
    }
    std::fs::write(&temp, data).map_err(|e| {
        ProviderError::new(
            crate::ErrorType::DownloadFailed,
            format!("写临时文件失败: {e}"),
        )
    })?;
    std::fs::rename(&temp, dest).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        ProviderError::new(crate::ErrorType::DownloadFailed, format!("改名失败: {e}"))
    })?;
    Ok(())
}

/// 下载封面写盘。
///
/// `overwrite` 为假且目标已存在时直接返回——重跑刮削不该把已经拿到的
/// 图再下一遍。
pub fn save_image(
    client: &HttpClient,
    url: &str,
    dest: &Path,
    overwrite: bool,
) -> Result<SaveOutcome, ProviderError> {
    if url.is_empty() {
        return Err(ProviderError::new(
            crate::ErrorType::DownloadFailed,
            "候选没有封面 URL",
        ));
    }
    if !overwrite
        && let Ok(meta) = std::fs::metadata(dest)
        && meta.len() > 0
    {
        return Ok(SaveOutcome::AlreadyThere);
    }

    let (data, kind) = fetch_image(client, url)?;
    write_atomic(dest, &data)?;

    Ok(SaveOutcome::Saved {
        bytes: data.len(),
        kind,
    })
}

/// 依次尝试多个 URL，第一个成功的就用它。
///
/// 为什么需要这个：实测这条链路上一张 600x600 封面 290KB 要 27 秒，
/// 网络更差的时候连 90 秒都下不完。这时退到 300x300（约 1/4 大小）
/// 甚至 100x100，**拿到一张小图也比一张都没有强**。
///
/// 返回 (用了第几个 URL, 结果)。
pub fn save_image_any(
    client: &HttpClient,
    urls: &[String],
    dest: &Path,
    overwrite: bool,
) -> Result<(usize, SaveOutcome), ProviderError> {
    let mut last: Option<ProviderError> = None;
    for (index, url) in urls.iter().enumerate() {
        if url.is_empty() {
            continue;
        }
        match save_image(client, url, dest, overwrite) {
            Ok(outcome) => return Ok((index, outcome)),
            Err(err) => last = Some(err),
        }
    }
    Err(last.unwrap_or_else(|| {
        ProviderError::new(crate::ErrorType::DownloadFailed, "没有可用的封面 URL")
    }))
}

/// 封面专用的「挑一张」：和 [`save_image_any`] 一样按顺序试，
/// 但**方的优先**，非方的只留作兜底。
///
/// 为什么单独一套：Cover Art Archive 里有的 release 传的是 MV 截图
/// （`お勉強しといてよ` 那张是 500x281，画面里还压着字幕），
/// 而同一个 release-group 下就是正经的方形单曲封面。一排方图里夹一张 16:9
/// 不只是难看——那本来就不是封面。
///
/// 非方的图不丢：全都不方时仍然写盘（有总比没有强），返回的下标是它的位置。
/// 歌手照片不走这里，用 [`save_image_any`]——人像本来就不该是方的。
pub fn save_cover_any(
    client: &HttpClient,
    urls: &[String],
    dest: &Path,
    overwrite: bool,
) -> Result<(usize, SaveOutcome), ProviderError> {
    if !overwrite
        && let Ok(meta) = std::fs::metadata(dest)
        && meta.len() > 0
    {
        return Ok((0, SaveOutcome::AlreadyThere));
    }

    let mut last: Option<ProviderError> = None;
    let mut fallback: Option<(usize, Vec<u8>, ImageKind)> = None;
    for (index, url) in urls.iter().enumerate() {
        if url.is_empty() {
            continue;
        }
        match fetch_image(client, url) {
            Ok((data, kind)) => {
                if looks_square(&data) {
                    write_atomic(dest, &data)?;
                    return Ok((
                        index,
                        SaveOutcome::Saved {
                            bytes: data.len(),
                            kind,
                        },
                    ));
                }
                // 记下第一张能用的，方的一张都没有时再拿它顶上
                if fallback.is_none() {
                    fallback = Some((index, data, kind));
                }
            }
            Err(err) => last = Some(err),
        }
    }
    if let Some((index, data, kind)) = fallback {
        write_atomic(dest, &data)?;
        return Ok((
            index,
            SaveOutcome::Saved {
                bytes: data.len(),
                kind,
            },
        ));
    }
    Err(last.unwrap_or_else(|| {
        ProviderError::new(crate::ErrorType::DownloadFailed, "没有可用的封面 URL")
    }))
}

/// 一条候选的封面 URL，由大到小、由准到备。下载时按顺序试，第一个成功的算数。
///
/// 两种源的形状不同：
/// - **iTunes** 把尺寸写在路径里（`/600x600bb.jpg`），换个数字就是另一档，不用再查一次；
/// - **MusicBrainz** 自己没有图，图在 Cover Art Archive，按 release 取；有的 release 没上传图，
///   但同一个 release-group（同一张专辑的其他版本）往往有，所以 release 之后再试 release-group。
///
/// 别的源（Deezer）给什么就用什么。
pub fn artwork_urls(candidate: &crate::models::ScrapeCandidate, sizes: &[u32]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |url: String| {
        if !url.is_empty() && !out.contains(&url) {
            out.push(url);
        }
    };

    if candidate.provider == "musicbrainz" {
        let release = candidate
            .extra
            .get("release_id")
            .cloned()
            .unwrap_or_default();
        let group = candidate
            .extra
            .get("release_group")
            .cloned()
            .unwrap_or_default();
        for size in sizes {
            push(crate::providers::musicbrainz::caa_front(
                crate::providers::musicbrainz::CAA_RELEASE,
                &release,
                caa_size(*size),
            ));
        }
        for size in sizes {
            push(crate::providers::musicbrainz::caa_front(
                crate::providers::musicbrainz::CAA_RELEASE_GROUP,
                &group,
                caa_size(*size),
            ));
        }
        return out;
    }

    let base = if candidate.artwork_url.is_empty() {
        &candidate.thumb_url
    } else {
        &candidate.artwork_url
    };
    if base.is_empty() {
        return out;
    }
    for size in sizes {
        push(crate::providers::itunes::hi_res_artwork(base, *size));
    }
    out
}

/// CAA 只有 250 / 500 / 1200 三档，把想要的尺寸归到**最近**的那一档。
///
/// 取最近而不是取不小于：想要 600 时 1200 是它四倍的像素、几倍的流量，
/// 而 500 已经超过界面上任何一处的显示尺寸。差 100 像素看不出来，差 800 KB 看得出来。
fn caa_size(size: u32) -> u32 {
    let tiers = [250u32, 500, 1200];
    *tiers
        .iter()
        .min_by_key(|tier| tier.abs_diff(size))
        .expect("tiers 不为空")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpConfig, HttpResponse, Transport};
    use std::collections::BTreeMap;

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0, 0, 0];
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x00";

    fn client(body: Vec<u8>) -> HttpClient {
        struct One(std::sync::Mutex<Option<Vec<u8>>>);
        impl Transport for One {
            fn get(
                &self,
                _u: &str,
                _h: &[(String, String)],
                _t: crate::http::Timeouts,
            ) -> Result<HttpResponse, ProviderError> {
                Ok(HttpResponse {
                    status: 200,
                    body: self.0.lock().unwrap().take().unwrap_or_default(),
                    headers: BTreeMap::new(),
                })
            }
        }
        HttpClient::new(Box::new(One(std::sync::Mutex::new(Some(body)))))
    }

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jp-artwork-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 最小的 JPEG：SOI 之后直接跟一个 SOF0，宽高就在里面
    fn jpeg_of(w: u16, h: u16) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
        out.extend_from_slice(&h.to_be_bytes());
        out.extend_from_slice(&w.to_be_bytes());
        out.extend_from_slice(&[0x03, 0x01, 0x22, 0x00]);
        out
    }

    fn png_of(w: u32, h: u32) -> Vec<u8> {
        let mut out = PNG[..8].to_vec();
        out.extend_from_slice(&13u32.to_be_bytes());
        out.extend_from_slice(b"IHDR");
        out.extend_from_slice(&w.to_be_bytes());
        out.extend_from_slice(&h.to_be_bytes());
        out
    }

    /// 按 URL 发不同的 body，用来验证「换一个 URL 再试」
    fn client_by_url(pairs: Vec<(&str, Vec<u8>)>) -> HttpClient {
        struct ByUrl(BTreeMap<String, Vec<u8>>);
        impl Transport for ByUrl {
            fn get(
                &self,
                url: &str,
                _h: &[(String, String)],
                _t: crate::http::Timeouts,
            ) -> Result<HttpResponse, ProviderError> {
                match self.0.get(url) {
                    Some(body) => Ok(HttpResponse {
                        status: 200,
                        body: body.clone(),
                        headers: BTreeMap::new(),
                    }),
                    None => Err(ProviderError::new(crate::ErrorType::NotFound, "404")),
                }
            }
        }
        let map = pairs.into_iter().map(|(u, b)| (u.to_string(), b)).collect();
        HttpClient::new(Box::new(ByUrl(map)))
    }

    fn mb_candidate() -> crate::models::ScrapeCandidate {
        let mut c = crate::models::ScrapeCandidate {
            provider: "musicbrainz".into(),
            provider_id: "rec-1".into(),
            ..Default::default()
        };
        c.extra.insert("release_id".into(), "rel-1".into());
        c.extra.insert("release_group".into(), "rg-1".into());
        c
    }

    #[test]
    fn musicbrainz_art_comes_from_the_cover_art_archive_release_then_the_group() {
        // MusicBrainz 自己没有图。有的 release 没人上传封面，但同一个
        // release-group 下的另一个版本有——所以 release 试完再试 group
        let urls = artwork_urls(&mb_candidate(), &[600, 300, 200, 100]);
        assert_eq!(
            urls,
            vec![
                // 600 和 300 都归到 500 档，去重后只剩一条
                "https://coverartarchive.org/release/rel-1/front-500",
                "https://coverartarchive.org/release/rel-1/front-250",
                "https://coverartarchive.org/release-group/rg-1/front-500",
                "https://coverartarchive.org/release-group/rg-1/front-250",
            ]
        );
    }

    #[test]
    fn a_requested_size_lands_on_the_nearest_cover_art_archive_tier() {
        // CAA 只有三档；600 取 500 而不是 1200——差 100 像素看不出来，差 800 KB 看得出来
        assert_eq!(caa_size(100), 250);
        assert_eq!(caa_size(300), 250);
        assert_eq!(caa_size(400), 500);
        assert_eq!(caa_size(600), 500);
        assert_eq!(caa_size(900), 1200);
    }

    #[test]
    fn itunes_art_is_rewritten_in_place_without_another_query() {
        // iTunes 把尺寸写在路径里，换个数字就是另一档
        let candidate = crate::models::ScrapeCandidate {
            provider: "itunes".into(),
            artwork_url: "https://is1-ssl.mzstatic.com/image/thumb/x/y/z.jpg/600x600bb.jpg".into(),
            ..Default::default()
        };
        let urls = artwork_urls(&candidate, &[600, 300]);
        assert_eq!(urls.len(), 2);
        assert!(urls[0].ends_with("/600x600bb.jpg"), "{:?}", urls[0]);
        assert!(urls[1].ends_with("/300x300bb.jpg"), "{:?}", urls[1]);

        // 一条封面 URL 都没有的候选不该凭空造出地址
        let bare = crate::models::ScrapeCandidate {
            provider: "itunes".into(),
            ..Default::default()
        };
        assert!(artwork_urls(&bare, &[600]).is_empty());
    }

    #[test]
    fn image_size_is_read_from_the_header() {
        assert_eq!(image_dimensions(&jpeg_of(500, 281)), Some((500, 281)));
        assert_eq!(image_dimensions(&jpeg_of(1200, 1200)), Some((1200, 1200)));
        assert_eq!(image_dimensions(&png_of(250, 250)), Some((250, 250)));
        // 不是图片、或者头不完整，都是 None 而不是 panic
        assert_eq!(image_dimensions(b"<!DOCTYPE html>"), None);
        assert_eq!(image_dimensions(&[0xFF, 0xD8, 0xFF]), None);

        // 扫描件差几个像素还算方的；16:9 的不算
        assert!(looks_square(&jpeg_of(500, 481)));
        assert!(looks_square(&jpeg_of(500, 500)));
        assert!(!looks_square(&jpeg_of(500, 281)));
        // 读不出尺寸的按方的算：不认识不等于不好，宁可收下
        assert!(looks_square(b"RIFF\0\0\0\0WEBPVP8 "));
    }

    #[test]
    fn a_sixteen_by_nine_video_still_loses_to_the_square_cover() {
        // 真事：お勉強しといてよ 的 release 在 CAA 上传的是带字幕的 MV 截图（500x281），
        // 同一个 release-group 下才是正经的方形单曲封面
        let dir = tempdir("square");
        let dest = dir.join("165.jpg");
        let wide = jpeg_of(500, 281);
        let square = jpeg_of(500, 500);
        let urls = vec![
            "https://caa/release/front-500".to_string(),
            "https://caa/release-group/front-500".to_string(),
        ];
        let c = client_by_url(vec![
            ("https://caa/release/front-500", wide.clone()),
            ("https://caa/release-group/front-500", square.clone()),
        ]);
        let (index, outcome) = save_cover_any(&c, &urls, &dest, true).unwrap();
        assert_eq!(index, 1, "用的应该是第二个 URL");
        assert!(matches!(outcome, SaveOutcome::Saved { .. }));
        assert_eq!(std::fs::read(&dest).unwrap(), square);
    }

    #[test]
    fn a_non_square_cover_is_still_better_than_none() {
        // 全都不方时照样写盘——有封面比没有强
        let dir = tempdir("square-none");
        let dest = dir.join("165.jpg");
        let wide = jpeg_of(500, 281);
        let urls = vec!["https://caa/release/front-500".to_string()];
        let c = client_by_url(vec![("https://caa/release/front-500", wide.clone())]);
        let (index, _) = save_cover_any(&c, &urls, &dest, true).unwrap();
        assert_eq!(index, 0);
        assert_eq!(std::fs::read(&dest).unwrap(), wide);
    }

    #[test]
    fn an_existing_cover_is_not_replaced_by_a_rounder_one() {
        // overwrite=false 的语义不变：已经有图就不下了，哪怕新的更方
        let dir = tempdir("square-skip");
        let dest = dir.join("165.jpg");
        std::fs::write(&dest, jpeg_of(500, 281)).unwrap();
        let urls = vec!["https://caa/release-group/front-500".to_string()];
        let c = client_by_url(vec![(
            "https://caa/release-group/front-500",
            jpeg_of(500, 500),
        )]);
        let (_, outcome) = save_cover_any(&c, &urls, &dest, false).unwrap();
        assert_eq!(outcome, SaveOutcome::AlreadyThere);
        assert_eq!(std::fs::read(&dest).unwrap(), jpeg_of(500, 281));
    }

    #[test]
    fn image_formats_are_recognised_by_their_header() {
        assert_eq!(sniff_image(JPEG), Some(ImageKind::Jpeg));
        assert_eq!(sniff_image(PNG), Some(ImageKind::Png));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some(ImageKind::Webp));
        assert_eq!(sniff_image(b"<!DOCTYPE html>"), None);
        assert_eq!(sniff_image(b""), None);
    }

    #[test]
    fn an_html_error_page_is_not_saved_as_a_jpg() {
        // Python 侧只要 HTTP 200 就写盘，于是错误页被存成 .jpg，
        // 之后每次渲染都失败而没人知道为什么
        let dir = tempdir("html");
        let dest = dir.join("x.jpg");
        let err = save_image(
            &client(b"<!DOCTYPE html><html>403</html>".to_vec()),
            "https://x/cover.jpg",
            &dest,
            true,
        )
        .unwrap_err();
        assert_eq!(err.error_type, crate::ErrorType::DownloadFailed);
        assert!(!dest.exists(), "不该留下文件");
    }

    #[test]
    fn a_real_image_is_written_atomically() {
        let dir = tempdir("ok");
        let dest = dir.join("歌手").join("001.jpg");
        let out = save_image(&client(JPEG.to_vec()), "https://x/c.jpg", &dest, true).unwrap();
        assert_eq!(
            out,
            SaveOutcome::Saved {
                bytes: JPEG.len(),
                kind: ImageKind::Jpeg
            }
        );
        assert_eq!(std::fs::read(&dest).unwrap(), JPEG);
        // 不该留下 .part
        let leftovers: Vec<_> = std::fs::read_dir(dest.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".part"))
            .collect();
        assert!(leftovers.is_empty(), "留下了临时文件");
    }

    #[test]
    fn an_existing_file_is_not_downloaded_again() {
        let dir = tempdir("skip");
        let dest = dir.join("001.jpg");
        std::fs::write(&dest, JPEG).unwrap();
        // 传一个会返回空 body 的 client：真去下载的话会因为不是图片而失败
        let out = save_image(&client(Vec::new()), "https://x/c.jpg", &dest, false).unwrap();
        assert_eq!(out, SaveOutcome::AlreadyThere);
    }

    #[test]
    fn an_empty_placeholder_file_is_re_downloaded() {
        // 上次下载失败留下的 0 字节文件不该挡住重试
        let dir = tempdir("empty");
        let dest = dir.join("001.jpg");
        std::fs::write(&dest, b"").unwrap();
        let out = save_image(&client(JPEG.to_vec()), "https://x/c.jpg", &dest, false).unwrap();
        assert!(matches!(out, SaveOutcome::Saved { .. }));
    }

    #[test]
    fn no_url_means_a_classified_error_not_a_panic() {
        let dir = tempdir("nourl");
        let err = save_image(&client(vec![]), "", &dir.join("x.jpg"), true).unwrap_err();
        assert_eq!(err.error_type, crate::ErrorType::DownloadFailed);
    }

    #[test]
    fn safe_name_matches_the_audio_folder_convention() {
        assert_eq!(safe_name("サカナクション"), "サカナクション");
        assert_eq!(safe_name("Mrs. GREEN APPLE"), "Mrs. GREEN APPLE");
        assert_eq!(safe_name("AC/DC"), "AC_DC");
        assert_eq!(safe_name("a:b*c?d"), "a_b_c_d");
        // 连续非法字符折成一个
        assert_eq!(safe_name("a///b"), "a_b");
        // 全是标点或空的退到 unknown
        assert_eq!(safe_name("  "), "unknown");
        assert_eq!(safe_name("..."), "unknown");
        // 尾部的点会让 Windows 路径出问题
        assert_eq!(
            safe_name("ずっと真夜中でいいのに。"),
            "ずっと真夜中でいいのに。"
        );
    }

    #[test]
    fn paths_follow_the_library_layout() {
        let covers = Path::new("D:/jp_corpus/raw/covers");
        let got = cover_path_for(covers, "001", "サカナクション");
        assert!(
            got.ends_with("サカナクション/001.jpg") || got.ends_with(r"サカナクション\001.jpg")
        );

        let artists = Path::new("D:/jp_corpus/raw/artists");
        let got = artist_image_path_for(artists, "AC/DC");
        assert!(got.to_string_lossy().ends_with("AC_DC.jpg"), "{got:?}");
    }

    #[test]
    fn a_download_failure_leaves_nothing_behind() {
        let dir = tempdir("fail");
        let dest = dir.join("001.jpg");
        struct Broken;
        impl Transport for Broken {
            fn get(
                &self,
                _u: &str,
                _h: &[(String, String)],
                _t: crate::http::Timeouts,
            ) -> Result<HttpResponse, ProviderError> {
                Err(ProviderError::network("断网"))
            }
        }
        // retries=0：网络错误是可重试的，默认配置会真的退避等 1.8 秒
        let c = HttpClient::new(Box::new(Broken)).with_config(HttpConfig {
            retries: 0,
            ..Default::default()
        });
        assert!(save_image(&c, "https://x/c.jpg", &dest, true).is_err());
        assert!(!dest.exists(), "失败不该留下半截文件");
    }
    #[test]
    fn a_slow_large_image_falls_back_to_a_smaller_one() {
        // 实测 600x600 在这条链路上要 27 秒，网络差的时候下不完。
        // 拿到一张小图也比一张都没有强。
        let dir = tempdir("fallback");
        let dest = dir.join("001.jpg");
        struct FirstFails(std::sync::atomic::AtomicUsize);
        impl Transport for FirstFails {
            fn get(
                &self,
                url: &str,
                _h: &[(String, String)],
                _t: crate::http::Timeouts,
            ) -> Result<HttpResponse, ProviderError> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if url.contains("600x600") {
                    return Err(ProviderError::timeout("太大了下不完"));
                }
                Ok(HttpResponse {
                    status: 200,
                    body: JPEG.to_vec(),
                    headers: BTreeMap::new(),
                })
            }
        }
        let c = HttpClient::new(Box::new(FirstFails(Default::default()))).with_config(HttpConfig {
            retries: 0,
            ..Default::default()
        });
        let urls = vec![
            "https://x/600x600bb.jpg".to_string(),
            "https://x/300x300bb.jpg".to_string(),
        ];
        let (index, outcome) = save_image_any(&c, &urls, &dest, true).unwrap();
        assert_eq!(index, 1, "该退到第二个 URL");
        assert!(matches!(outcome, SaveOutcome::Saved { .. }));
        assert!(dest.exists());
    }

    #[test]
    fn every_url_failing_reports_the_last_error() {
        let dir = tempdir("allfail");
        struct Dead;
        impl Transport for Dead {
            fn get(
                &self,
                _u: &str,
                _h: &[(String, String)],
                _t: crate::http::Timeouts,
            ) -> Result<HttpResponse, ProviderError> {
                Err(ProviderError::timeout("超时"))
            }
        }
        let c = HttpClient::new(Box::new(Dead)).with_config(HttpConfig {
            retries: 0,
            ..Default::default()
        });
        let urls = vec!["https://x/a.jpg".to_string(), "https://x/b.jpg".to_string()];
        let err = save_image_any(&c, &urls, &dir.join("x.jpg"), true).unwrap_err();
        assert_eq!(err.error_type, crate::ErrorType::Timeout);
    }

    #[test]
    fn an_empty_url_list_is_a_classified_error() {
        let dir = tempdir("empty-list");
        let c = HttpClient::new(Box::new(
            super::super::http::testing::ScriptedTransport::new(vec![]),
        ));
        let err = save_image_any(&c, &[], &dir.join("x.jpg"), true).unwrap_err();
        assert_eq!(err.error_type, crate::ErrorType::DownloadFailed);
    }
}
