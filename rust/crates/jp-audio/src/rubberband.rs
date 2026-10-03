//! Rubber Band 的最小封装：整首歌离线变调，不改时长。
//!
//! 源码在仓库根的 `third_party/rubberband/`，`build.rs` 用它的单文件构建编进来。
//! 这里只碰 C 接口（`rubberband/rubberband-c.h`）里用得到的那几个函数，声明是手写的。
//!
//! **只用离线模式**：先把整首歌过一遍（study），再处理。离线模式下起始延迟由库自己补，
//! 输出和输入逐样本对齐，歌词时间轴照用——实时模式要自己量延迟、自己裁，
//! 评估时的原型就因为这个早了 29 ms。

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, bail};

use crate::decode::Pcm;

unsafe extern "C" {
    fn rubberband_new(rate: u32, channels: u32, options: i32, time_ratio: f64, pitch_scale: f64) -> *mut c_void;
    fn rubberband_delete(state: *mut c_void);
    fn rubberband_get_engine_version(state: *mut c_void) -> i32;
    fn rubberband_set_expected_input_duration(state: *mut c_void, samples: u32);
    fn rubberband_study(state: *mut c_void, input: *const *const f32, samples: u32, last: i32);
    fn rubberband_process(state: *mut c_void, input: *const *const f32, samples: u32, last: i32);
    fn rubberband_available(state: *const c_void) -> i32;
    fn rubberband_retrieve(state: *const c_void, output: *const *mut f32, samples: u32) -> u32;
}

// `RubberBandOption` 里用到的两个取值；其余选项都用默认（0）
const OPTION_PROCESS_OFFLINE: i32 = 0x0000_0000;
const OPTION_ENGINE_FINER: i32 = 0x2000_0000;

/// 一次喂给库多少帧。也是检查取消标志的间隔：44.1 kHz 下不到 0.1 秒的音频
const BLOCK: usize = 4096;

/// Rubber Band 的两个引擎。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// R2（"Faster"）。ffmpeg 的 `rubberband` 滤镜用的就是它
    R2,
    /// R3（"Finer"）。上游推荐的高质量引擎，慢三到四倍
    R3,
}

impl Engine {
    /// 写进缓存文件名里的短名
    pub fn tag(self) -> &'static str {
        match self {
            Engine::R2 => "r2",
            Engine::R3 => "r3",
        }
    }

    fn options(self) -> i32 {
        match self {
            Engine::R2 => OPTION_PROCESS_OFFLINE,
            Engine::R3 => OPTION_PROCESS_OFFLINE | OPTION_ENGINE_FINER,
        }
    }

    fn version(self) -> i32 {
        match self {
            Engine::R2 => 2,
            Engine::R3 => 3,
        }
    }
}

/// 编进来的 Rubber Band 版本（`build.rs` 从上游头文件里读的）
pub const LIBRARY_VERSION: &str = env!("RUBBERBAND_VERSION");

/// 库的状态句柄。只为了保证每条退出路径（包括取消、出错）都会释放。
struct State(*mut c_void);

impl Drop for State {
    fn drop(&mut self) {
        // SAFETY: 指针来自 rubberband_new，只在这里释放一次
        unsafe { rubberband_delete(self.0) }
    }
}

