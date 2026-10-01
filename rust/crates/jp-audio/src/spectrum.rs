//! 频谱：采样窗 → 加窗 → 实数 FFT → 对数刻度的频段能量。
//!
//! 纯计算，不碰音频设备，所以能完整单测——这正是把它单独拆出来的理由。
//!
//! 输出是**对数频段**而不是原始 bin：线性 bin 画出来低频挤成一团、
//! 高频一片空白，看着像心电图。人耳是对数的，频段也该是。

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

/// 默认窗长。1024 @ 44.1kHz ≈ 23ms，兼顾频率分辨率和响应速度。
pub const DEFAULT_WINDOW: usize = 1024;

/// 默认频段数。够画出形状，又不至于让前端画上百个矩形。
pub const DEFAULT_BANDS: usize = 48;

/// 低于这个能量视为静音，避免对 0 取对数。
const FLOOR_DB: f32 = -70.0;

pub struct SpectrumAnalyzer {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    scratch_in: Vec<f32>,
    scratch_out: Vec<realfft::num_complex::Complex<f32>>,
    bands: usize,
    /// 每个频段覆盖的 bin 区间，构造时算好，每帧直接用
    band_ranges: Vec<(usize, usize)>,
}

impl SpectrumAnalyzer {
    pub fn new(window_len: usize, bands: usize, sample_rate: u32) -> Self {
        let window_len = window_len.next_power_of_two().max(64);
        let bands = bands.clamp(4, window_len / 2);
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(window_len);
        let scratch_out = fft.make_output_vec();

        Self {
            window: hann(window_len),
            scratch_in: vec![0.0; window_len],
            scratch_out,
            band_ranges: log_bands(window_len / 2, bands, sample_rate),
            bands,
            fft,
        }
    }

    pub fn window_len(&self) -> usize {
        self.window.len()
    }

    pub fn bands(&self) -> usize {
        self.bands
    }

    /// 算一帧频谱。返回 `bands` 个 0~1 的值。
    ///
    /// 输入长度不足时前面补零；多了取最后一窗（最新的那部分）。
    pub fn analyze(&mut self, samples: &[f32]) -> Vec<f32> {
        let n = self.window.len();
        self.scratch_in.fill(0.0);
        let take = samples.len().min(n);
        let src = &samples[samples.len() - take..];
        // 补零补在前面，让最新的样本落在窗口右端
        self.scratch_in[n - take..].copy_from_slice(src);
        for (sample, w) in self.scratch_in.iter_mut().zip(self.window.iter()) {
            *sample *= w;
        }

        if self
            .fft
            .process(&mut self.scratch_in, &mut self.scratch_out)
            .is_err()
        {
            return vec![0.0; self.bands];
        }

        // 幅度归一化：FFT 的输出随窗长线性增长，除掉才能和窗长无关
        let scale = 2.0 / n as f32;
        self.band_ranges
            .iter()
            .map(|&(lo, hi)| {
                let slice = &self.scratch_out[lo..hi.max(lo + 1).min(self.scratch_out.len())];
                if slice.is_empty() {
                    return 0.0;
                }
                // 取区间内的峰值而不是均值：均值会把尖锐的谐波抹平，
                // 画出来是一条没有细节的包络
                let peak = slice
                    .iter()
                    .map(|c| c.norm() * scale)
                    .fold(0.0f32, f32::max);
                normalise_db(peak)
            })
            .collect()
    }
}

/// 幅度 → 0~1。先转 dB 再线性映射到 [FLOOR_DB, 0]。
fn normalise_db(magnitude: f32) -> f32 {
    if magnitude <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * magnitude.log10();
    ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

/// Hann 窗。不加窗的话每一帧的首尾突变会产生大量假的高频。
fn hann(len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| {
            let x = std::f32::consts::PI * 2.0 * i as f32 / len as f32;
            0.5 * (1.0 - x.cos())
        })
        .collect()
}

