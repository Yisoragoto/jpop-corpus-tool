//! 限幅：把超出满幅的峰值压回去，响度不变、不削平。变调渲染在写 16 位文件之前用。
//!
//! # 为什么需要
//!
//! 变调后的浮点样本会超出 ±1.0，而且超得不少。流行歌的母带本来就压在 0 dB 附近，
//! 变调改变了各频率的相位关系，被压平的峰值重新冒出来。实测一首 4 分 17 秒的歌
//! 升 3 个半音（R3）：峰值 1.907（+5.6 dB），0.37% 的样本超幅，257 秒里有 178 秒出现过。
//! 转 16 位时有三种处理：
//!
//! - **硬削**（ffmpeg 以前就是这样）：超出的部分削平，响度不变，削过的地方有失真；
//! - **整首按峰值压低**：波形不变形，但整首轻了 5.6 dB——为了千分之四的样本，
//!   而且换调时音量会跳；
//! - **限幅**（这里）：只在峰值附近把增益平滑地压下去，过后再放回来。
//!
//! # 怎么做的
//!
//! 前视式限幅器，四步，全是逐帧的增益（各声道用同一个增益，声像不动）：
//!
//! 1. 每帧需要的增益 `t = min(1, 1 / 这一帧的峰值)`；
//! 2. 往后看 [`LOOKAHEAD_SEC`]：取这段里最小的 `t`，这样增益在峰值到来之前就开始降；
//! 3. 释放：增益下降立刻跟，回升按 [`RELEASE_SEC`] 的时间常数慢慢回，免得增益抖动；
//! 4. 再对过去 [`LOOKAHEAD_SEC`] 做滑动平均，把第 2 步的台阶抹成斜坡——增益突变本身就是咔哒声。
//!
//! 第 2 步保证了第 4 步平均的每一项都不大于当前帧需要的增益，所以**输出不会超过满幅**
//! （不是「大概不会」，测试里钉住了）。没超幅的音频原样返回，一个比特都不动。

use std::collections::VecDeque;

use crate::decode::Pcm;

/// 提前多久开始压。5 ms：比鼓点的起音短，听不出「提前变轻」；又够把增益的变化抹平
pub const LOOKAHEAD_SEC: f64 = 0.005;

/// 压完之后多久放回来（时间常数）
pub const RELEASE_SEC: f64 = 0.1;

/// 限幅做了什么。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limited {
    /// 处理之前的峰值（绝对值）。不大于 1.0 时什么都没做
    pub peak: f32,
    /// 增益被压低的帧数
    pub frames_reduced: usize,
}

