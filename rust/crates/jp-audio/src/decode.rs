//! 把音频文件解码成内存里的 PCM：变调渲染和 Anki 例句片段都从这里拿样本。
//!
//! **和播放走同一个解码入口**（`rodio::Decoder::try_from(File)`，`engine.rs` 里加载歌曲
//! 用的就是这一行）。所以播放器放得出来的文件这里就解得开，放不出来的这里也解不开；
//! 时间轴也是同一条——MP3 的编码器延迟怎么裁、结尾坏帧在哪儿停，两边不会各算各的。
//!
//! 不需要音频设备，CI 上能跑。

use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result, bail};
use rodio::{Decoder, Source};

/// 解码后的音频：交织的 f32 样本，范围 ±1.0。
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
    /// 交织：`[左, 右, 左, 右, …]`
    pub samples: Vec<f32>,
    pub channels: u16,
    pub rate: u32,
}

impl Pcm {
    /// 帧数（一帧 = 每个声道各一个样本）
    pub fn frames(&self) -> usize {
        match usize::from(self.channels) {
            0 => 0,
            channels => self.samples.len() / channels,
        }
    }

    pub fn seconds(&self) -> f64 {
        if self.rate == 0 { 0.0 } else { self.frames() as f64 / f64::from(self.rate) }
    }
}

fn open(path: &Path) -> Result<Decoder<std::io::BufReader<File>>> {
    let file = File::open(path).with_context(|| format!("打不开音频文件 {}", path.display()))?;
    Decoder::try_from(file).with_context(|| format!("解码失败 {}", path.display()))
}

/// 整首解码。
pub fn decode_all(path: &Path) -> Result<Pcm> {
    let decoder = open(path)?;
    let (channels, rate) = (decoder.channels().get(), decoder.sample_rate().get());
    let mut samples: Vec<f32> = match decoder.total_duration() {
        // 知道时长就一次要够，免得四五十 MB 的缓冲区反复搬家
        Some(total) => Vec::with_capacity((total.as_secs_f64() * f64::from(rate)) as usize * usize::from(channels) + 4096),
        None => Vec::new(),
    };
    samples.extend(decoder);
    // 结尾坏掉的文件可能停在半帧上
    samples.truncate(samples.len() - samples.len() % usize::from(channels));
    Ok(Pcm { samples, channels, rate })
}

/// 解出 `[from_sec, to_sec)` 这一段，起止精确到帧。
///
/// **从文件开头解码、数着样本走到起点，前面的丢掉**——不用 seek。seek 落在哪一帧
/// 各种格式各有各的说法（VBR 的 MP3 没有索引时只能估），而且有损格式从中间开始解，
/// 头几帧的样本和从头解下来的不一样。数样本没有这些问题：第 N 帧就是第 N 帧。
/// 代价是起点之前的都要解一遍，一首 4 分多钟的 FLAC 整首解完实测 0.3 秒
/// （以前 ffmpeg 的 `-ss` 写在 `-i` 后面，也是从头解的）。
///
/// 终点超出文件就到文件结尾为止；起点已经超出文件时返回空的一段。
pub fn decode_window(path: &Path, from_sec: f64, to_sec: f64) -> Result<Pcm> {
    if !(from_sec.is_finite() && to_sec.is_finite()) || from_sec < 0.0 || to_sec <= from_sec {
        bail!("时间段不对：{from_sec} – {to_sec} 秒");
    }
    let mut decoder = open(path)?;
    let (channels, rate) = (decoder.channels().get(), decoder.sample_rate().get());
    let width = usize::from(channels);
    let first = (from_sec * f64::from(rate)).round() as usize;
    let last = (to_sec * f64::from(rate)).round() as usize;

    let skip = first * width;
    if skip > 0 && decoder.nth(skip - 1).is_none() {
        return Ok(Pcm { samples: Vec::new(), channels, rate });
    }
    let mut samples: Vec<f32> = Vec::with_capacity((last - first) * width);
    samples.extend(decoder.take((last - first) * width));
    samples.truncate(samples.len() - samples.len() % width);
    Ok(Pcm { samples, channels, rate })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wav;

    /// 测试音：每一帧的值都不一样，哪一帧对到哪一帧一眼看得出来
    fn ramp(frames: usize, channels: u16) -> Vec<f32> {
        (0..frames * usize::from(channels)).map(|i| ((i % 20_011) as f32 / 20_011.0) - 0.5).collect()
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("jp-audio-decode-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_window_is_exactly_the_same_frames_as_the_full_decode() {
        let dir = scratch("window");
        let path = dir.join("ramp.wav");
        let rate = 22_050;
        wav::write_pcm16(&path, &Pcm { samples: ramp(rate as usize * 5, 2), channels: 2, rate }).unwrap();

        let whole = decode_all(&path).unwrap();
        assert_eq!((whole.channels, whole.rate, whole.frames()), (2, rate, rate as usize * 5));
        assert!((whole.seconds() - 5.0).abs() < 1e-9);

        for (from, to) in [(0.0, 1.0), (1.7, 3.2), (0.3333, 0.3334), (4.5, 5.0)] {
            let piece = decode_window(&path, from, to).unwrap();
            let (a, b) = ((from * f64::from(rate)).round() as usize, (to * f64::from(rate)).round() as usize);
            assert_eq!(piece.samples, whole.samples[a * 2..b * 2], "{from} – {to}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_window_past_the_end_stops_at_the_end() {
        let dir = scratch("end");
        let path = dir.join("short.wav");
        wav::write_pcm16(&path, &Pcm { samples: ramp(8_000, 1), channels: 1, rate: 8_000 }).unwrap();
        assert_eq!(decode_window(&path, 0.5, 9.0).unwrap().frames(), 4_000, "终点超出：到结尾为止");
        assert_eq!(decode_window(&path, 3.0, 4.0).unwrap().frames(), 0, "起点超出：空的");
        assert!(decode_window(&path, 2.0, 1.0).is_err(), "终点在起点前面要报错");
        assert!(decode_window(&path, f64::NAN, 1.0).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_audio_fails_with_its_path() {
        let dir = scratch("bad");
        let path = dir.join("假的.flac");
        std::fs::write(&path, b"this is not audio").unwrap();
        let err = format!("{:#}", decode_all(&path).unwrap_err());
        assert!(err.contains("假的.flac"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