/// 把整段音频升降 `semitones` 个半音，**帧数不变**。
///
/// `cancel` 置位后在下一块（[`BLOCK`] 帧）停下，返回 `Ok(None)`。
pub fn shift_pitch(pcm: &Pcm, semitones: f64, engine: Engine, cancel: &AtomicBool) -> Result<Option<Pcm>> {
    let channels = usize::from(pcm.channels);
    let frames = pcm.frames();
    if channels == 0 || pcm.rate == 0 {
        bail!("音频没有声道或采样率（{} 声道，{} Hz）", pcm.channels, pcm.rate);
    }
    // C 接口里时长是 unsigned int。44.1 kHz 下 2^32 帧是 27 小时，真碰上了宁可报错也不要截断
    let Ok(total) = u32::try_from(frames) else {
        bail!("音频太长，变调不了（{frames} 帧）");
    };
    if frames == 0 {
        return Ok(Some(Pcm { samples: Vec::new(), channels: pcm.channels, rate: pcm.rate }));
    }
    let scale = 2f64.powf(semitones / 12.0);

    // SAFETY: 参数都是普通数值；返回空指针时下面报错
    let state = State(unsafe { rubberband_new(pcm.rate, channels as u32, engine.options(), 1.0, scale) });
    if state.0.is_null() {
        bail!("Rubber Band 初始化失败（{} 声道，{} Hz）", pcm.channels, pcm.rate);
    }
    // 选项位写错的话库会悄悄退回另一个引擎，缓存名却照样写着这一个——所以问一句
    // SAFETY: state 有效
    let actual = unsafe { rubberband_get_engine_version(state.0) };
    if actual != engine.version() {
        bail!("Rubber Band 用的是 R{actual} 引擎，要的是 R{}", engine.version());
    }
    // SAFETY: state 有效
    unsafe { rubberband_set_expected_input_duration(state.0, total) };

    // 库要的是「每个声道一条」的平面数据，这边是交织的：一次只拆一块，不为整首歌再多占一份内存
    let mut planar: Vec<Vec<f32>> = vec![vec![0.0; BLOCK]; channels];
    let fill = |planar: &mut [Vec<f32>], start: usize, count: usize| {
        let block = &pcm.samples[start * channels..(start + count) * channels];
        for (c, plane) in planar.iter_mut().enumerate() {
            for (dst, frame) in plane[..count].iter_mut().zip(block.chunks_exact(channels)) {
                *dst = frame[c];
            }
        }
    };

    // 第一遍：study
    let mut pos = 0;
    while pos < frames {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let count = BLOCK.min(frames - pos);
        fill(&mut planar, pos, count);
        let pointers: Vec<*const f32> = planar.iter().map(|plane| plane.as_ptr()).collect();
        // SAFETY: 每个声道的缓冲区至少有 count 个样本，调用期间不会被挪动
        unsafe { rubberband_study(state.0, pointers.as_ptr(), count as u32, i32::from(pos + count == frames)) };
        pos += count;
    }

    // 第二遍：process，边喂边取
    let mut out: Vec<f32> = Vec::with_capacity(pcm.samples.len());
    let mut scratch: Vec<Vec<f32>> = vec![vec![0.0; BLOCK]; channels];
    let mut drain = |out: &mut Vec<f32>| {
        loop {
            // SAFETY: state 有效
            let available = unsafe { rubberband_available(state.0) };
            if available <= 0 {
                break;
            }
            let want = (available as usize).min(BLOCK);
            let pointers: Vec<*mut f32> = scratch.iter_mut().map(|plane| plane.as_mut_ptr()).collect();
            // SAFETY: 每个声道的缓冲区有 BLOCK 个位置，want 不超过 BLOCK
            let got = unsafe { rubberband_retrieve(state.0, pointers.as_ptr(), want as u32) } as usize;
            if got == 0 {
                break;
            }
            for i in 0..got {
                for plane in scratch.iter() {
                    out.push(plane[i]);
                }
            }
        }
    };
    let mut pos = 0;
    while pos < frames {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let count = BLOCK.min(frames - pos);
        fill(&mut planar, pos, count);
        let pointers: Vec<*const f32> = planar.iter().map(|plane| plane.as_ptr()).collect();
        // SAFETY: 同 study
        unsafe { rubberband_process(state.0, pointers.as_ptr(), count as u32, i32::from(pos + count == frames)) };
        pos += count;
        drain(&mut out);
    }
    drain(&mut out);

    // 时间比是 1，离线模式给出的帧数就该等于输入。差的那一点（实测是 0）补静音或裁掉：
    // 调用方靠「变调前后一样长」来沿用歌词时间轴和播放位置
    out.resize(pcm.samples.len(), 0.0);
    Ok(Some(Pcm { samples: out, channels: pcm.channels, rate: pcm.rate }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f64, seconds: f64, rate: u32, channels: u16) -> Pcm {
        let frames = (seconds * f64::from(rate)) as usize;
        let mut samples = Vec::with_capacity(frames * usize::from(channels));
        for i in 0..frames {
            let value = (0.5 * (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(rate)).sin()) as f32;
            for _ in 0..channels {
                samples.push(value);
            }
        }
        Pcm { samples, channels, rate }
    }

    /// 数过零点估频率：够判断「升了几个半音」，不用为测试拉一个 FFT 进来
    fn frequency(pcm: &Pcm, from_sec: f64, to_sec: f64) -> f64 {
        let channels = usize::from(pcm.channels);
        let (a, b) = ((from_sec * f64::from(pcm.rate)) as usize, (to_sec * f64::from(pcm.rate)) as usize);
        let mono: Vec<f32> = pcm.samples.chunks_exact(channels).skip(a).take(b - a).map(|f| f[0]).collect();
        let crossings = mono.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        crossings as f64 / (to_sec - from_sec)
    }

    /// 两个引擎都要：音高按半音数走，帧数、声道、采样率一个不变。
    #[test]
    fn both_engines_shift_the_pitch_and_keep_the_length() {
        let source = sine(440.0, 3.0, 44_100, 2);
        for engine in [Engine::R2, Engine::R3] {
            for semitones in [-6.0, 3.0, 6.0] {
                let shifted = shift_pitch(&source, semitones, engine, &AtomicBool::new(false)).unwrap().unwrap();
                assert_eq!(shifted.samples.len(), source.samples.len(), "{engine:?} {semitones:+}");
                assert_eq!((shifted.channels, shifted.rate), (2, 44_100));
                let expected = 440.0 * 2f64.powf(semitones / 12.0);
                let measured = frequency(&shifted, 1.0, 2.0);
                assert!((measured - expected).abs() < expected * 0.01, "{engine:?} {semitones:+}：{measured} Hz，应为 {expected} Hz");
            }
        }
    }

    #[test]
    fn a_set_cancel_flag_stops_the_render() {
        let source = sine(440.0, 1.0, 44_100, 1);
        assert!(shift_pitch(&source, 2.0, Engine::R3, &AtomicBool::new(true)).unwrap().is_none());
    }

    #[test]
    fn empty_audio_is_passed_through() {
        let empty = Pcm { samples: Vec::new(), channels: 2, rate: 44_100 };
        let out = shift_pitch(&empty, 2.0, Engine::R2, &AtomicBool::new(false)).unwrap().unwrap();
        assert!(out.samples.is_empty());
    }

    #[test]
    fn the_library_version_comes_from_the_vendored_header() {
        // build.rs 读的是 third_party/rubberband/rubberband/rubberband-c.h
        assert!(LIBRARY_VERSION.split('.').count() == 3 && LIBRARY_VERSION.chars().all(|c| c.is_ascii_digit() || c == '.'), "{LIBRARY_VERSION}");
    }
}
