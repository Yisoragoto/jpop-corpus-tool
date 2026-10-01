//! 播放状态。**这一层不认识歌词。**
//!
//! 需求书第十四条列的状态里有 `current_line`，但那不该由音频引擎持有——
//! 当前行是「位置 + 歌词时间轴」推导出来的，引擎只负责位置。
//! 把它塞进来会让引擎依赖歌词，正是要求书要拆开的那种耦合。
//!
//! 歌词层订阅 `PlaybackState.position_sec` 自己算当前行。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlayState {
    /// 没有加载任何音频
    Empty,
    Playing,
    Paused,
    /// 播完了，位置停在末尾
    Ended,
}

/// A-B 循环区间。单句循环就是把它设成那一句的起止。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopRegion {
    pub start_sec: f64,
    pub end_sec: f64,
}

impl LoopRegion {
    /// 起止写反了就交换，而不是拒绝——UI 上从后往前拖是很自然的操作。
    pub fn new(a: f64, b: f64) -> Option<Self> {
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        // 太短的区间循环起来只会是咔哒声，不如不设
        (end - start >= MIN_LOOP_SEC).then_some(Self {
            start_sec: start.max(0.0),
            end_sec: end,
        })
    }

    pub fn contains(&self, pos: f64) -> bool {
        pos >= self.start_sec && pos < self.end_sec
    }

    pub fn duration_sec(&self) -> f64 {
        self.end_sec - self.start_sec
    }
}

/// 比这更短的循环区间没有意义。J-Pop 最短的一句也在 0.5 秒以上。
pub const MIN_LOOP_SEC: f64 = 0.15;

/// 倍速的合理范围。低于 0.5 时 WSOLA 的金属感很明显，
/// 高于 2.0 对听写没有帮助。和 Python 版的 PlaybackSpeedControl 对齐。
pub const MIN_RATE: f32 = 0.5;
pub const MAX_RATE: f32 = 2.0;

pub fn clamp_rate(rate: f32) -> f32 {
    if rate.is_finite() {
        rate.clamp(MIN_RATE, MAX_RATE)
    } else {
        1.0
    }
}

pub fn clamp_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// 一次状态快照。前端轮询这个，而不是自己持有播放器。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackState {
    /// 当前加载的曲目。空串表示没加载。
    pub song_id: String,
    pub play_state: PlayState,
    pub position_sec: f64,
    /// 解码器给不出时长的格式（某些流式 mp3）会是 None，UI 要能降级
    pub duration_sec: Option<f64>,
    pub rate: f32,
    pub volume: f32,
    pub loop_region: Option<LoopRegion>,
    /// 变调半音数。引擎本身不变调（播的是渲染好的文件），这三个字段由应用层填，见 jp-app 的 `pitch.rs`
    pub pitch_semitones: i32,
    /// 正在渲染变调音频
    pub pitch_rendering: bool,
    /// 最近一次变调失败。`id` 变了前端才提示
    pub pitch_error: Option<PitchError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PitchError {
    pub id: u64,
    pub message: String,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            song_id: String::new(),
            play_state: PlayState::Empty,
            position_sec: 0.0,
            duration_sec: None,
            rate: 1.0,
            volume: 1.0,
            loop_region: None,
            pitch_semitones: 0,
            pitch_rendering: false,
            pitch_error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_region_normalises_reversed_input() {
        let region = LoopRegion::new(30.0, 10.0).expect("应当接受反向输入");
        assert_eq!(region.start_sec, 10.0);
        assert_eq!(region.end_sec, 30.0);
    }

    #[test]
    fn loop_region_rejects_degenerate_spans() {
        assert!(LoopRegion::new(10.0, 10.0).is_none());
        assert!(LoopRegion::new(10.0, 10.05).is_none());
        assert!(LoopRegion::new(10.0, 10.2).is_some());
    }

    #[test]
    fn loop_region_clamps_negative_start() {
        let region = LoopRegion::new(-5.0, 3.0).unwrap();
        assert_eq!(region.start_sec, 0.0);
    }

    #[test]
    fn loop_contains_is_half_open() {
        let region = LoopRegion::new(10.0, 20.0).unwrap();
        assert!(region.contains(10.0));
        assert!(region.contains(19.999));
        // 右端开区间：到了终点就该跳回去，不该算「还在区间内」
        assert!(!region.contains(20.0));
    }

    #[test]
    fn rate_is_clamped_to_a_useful_range() {
        assert_eq!(clamp_rate(0.1), MIN_RATE);
        assert_eq!(clamp_rate(9.0), MAX_RATE);
        assert_eq!(clamp_rate(1.25), 1.25);
    }

    #[test]
    fn non_finite_values_fall_back_instead_of_poisoning_playback() {
        assert_eq!(clamp_rate(f32::NAN), 1.0);
        assert_eq!(clamp_rate(f32::INFINITY), 1.0);
        assert_eq!(clamp_volume(f32::NAN), 1.0);
    }

    #[test]
    fn volume_is_clamped() {
        assert_eq!(clamp_volume(-1.0), 0.0);
        assert_eq!(clamp_volume(2.0), 1.0);
    }

    #[test]
    fn state_serialises_as_camel_case() {
        let json = serde_json::to_value(PlaybackState::default()).unwrap();
        for key in ["songId", "playState", "positionSec", "durationSec", "loopRegion"] {
            assert!(json.get(key).is_some(), "缺字段 {key}");
        }
        assert!(json.get("position_sec").is_none(), "不该泄漏 snake_case");
    }

    #[test]
    fn default_state_is_empty_not_playing() {
        assert_eq!(PlaybackState::default().play_state, PlayState::Empty);
    }
}
