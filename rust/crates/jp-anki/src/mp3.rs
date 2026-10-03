//! 把一小段 PCM 编成 MP3（LAME，VBR 质量 5）。Anki 例句片段用。
//!
//! 以前是 `ffmpeg -acodec libmp3lame -q:a 5`。ffmpeg 里的 libmp3lame 就是 LAME，
//! `-q:a 5` 就是 LAME 的 VBR 质量 5（`-V5`）；这里直接调 LAME，参数相同
//! （同一段 4.8 秒的音频，ffmpeg 出来 76,025 字节，这里 74,983 字节）。
//!
//! # LAME 信息帧（Xing 头）
//!
//! VBR 的 MP3 每帧码率不同，播放器没法用「文件大小 ÷ 码率」算时长，要靠文件开头一个
//! 特殊的帧：里面记着总帧数，LAME 的扩展部分还记着编码器在开头垫了多少样本、结尾补了多少。
//! 没有它，时长显示不准，开头还会多出约 26 毫秒的静音。
//!
//! LAME 的做法是：编码一开始先在流的最前面占一个全零的帧，编完之后才知道该填什么，
//! 由调用方**覆盖回文件开头**（`lame_get_lametag_frame`）。[`encode`] 做了这一步，
//! 测试里解析这个帧，核对帧数和时长。

use anyhow::{Result, anyhow, bail};
use jp_audio::Pcm;
use mp3lame_encoder::{Builder, FlushGap, InterleavedPcm, MonoPcm, Quality, VbrMode};

/// VBR 质量。5 对应 ffmpeg 的 `-q:a 5`，44.1 kHz 立体声平均 130 kbps 上下
const VBR_QUALITY: Quality = Quality::Good;

/// 编成 MP3。单声道和立体声原样编；更多声道的折成立体声（见 [`to_stereo`]）。
pub fn encode(pcm: &Pcm) -> Result<Vec<u8>> {
    if pcm.frames() == 0 {
        bail!("没有可编码的音频");
    }
    let folded;
    let pcm = if pcm.channels > 2 {
        folded = to_stereo(pcm);
        &folded
    } else {
        pcm
    };

    let lame = |what: &str, err: mp3lame_encoder::BuildError| anyhow!("LAME 不接受{what}：{err}");
    let mut builder = Builder::new().ok_or_else(|| anyhow!("LAME 初始化失败（内存不足）"))?;
    builder.set_num_channels(pcm.channels as u8).map_err(|e| lame(&format!(" {} 声道", pcm.channels), e))?;
    builder.set_sample_rate(pcm.rate).map_err(|e| lame(&format!("采样率 {} Hz", pcm.rate), e))?;
    builder.set_vbr_mode(VbrMode::Mtrh).map_err(|e| lame(" VBR 模式", e))?;
    builder.set_vbr_quality(VBR_QUALITY).map_err(|e| lame(" VBR 质量", e))?;
    builder.set_to_write_vbr_tag(true).map_err(|e| lame("写信息帧", e))?;
    let mut encoder = builder.build().map_err(|e| lame("这组参数", e))?;

    let mut out: Vec<u8> = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(pcm.frames()));
    let encoded = if pcm.channels == 1 {
        encoder.encode_to_vec(MonoPcm(&pcm.samples[..]), &mut out)
    } else {
        encoder.encode_to_vec(InterleavedPcm(&pcm.samples[..]), &mut out)
    };
    encoded.map_err(|err| anyhow!("LAME 编码失败：{err}"))?;
    // 收尾：最后一帧不满的用静音补齐。补了多少记在信息帧里，解码时会裁掉
    out.reserve(7200);
    encoder.flush_to_vec::<FlushGap>(&mut out).map_err(|err| anyhow!("LAME 收尾失败：{err}"))?;

    // 信息帧：覆盖流最前面那个占位的帧
    let mut tag: Vec<u8> = Vec::with_capacity(encoder.lame_tag_size().max(1));
    let written = encoder.lame_tag_encode_to_vec(&mut tag).map(|n| n.get()).unwrap_or(0);
    if written == 0 || written > out.len() {
        bail!("LAME 没有给出信息帧（{written} 字节，音频 {} 字节）", out.len());
    }
    out[..written].copy_from_slice(&tag[..written]);
    Ok(out)
}

