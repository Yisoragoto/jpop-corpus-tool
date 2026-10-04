//! 把 PCM 写成 16 位的 WAV（变调缓存用）。
//!
//! 为什么是 WAV：写出来不用编码器，四十几行就够，不为它加依赖；解码也最快，
//! 换调时切过去没有停顿。代价是比 FLAC 大三成左右（一首 4 分钟的歌约 45 MB），
//! 所以缓存目录有总量上限（见 `pitch.rs`）。
//!
//! # 削波
//!
//! 变调后的样本会超出 ±1.0。这里转整数时夹在 16 位范围内（硬削）：**无论上游给什么，都不能回绕**
//! （正峰回绕成负的满幅值是一声爆音）。NaN 当静音。为什么是硬削而不是限幅，见 `pitch.rs` 的「超幅」一节。

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::decode::Pcm;

/// 一个样本转 16 位。满幅 ±1.0 对 ±32767，超出的夹住，NaN 当 0。
pub fn quantize(sample: f32) -> i16 {
    // `as` 对浮点转整数是饱和的、NaN 得 0；这里先夹一遍只是把意图写在明面上
    (sample * 32767.0).round().clamp(-32768.0, 32767.0) as i16
}

/// 写一个 16 位 PCM 的 WAV。
pub fn write_pcm16(path: &Path, pcm: &Pcm) -> Result<()> {
    let channels = u32::from(pcm.channels);
    if channels == 0 || pcm.rate == 0 {
        bail!("音频没有声道或采样率（{} 声道，{} Hz）", pcm.channels, pcm.rate);
    }
    // RIFF 的长度字段是 32 位。16 位立体声 44.1 kHz 要 6 个多小时才会碰到
    let bytes = pcm.samples.len() as u64 * 2;
    let Ok(data_len) = u32::try_from(bytes).and_then(|n| u32::try_from(u64::from(n) + 36).map(|_| n)) else {
        bail!("音频太长，写不成 WAV（{bytes} 字节）");
    };

    let file = File::create(path).with_context(|| format!("建不了文件 {}", path.display()))?;
    let mut out = BufWriter::with_capacity(1 << 16, file);
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + data_len).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes()); // PCM
    header.extend_from_slice(&pcm.channels.to_le_bytes());
    header.extend_from_slice(&pcm.rate.to_le_bytes());
    header.extend_from_slice(&(pcm.rate * channels * 2).to_le_bytes());
    header.extend_from_slice(&(pcm.channels * 2).to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_len.to_le_bytes());
    let write = |out: &mut BufWriter<File>| -> std::io::Result<()> {
        out.write_all(&header)?;
        for sample in &pcm.samples {
            out.write_all(&quantize(*sample).to_le_bytes())?;
        }
        out.flush()
    };
    write(&mut out).with_context(|| format!("写不了 {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::decode_all;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("jp-audio-wav-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn in_range_audio_is_written_as_is() {
        let dir = scratch("plain");
        let path = dir.join("a.wav");
        let pcm = Pcm { samples: vec![0.0, 0.5, -0.5, 1.0, -1.0, 0.25], channels: 2, rate: 8_000 };
        write_pcm16(&path, &pcm).unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 44 + 12);
        let values: Vec<i16> = bytes[44..].as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect();
        assert_eq!(values, [0, 16_384, -16_384, 32_767, -32_767, 8_192]);

        // 播放用的解码器读得回来，声道、采样率、帧数都对
        let back = decode_all(&path).unwrap();
        assert_eq!((back.channels, back.rate, back.frames()), (2, 8_000, 3));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 超幅的、不是数的样本：夹到满幅或当静音，绝不回绕。
    /// （`1.6 * 32767` 直接转 i16 再截断的话会变成负数。）
    #[test]
    fn out_of_range_samples_saturate_and_never_wrap() {
        assert_eq!(quantize(1.6), 32_767);
        assert_eq!(quantize(-1.6), -32_768);
        assert_eq!(quantize(7.0), 32_767);
        assert_eq!(quantize(f32::INFINITY), 32_767);
        assert_eq!(quantize(f32::NEG_INFINITY), -32_768);
        assert_eq!(quantize(f32::NAN), 0);

        let dir = scratch("overs");
        let path = dir.join("a.wav");
        write_pcm16(&path, &Pcm { samples: vec![0.4, 1.6, -1.6, f32::NAN], channels: 1, rate: 8_000 }).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let values: Vec<i16> = bytes[44..].as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect();
        assert_eq!(values, [13_107, 32_767, -32_768, 0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn audio_without_channels_is_refused() {
        let dir = scratch("zero");
        assert!(write_pcm16(&dir.join("a.wav"), &Pcm { samples: vec![], channels: 0, rate: 44_100 }).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