/// 原地限幅。NaN 和无穷大先当成静音。
pub fn limit(pcm: &mut Pcm) -> Limited {
    let channels = usize::from(pcm.channels).max(1);
    for sample in &mut pcm.samples {
        if !sample.is_finite() {
            *sample = 0.0;
        }
    }
    let peak = pcm.samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
    if peak <= 1.0 {
        return Limited { peak, frames_reduced: 0 };
    }

    // 1. 每帧需要的增益
    let mut gain: Vec<f32> = pcm
        .samples
        .chunks(channels)
        .map(|frame| {
            let level = frame.iter().fold(0.0f32, |level, s| level.max(s.abs()));
            if level > 1.0 { 1.0 / level } else { 1.0 }
        })
        .collect();
    let frames = gain.len();
    let lookahead = ((LOOKAHEAD_SEC * f64::from(pcm.rate)).round() as usize).max(1);

    // 2. 往后 lookahead 帧里的最小值。单调队列：队头永远是窗口里最小的
    let mut window: VecDeque<(usize, f32)> = VecDeque::new();
    let mut next = 0;
    for i in 0..frames {
        while next < frames && next <= i + lookahead {
            let value = gain[next];
            while window.back().is_some_and(|(_, v)| *v >= value) {
                window.pop_back();
            }
            window.push_back((next, value));
            next += 1;
        }
        while window.front().is_some_and(|(index, _)| *index < i) {
            window.pop_front();
        }
        gain[i] = window.front().map_or(1.0, |(_, v)| *v);
    }

    // 3. 释放：降得快，回得慢。状态用 f64：f32 在接近 1.0 时每步的增量比它的精度还小，
    //    增益会卡在 0.9999 附近永远回不到 1.0
    let rise = 1.0 - (-1.0 / (RELEASE_SEC * f64::from(pcm.rate))).exp();
    let mut previous = 1.0f64;
    for value in &mut gain {
        let target = f64::from(*value);
        previous = if target > previous {
            let risen = previous + (target - previous) * rise;
            // 指数回升永远差一点点到不了头；差得够小就当到了，这样远离峰值的地方增益是精确的 1.0
            if target - risen > 1e-4 { risen } else { target }
        } else {
            target
        };
        *value = previous as f32;
    }

    // 4. 对过去 lookahead 帧做滑动平均
    let mut recent: VecDeque<f32> = VecDeque::with_capacity(lookahead + 2);
    let mut sum = 0.0f64;
    // 窗口里有几个不是 1.0 的
    let mut lowered = 0usize;
    let mut frames_reduced = 0;
    for (value, frame) in gain.iter().zip(pcm.samples.chunks_mut(channels)) {
        recent.push_back(*value);
        sum += f64::from(*value);
        lowered += usize::from(*value < 1.0);
        if recent.len() > lookahead + 1 {
            let gone = recent.pop_front().unwrap_or(1.0);
            sum -= f64::from(gone);
            lowered -= usize::from(gone < 1.0);
        }
        // 窗口里全是 1.0 时直接给 1.0：累加的浮点误差不该让没超幅的地方也变掉最后一位
        let smoothed = if lowered == 0 { 1.0 } else { (sum / recent.len() as f64) as f32 };
        if smoothed < 1.0 {
            frames_reduced += 1;
            for sample in frame {
                *sample *= smoothed;
            }
        }
    }
    Limited { peak, frames_reduced }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    fn mono(samples: Vec<f32>) -> Pcm {
        Pcm { samples, channels: 1, rate: RATE }
    }

    fn sine(amplitude: f32, hz: f64, frames: usize) -> Vec<f32> {
        (0..frames).map(|i| amplitude * (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(RATE)).sin() as f32).collect()
    }

    #[test]
    fn audio_within_range_is_not_touched() {
        let original = sine(1.0, 440.0, 20_000);
        let mut pcm = mono(original.clone());
        let limited = limit(&mut pcm);
        assert_eq!(limited.frames_reduced, 0);
        assert!(limited.peak <= 1.0);
        assert!(pcm.samples == original, "没超幅的音频一个比特都不该动");
    }

    /// 一段正常音量的音里夹着一阵 +4 dB 的：输出不超过满幅，没被削平，
    /// 离那一阵远的地方原样不动。
    #[test]
    fn a_loud_burst_is_pulled_under_full_scale_without_flattening() {
        let mut samples = sine(0.5, 440.0, RATE as usize * 3);
        let burst = RATE as usize..RATE as usize + 4_410; // 第 1 秒起的 0.1 秒
        for i in burst.clone() {
            samples[i] *= 3.2; // 0.5 × 3.2 = 1.6，+4.1 dB
        }
        let original = samples.clone();
        let mut pcm = mono(samples);
        let limited = limit(&mut pcm);

        assert!((limited.peak - 1.6).abs() < 0.01, "{limited:?}");
        let after = pcm.samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
        assert!(after <= 1.0, "限幅之后峰值还有 {after}");
        assert!(after > 0.95, "峰值被压到 {after}，压过头了");

        // 硬削的话，幅度 1.6 的正弦有 57% 的时间在满幅以上，这 4410 个样本里约 2500 个会贴在满幅上；
        // 限幅之后只有每个半波的最高那一个样本可能刚好碰到
        let pinned = pcm.samples[burst.clone()].iter().filter(|s| s.abs() >= 0.9999).count();
        assert!(pinned < 200, "{pinned} 个样本贴在满幅上，这是削平不是限幅");

        // 开头半秒离得远：原样。那一阵在 1.1 秒结束，释放要 0.83 秒，2.2 秒以后也回到原样
        assert!(pcm.samples[..RATE as usize / 2] == original[..RATE as usize / 2], "开头半秒被动过");
        let tail = RATE as usize * 22 / 10;
        assert!(pcm.samples[tail..] == original[tail..], "2.2 秒以后还没放回来");
        assert!(limited.frames_reduced > 4_410 && limited.frames_reduced < RATE as usize, "{limited:?}");
    }

    /// 增益本身要平滑：恒定的 0.5 里夹一个尖峰，输出除以 0.5 就是增益曲线。
    /// 峰值到来之前就开始降，峰值那一帧降够，相邻两帧的增益差很小（没有台阶）。
    #[test]
    fn the_gain_ramps_down_before_the_peak_and_never_jumps() {
        let spike = 30_000;
        let mut samples = vec![0.5f32; 90_000];
        samples[spike] = 2.0;
        let mut pcm = mono(samples);
        limit(&mut pcm);

        let lookahead = (LOOKAHEAD_SEC * f64::from(RATE)).round() as usize;
        assert!(pcm.samples[spike] <= 1.0 && pcm.samples[spike] > 0.99, "尖峰本身：{}", pcm.samples[spike]);
        let gain_at = |i: usize| if i == spike { pcm.samples[i] / 2.0 } else { pcm.samples[i] / 0.5 };
        assert_eq!(gain_at(spike - lookahead - 2), 1.0, "还没到前视范围：不该动");
        assert!(gain_at(spike - lookahead / 2) < 0.9, "峰值到来之前就该在降了：{}", gain_at(spike - lookahead / 2));
        assert!(gain_at(spike + lookahead) < 0.53, "尖峰刚过，才开始往回放：{}", gain_at(spike + lookahead));
        assert_eq!(gain_at(89_999), 1.0, "过了 1.36 秒该完全放回来了");

        let mut worst = 0.0f32;
        for i in 1..90_000 {
            worst = worst.max((gain_at(i) - gain_at(i - 1)).abs());
        }
        // 从 1.0 降到 0.5 摊在 221 帧上，每帧约 0.0023
        assert!(worst < 0.004, "相邻两帧的增益差了 {worst}，会听到咔哒声");
    }

    #[test]
    fn stereo_channels_share_one_gain_so_the_image_stays_put() {
        // 左声道超幅、右声道没有：两边要按同一个增益压，比例不变
        let mut samples = Vec::new();
        for value in sine(1.5, 300.0, 8_000) {
            samples.extend([value, value * 0.5]);
        }
        let mut pcm = Pcm { samples, channels: 2, rate: RATE };
        limit(&mut pcm);
        for frame in pcm.samples.as_chunks::<2>().0 {
            assert!(frame[0].abs() <= 1.0);
            assert!((frame[1] - frame[0] * 0.5).abs() < 1e-6, "{frame:?}");
        }
    }

    #[test]
    fn nonsense_samples_become_silence_and_do_not_crush_the_rest() {
        let mut pcm = mono(vec![0.5, f32::NAN, -0.75, f32::INFINITY, 0.25, f32::NEG_INFINITY]);
        let limited = limit(&mut pcm);
        assert_eq!(pcm.samples, [0.5, 0.0, -0.75, 0.0, 0.25, 0.0]);
        assert_eq!(limited, Limited { peak: 0.75, frames_reduced: 0 });
    }
}
