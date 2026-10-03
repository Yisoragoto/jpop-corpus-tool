//! JPOP Corpus Tool 的音频引擎。
//!
//! 分三层，只有 `engine` 碰音频设备：
//!
//! | 模块 | 职责 | 能否离线测试 |
//! |---|---|---|
//! | `state` | 播放状态、倍速/音量钳制、循环区间 | ✅ |
//! | `tap` | 采样环形缓冲 + Source 包装 | ✅ |
//! | `spectrum` | 加窗 + 实数 FFT + 对数频段 | ✅ |
//! | `probe` | 读时长，不播放、不占设备 | ✅ |
//! | `decode` | 整首 / 按时间段解码成 PCM，和播放同一个解码器 | ✅ |
//! | `rubberband` | Rubber Band（`third_party/`）的封装：离线变调 | ✅ |
//! | `limiter` | 前视限幅：变调后超出满幅的峰值压回去，不削平 | ✅ |
//! | `wav` | 写 16 位 WAV | ✅ |
//! | `pitch` | 变调的渲染、缓存命名、缓存上限 | ✅ |
//! | `engine` | rodio 播放链、循环看门狗 | 需要声卡，无卡自动跳过 |
//!
//! **这一层不认识歌词。** 当前行由「位置 + 歌词时间轴」推导，
//! 是歌词层的职责。见 `state.rs` 的说明。
//!
//! **变速不变调**：走 WSOLA 时间伸缩，而不是 rodio 自带的 `set_speed`
//! （那个是重采样，会把音高一起改掉）。见 `engine.rs`。
//!
//! **变调**：Rubber Band 离线渲染整首、缓存成 WAV 再播，见 `pitch.rs`。库是编进来的，不需要 ffmpeg。
//! 引擎本身不变调，只播文件；什么时候渲染、换哪个文件由应用层决定。

pub mod decode;
pub mod engine;
pub mod limiter;
pub mod pitch;
pub mod probe;
pub mod rubberband;
pub mod spectrum;
mod sha1;
pub mod state;
pub mod tap;
pub mod wav;

pub use decode::{Pcm, decode_all, decode_window};
pub use engine::AudioEngine;
pub use probe::{ProbeOutcome, probe, probe_duration};
pub use spectrum::{DEFAULT_BANDS, DEFAULT_WINDOW, SpectrumAnalyzer};
pub use state::{LoopRegion, PitchError, PlayState, PlaybackState};
pub use tap::TapBuffer;