/// 多声道折成立体声：偶数号声道平均成左，奇数号平均成右。
///
/// 只求不丢声音——5.1 的标准下混还要给中置和环绕配系数，曲库里没有这种文件，不为它写一套。
fn to_stereo(pcm: &Pcm) -> Pcm {
    let channels = usize::from(pcm.channels);
    let (lefts, rights) = (channels.div_ceil(2) as f32, (channels / 2).max(1) as f32);
    let mut samples = Vec::with_capacity(pcm.frames() * 2);
    for frame in pcm.samples.chunks_exact(channels) {
        let left: f32 = frame.iter().step_by(2).sum();
        let right: f32 = frame.iter().skip(1).step_by(2).sum();
        samples.extend([left / lefts, right / rights]);
    }
    Pcm { samples, channels: 2, rate: pcm.rate }
}

/// 测试里用来核对信息帧：从 MP3 开头的 Xing/Info 帧里读出来的东西。
#[cfg(test)]
pub(crate) mod inspect {
    /// MPEG 音频帧头里读出来的几样
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Header {
        pub rate: u32,
        pub channels: u16,
        /// 一帧多少个样本（MPEG-1 Layer III 是 1152，MPEG-2/2.5 是 576）
        pub samples_per_frame: u32,
        /// 这一帧占多少字节
        pub frame_bytes: usize,
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct InfoTag {
        pub header: Header,
        /// `Xing` 是 VBR，`Info` 是 CBR
        pub vbr: bool,
        /// 音频帧数（不算信息帧自己）
        pub frames: u32,
        /// 音频数据的字节数
        pub bytes: u32,
        /// 编码器在开头垫的样本数
        pub encoder_delay: u32,
        /// 结尾补的样本数
        pub encoder_padding: u32,
        /// Xing 的质量字段：`100 − 10 × VBR 质量 − 算法质量`
        pub quality: u32,
        /// LAME 扩展里的 VBR 方法：1 = CBR，2 = ABR，3 = 旧 VBR，4 = mtrh（现在的默认 VBR）
        pub vbr_method: u8,
    }

    impl InfoTag {
        /// 信息帧说这个文件里有多少个「真正的」样本
        pub fn samples(&self) -> u64 {
            u64::from(self.frames) * u64::from(self.header.samples_per_frame)
                - u64::from(self.encoder_delay)
                - u64::from(self.encoder_padding)
        }

        pub fn seconds(&self) -> f64 {
            self.samples() as f64 / f64::from(self.header.rate)
        }
    }

    pub fn header(bytes: &[u8]) -> Option<Header> {
        let b = bytes.get(..4)?;
        if b[0] != 0xFF || b[1] & 0xE0 != 0xE0 {
            return None;
        }
        let version = (b[1] >> 3) & 3; // 3 = MPEG-1，2 = MPEG-2，0 = MPEG-2.5
        let layer = (b[1] >> 1) & 3; // 1 = Layer III
        if version == 1 || layer != 1 {
            return None;
        }
        let bitrate_index = usize::from(b[2] >> 4);
        let rate_index = usize::from((b[2] >> 2) & 3);
        let padding = usize::from((b[2] >> 1) & 1);
        let mono = (b[3] >> 6) == 3;
        let mpeg1 = version == 3;
        let kbps: [usize; 16] = if mpeg1 {
            [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0]
        } else {
            [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0]
        };
        let base = *[44_100u32, 48_000, 32_000].get(rate_index)?;
        let rate = match version {
            3 => base,
            2 => base / 2,
            _ => base / 4,
        };
        let samples_per_frame = if mpeg1 { 1152 } else { 576 };
        let frame_bytes = samples_per_frame as usize / 8 * kbps[bitrate_index] * 1000 / rate as usize + padding;
        (frame_bytes > 4).then_some(Header { rate, channels: if mono { 1 } else { 2 }, samples_per_frame, frame_bytes })
    }

    /// 读文件开头的信息帧。没有就是 None。
    pub fn info_tag(mp3: &[u8]) -> Option<InfoTag> {
        let header = header(mp3)?;
        let mpeg1 = header.samples_per_frame == 1152;
        // 帧头 4 字节之后是 side info，长度看版本和声道数；信息帧的标记紧跟其后
        let side = match (mpeg1, header.channels) {
            (true, 1) => 17,
            (true, _) => 32,
            (false, 1) => 9,
            (false, _) => 17,
        };
        let at = 4 + side;
        let vbr = match mp3.get(at..at + 4)? {
            b"Xing" => true,
            b"Info" => false,
            _ => return None,
        };
        let u32_at = |offset: usize| mp3.get(offset..offset + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
        let flags = u32_at(at + 4)?;
        // 要有帧数（位 0）和字节数（位 1）；LAME 还会写 TOC（位 2）和质量（位 3）
        if flags & 0b11 != 0b11 {
            return None;
        }
        let frames = u32_at(at + 8)?;
        let bytes = u32_at(at + 12)?;
        // Xing 部分：标记 4 + 标志 4 + 帧数 4 + 字节数 4 + TOC 100 + 质量 4 = 120 字节；
        // 后面是 LAME 扩展：版本串 9 字节 …… 第 21 字节起的 3 个字节是延迟和补齐，各 12 位
        let lame = at + 120;
        if mp3.get(lame..lame + 4)? != b"LAME" {
            return None;
        }
        let d = mp3.get(lame + 21..lame + 24)?;
        let encoder_delay = (u32::from(d[0]) << 4) | (u32::from(d[1]) >> 4);
        let encoder_padding = ((u32::from(d[1]) & 0x0F) << 8) | u32::from(d[2]);
        let quality = u32_at(at + 116)?;
        let vbr_method = *mp3.get(lame + 9)? & 0x0F;
        Some(InfoTag { header, vbr, frames, bytes, encoder_delay, encoder_padding, quality, vbr_method })
    }

    /// 数一遍文件里的帧（跳过开头的信息帧），顺带确认帧一个接一个、中间没有垃圾
    pub fn count_audio_frames(mp3: &[u8]) -> Option<(u32, usize)> {
        let first = header(mp3)?;
        let mut at = first.frame_bytes;
        let mut frames = 0u32;
        while at + 4 <= mp3.len() {
            let h = header(&mp3[at..])?;
            at += h.frame_bytes;
            frames += 1;
        }
        Some((frames, mp3.len() - first.frame_bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::inspect::{count_audio_frames, info_tag};
    use super::*;

    fn tone(seconds: f64, rate: u32, channels: u16) -> Pcm {
        let frames = (seconds * f64::from(rate)).round() as usize;
        let mut samples = Vec::with_capacity(frames * usize::from(channels));
        for i in 0..frames {
            let t = i as f64 / f64::from(rate);
            // 一个慢慢升高的音：每一刻的频率都不一样，对不齐的话一比就知道
            let value = (0.4 * (2.0 * std::f64::consts::PI * (300.0 * t + 150.0 * t * t)).sin()) as f32;
            for c in 0..channels {
                samples.push(if c == 0 { value } else { value * 0.8 });
            }
        }
        Pcm { samples, channels, rate }
    }

    /// 验收条件：信息帧必须有，里面的时长必须对。
    ///
    /// 帧数、字节数要和文件里实际数出来的一致；按「帧数 × 每帧样本 − 开头垫的 − 结尾补的」
    /// 算出来的样本数要**正好等于**喂进去的样本数——播放器显示的时长就是这么算的。
    #[test]
    fn the_info_frame_is_written_and_its_duration_is_exact() {
        for (seconds, rate, channels) in [(4.8, 44_100, 2), (1.3, 44_100, 1), (2.0, 48_000, 2), (0.9, 22_050, 2)] {
            let pcm = tone(seconds, rate, channels);
            let mp3 = encode(&pcm).unwrap();
            let label = format!("{seconds} 秒 {rate} Hz {channels} 声道");

            let tag = info_tag(&mp3).unwrap_or_else(|| panic!("{label}：文件开头没有 LAME 信息帧"));
            assert!(tag.vbr, "{label}：VBR 的文件要写 Xing，不是 Info");
            assert_eq!((tag.header.rate, tag.header.channels), (rate, channels), "{label}");

            let (frames, bytes) = count_audio_frames(&mp3).unwrap_or_else(|| panic!("{label}：帧接不上"));
            assert_eq!(tag.frames, frames, "{label}：信息帧里的帧数");
            assert_eq!(tag.bytes as usize, bytes + tag.header.frame_bytes, "{label}：信息帧里的字节数（LAME 把信息帧自己也算在内）");

            assert_eq!(tag.samples(), pcm.frames() as u64, "{label}：信息帧算出来的样本数");
            assert!((tag.seconds() - seconds).abs() < 1.0 / f64::from(rate), "{label}：时长 {}", tag.seconds());
            // LAME 固定在开头垫 576 个样本
            assert_eq!(tag.encoder_delay, 576, "{label}");
        }
    }

    /// 占位的帧是被**覆盖**掉的，不是在前面又插了一个：文件里只有一个信息帧，
    /// 第二帧起就是音频。
    #[test]
    fn there_is_exactly_one_info_frame() {
        let mp3 = encode(&tone(2.0, 44_100, 2)).unwrap();
        let marks = mp3.windows(4).filter(|w| w == b"Xing" || w == b"Info").count();
        assert_eq!(marks, 1);
        // 没被覆盖的占位帧里，帧头后面全是零
        assert!(mp3[36..40] == *b"Xing", "信息帧不在文件开头");
    }

    /// 编码参数要和 ffmpeg 的 `-acodec libmp3lame -q:a 5` 对得上：LAME 的 VBR（mtrh），质量 5。
    /// 这两样 LAME 自己记在信息帧里，直接读出来看——拿码率去猜不行，VBR 的码率跟着内容走
    /// （这个测试音只有 40 kbps，一首歌是 130 kbps 上下）。
    #[test]
    fn the_encoder_runs_at_vbr_quality_5() {
        let tag = info_tag(&encode(&tone(2.0, 44_100, 2)).unwrap()).unwrap();
        assert_eq!(tag.vbr_method, 4, "要是 mtrh VBR，不是 CBR / ABR");
        assert_eq!((100 - tag.quality) / 10, 5, "VBR 质量（Xing 质量字段 {}）", tag.quality);
    }

    #[test]
    fn more_than_two_channels_are_folded_to_stereo() {
        let six = Pcm { samples: [0.1, 0.2, 0.3, 0.4, 0.5, 0.6].repeat(22_050), channels: 6, rate: 44_100 };
        let stereo = to_stereo(&six);
        assert_eq!((stereo.channels, stereo.frames()), (2, 22_050));
        assert!((stereo.samples[0] - 0.3).abs() < 1e-6 && (stereo.samples[1] - 0.4).abs() < 1e-6, "{:?}", &stereo.samples[..2]);
        let tag = info_tag(&encode(&six).unwrap()).unwrap();
        assert_eq!((tag.header.channels, tag.samples()), (2, 22_050));
    }

    #[test]
    fn empty_audio_is_refused() {
        assert!(encode(&Pcm { samples: Vec::new(), channels: 2, rate: 44_100 }).is_err());
    }
}