/// 把 bin 按对数频率分成 `bands` 段。
///
/// 起点取 30Hz（再低听不见也画不出），终点取奈奎斯特频率。
fn log_bands(bin_count: usize, bands: usize, sample_rate: u32) -> Vec<(usize, usize)> {
    let nyquist = (sample_rate as f32 / 2.0).max(1.0);
    let hz_per_bin = nyquist / bin_count.max(1) as f32;
    let low_hz = 30.0f32;
    let high_hz = nyquist;

    let mut ranges = Vec::with_capacity(bands);
    let mut prev_bin = (low_hz / hz_per_bin) as usize;
    for i in 1..=bands {
        let t = i as f32 / bands as f32;
        // 对数插值：low * (high/low)^t
        let hz = low_hz * (high_hz / low_hz).powf(t);
        let bin = ((hz / hz_per_bin) as usize).min(bin_count);
        // 每段至少占一个 bin，否则低频几段会全空
        let end = bin.max(prev_bin + 1).min(bin_count);
        ranges.push((prev_bin.min(bin_count), end));
        prev_bin = end;
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    fn sine(freq: f32, len: usize, rate: u32) -> Vec<f32> {
        (0..len)
            .map(|i| {
                (std::f32::consts::PI * 2.0 * freq * i as f32 / rate as f32).sin()
            })
            .collect()
    }

    fn peak_band(spectrum: &[f32]) -> usize {
        spectrum
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap()
    }

    #[test]
    fn window_and_band_counts_are_sane() {
        let a = SpectrumAnalyzer::new(1000, 48, RATE);
        assert_eq!(a.window_len(), 1024);
        assert_eq!(a.bands(), 48);
        // 频段数不能超过 bin 数
        let b = SpectrumAnalyzer::new(64, 999, RATE);
        assert!(b.bands() <= 32);
    }

    #[test]
    fn silence_produces_no_energy() {
        let mut a = SpectrumAnalyzer::new(DEFAULT_WINDOW, DEFAULT_BANDS, RATE);
        let spectrum = a.analyze(&vec![0.0; DEFAULT_WINDOW]);
        assert_eq!(spectrum.len(), DEFAULT_BANDS);
        assert!(spectrum.iter().all(|&v| v == 0.0), "静音不该有能量");
    }

    #[test]
    fn output_is_always_in_unit_range() {
        let mut a = SpectrumAnalyzer::new(DEFAULT_WINDOW, DEFAULT_BANDS, RATE);
        // 削顶到 ±1 之外也不该跑出 [0,1]
        let loud: Vec<f32> = sine(440.0, DEFAULT_WINDOW, RATE).iter().map(|s| s * 50.0).collect();
        for v in a.analyze(&loud) {
            assert!((0.0..=1.0).contains(&v), "越界: {v}");
        }
    }

    #[test]
    fn a_low_tone_lands_in_a_lower_band_than_a_high_tone() {
        let mut a = SpectrumAnalyzer::new(DEFAULT_WINDOW, DEFAULT_BANDS, RATE);
        let low = peak_band(&a.analyze(&sine(200.0, DEFAULT_WINDOW, RATE)));
        let high = peak_band(&a.analyze(&sine(4000.0, DEFAULT_WINDOW, RATE)));
        assert!(low < high, "200Hz 应当落在比 4kHz 更低的频段 ({low} vs {high})");
    }

    #[test]
    fn louder_input_yields_higher_energy() {
        let mut a = SpectrumAnalyzer::new(DEFAULT_WINDOW, DEFAULT_BANDS, RATE);
        let quiet: Vec<f32> = sine(1000.0, DEFAULT_WINDOW, RATE).iter().map(|s| s * 0.01).collect();
        let loud = sine(1000.0, DEFAULT_WINDOW, RATE);
        let q = a.analyze(&quiet).iter().cloned().fold(0.0f32, f32::max);
        let l = a.analyze(&loud).iter().cloned().fold(0.0f32, f32::max);
        assert!(l > q, "响的应当能量更高: {l} vs {q}");
    }

    #[test]
    fn short_input_is_zero_padded_not_rejected() {
        let mut a = SpectrumAnalyzer::new(DEFAULT_WINDOW, DEFAULT_BANDS, RATE);
        let spectrum = a.analyze(&sine(1000.0, 100, RATE));
        assert_eq!(spectrum.len(), DEFAULT_BANDS);
    }

    #[test]
    fn empty_input_does_not_panic() {
        let mut a = SpectrumAnalyzer::new(DEFAULT_WINDOW, DEFAULT_BANDS, RATE);
        assert_eq!(a.analyze(&[]).len(), DEFAULT_BANDS);
    }

    #[test]
    fn long_input_uses_the_newest_window() {
        let mut a = SpectrumAnalyzer::new(256, 16, RATE);
        // 前半段静音、后半段是音——应当测到能量，说明取的是最新那一窗
        let mut samples = vec![0.0; 2048];
        samples.extend(sine(1000.0, 256, RATE));
        let spectrum = a.analyze(&samples);
        assert!(spectrum.iter().any(|&v| v > 0.1), "应当取到尾部的信号");
    }

    #[test]
    fn bands_are_contiguous_and_non_overlapping() {
        let ranges = log_bands(512, 32, RATE);
        assert_eq!(ranges.len(), 32);
        for pair in ranges.windows(2) {
            assert_eq!(pair[0].1, pair[1].0, "频段之间不该有空隙或重叠");
        }
        for &(lo, hi) in &ranges {
            assert!(lo < hi, "每段至少要占一个 bin");
        }
    }

    #[test]
    fn hann_window_tapers_to_zero_at_the_edges() {
        let w = hann(64);
        assert!(w[0].abs() < 1e-6);
        // 峰值在中间
        assert!(w[32] > 0.99);
    }

    #[test]
    fn analyzer_is_reusable_across_frames() {
        let mut a = SpectrumAnalyzer::new(256, 16, RATE);
        let tone = sine(1000.0, 256, RATE);
        let first = a.analyze(&tone);
        let second = a.analyze(&tone);
        assert_eq!(first, second, "同样的输入应当得到同样的输出");
    }
}
