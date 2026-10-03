//! 收听会话统计。
//!
//! `play_history` 表和 `recently_played` / `most_played` 查询早就有了，
//! 但**从来没有人往表里写**——Home 页要的数据链路缺的就是这一环。
//!
//! 这里只做纯逻辑：喂进播放状态快照，吐出该落库的事件。
//! 不碰数据库、不碰 Tauri、不开线程，所以能完整单测。
//! 驱动它的是 `AppState::tick`（算墙钟间隔、喂快照、有事件就写库），
//! 而 `tick` 由前端每 100ms 一次的 `audio_tick` 轮询调用。
//!
//! ## 「听了多久」按墙上时钟算，不按播放位置算
//!
//! 两者在三种情况下会分道扬镳：
//!
//! * **单句循环**：位置在原地打转，但人确实在反复听 —— 墙钟才是对的
//! * **变速**：2 倍速下 1 秒墙钟 = 2 秒音频 —— 注意力花了 1 秒，算 1 秒
//! * **拖动进度条**：位置瞬间跳几分钟，但没人听 —— 墙钟不会被骗
//!
//! 墙钟衡量的是「花了多少注意力」，这正是收听统计想回答的问题。

use jp_audio::{PlayState, PlaybackState};
use jp_corpus::PlayEvent;

/// 听够这么久才记一条。点开就切走不该进历史，否则「最近播放」
/// 会被一串误触淹没。
pub const MIN_LISTENED_SEC: f64 = 5.0;

/// 播到这个比例就算听完。留 5% 余量是因为很多歌结尾有淡出和静音，
/// 要求 100% 会让几乎没有一首算「听完」。
pub const COMPLETION_RATIO: f64 = 0.95;

/// 两次采样间隔超过这么久就不计入——说明中间休眠或卡住了，
/// 把整段空档算成「在听」会严重虚高。
const MAX_SAMPLE_GAP_SEC: f64 = 2.0;

#[derive(Debug, Clone)]
struct Session {
    song_id: String,
    listened_sec: f64,
    last_position: f64,
    duration_sec: Option<f64>,
    /// 见过的最大位置。用它判断有没有播到结尾——
    /// 用当前位置会被结尾处的跳转骗过去。
    furthest: f64,
    source: String,
}

impl Session {
    fn new(state: &PlaybackState, source: &str) -> Self {
        Self {
            song_id: state.song_id.clone(),
            listened_sec: 0.0,
            last_position: state.position_sec,
            duration_sec: state.duration_sec,
            furthest: state.position_sec,
            source: source.to_string(),
        }
    }

    fn completed(&self) -> bool {
        match self.duration_sec {
            Some(duration) if duration > 0.0 => self.furthest >= duration * COMPLETION_RATIO,
            // 时长未知时不敢说「听完了」，宁可少记
            _ => false,
        }
    }

    /// 够格落库吗。
    fn worth_recording(&self) -> bool {
        self.listened_sec >= MIN_LISTENED_SEC
    }

    fn into_event(self) -> PlayEvent {
        // completed() 借用 self，得在移动字段之前算好
        let completed = self.completed();
        PlayEvent {
            song_id: self.song_id,
            listened_sec: self.listened_sec,
            position_sec: self.last_position,
            completed,
            source: self.source,
        }
    }
}

/// 观察播放状态，在合适的时机吐出该落库的事件。
#[derive(Debug, Default)]
pub struct PlayTracker {
    current: Option<Session>,
    source: String,
}

impl PlayTracker {
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            current: None,
            source: source.into(),
        }
    }

    /// 喂一个状态快照。`elapsed_sec` 是距上次调用的墙钟秒数。
    ///
    /// 返回 `Some(event)` 表示有一段收听结束了、该落库。
    pub fn observe(&mut self, state: &PlaybackState, elapsed_sec: f64) -> Option<PlayEvent> {
        // 换歌 / 停止：先把上一段结掉
        let switched = self
            .current
            .as_ref()
            .is_some_and(|s| s.song_id != state.song_id);
        let finished = matches!(state.play_state, PlayState::Empty | PlayState::Ended);

        let mut emitted = None;
        if switched || (finished && self.current.is_some()) {
            emitted = self.take_current();
        }

        match state.play_state {
            PlayState::Playing => {
                let session = self
                    .current
                    .get_or_insert_with(|| Session::new(state, &self.source));
                // 采样间隔异常时只更新位置，不累加时长
                if (0.0..=MAX_SAMPLE_GAP_SEC).contains(&elapsed_sec) {
                    session.listened_sec += elapsed_sec;
                }
                session.last_position = state.position_sec;
                session.furthest = session.furthest.max(state.position_sec);
                session.duration_sec = state.duration_sec.or(session.duration_sec);
            }
            PlayState::Paused => {
                // 暂停不累加，但保留会话——继续播时接着算
                if let Some(session) = self.current.as_mut() {
                    session.last_position = state.position_sec;
                    session.furthest = session.furthest.max(state.position_sec);
                }
            }
            PlayState::Empty | PlayState::Ended => {}
        }

        emitted
    }

    /// 应用退出前冲刷。不调的话最后一段收听会丢。
    pub fn flush(&mut self) -> Option<PlayEvent> {
        self.take_current()
    }

    /// 当前这段听了多久。给 UI 显示用，不影响落库判断。
    pub fn current_listened_sec(&self) -> f64 {
        self.current.as_ref().map_or(0.0, |s| s.listened_sec)
    }

    fn take_current(&mut self) -> Option<PlayEvent> {
        let session = self.current.take()?;
        session.worth_recording().then(|| session.into_event())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(song: &str, play: PlayState, position: f64, duration: Option<f64>) -> PlaybackState {
        PlaybackState {
            song_id: song.into(),
            play_state: play,
            position_sec: position,
            duration_sec: duration,
            ..Default::default()
        }
    }

    /// 喂 n 拍，每拍 `step` 秒。返回期间吐出的事件。
    fn play_for(
        tracker: &mut PlayTracker,
        song: &str,
        seconds: f64,
        duration: Option<f64>,
    ) -> Vec<PlayEvent> {
        let step = 0.1;
        let mut out = Vec::new();
        let mut position = 0.0;
        let mut elapsed = 0.0;
        while elapsed < seconds {
            position += step;
            elapsed += step;
            if let Some(e) =
                tracker.observe(&state(song, PlayState::Playing, position, duration), step)
            {
                out.push(e);
            }
        }
        out
    }

    #[test]
    fn a_short_touch_is_not_recorded() {
        // 点开就切走不该进历史，否则「最近播放」会被误触淹没
        let mut tracker = PlayTracker::new("library");
        play_for(&mut tracker, "001", 2.0, Some(200.0));
        assert!(tracker.flush().is_none());
    }

    #[test]
    fn listening_long_enough_is_recorded_on_flush() {
        let mut tracker = PlayTracker::new("library");
        play_for(&mut tracker, "001", 8.0, Some(200.0));
        let event = tracker.flush().expect("应当记一条");
        assert_eq!(event.song_id, "001");
        assert!(event.listened_sec >= MIN_LISTENED_SEC);
        assert_eq!(event.source, "library");
    }

    #[test]
    fn switching_tracks_emits_the_previous_session() {
        let mut tracker = PlayTracker::new("library");
        play_for(&mut tracker, "001", 8.0, Some(200.0));
        // 换到另一首
        let event = tracker
            .observe(&state("002", PlayState::Playing, 0.0, Some(200.0)), 0.1)
            .expect("换歌时应当结掉上一段");
        assert_eq!(event.song_id, "001");
        // 新的一段已经开始
        play_for(&mut tracker, "002", 8.0, Some(200.0));
        assert_eq!(tracker.flush().unwrap().song_id, "002");
    }

    #[test]
    fn stopping_emits_the_session() {
        let mut tracker = PlayTracker::new("library");
        play_for(&mut tracker, "001", 8.0, Some(200.0));
        let event = tracker
            .observe(&state("", PlayState::Empty, 0.0, None), 0.1)
            .expect("停止时应当结掉");
        assert_eq!(event.song_id, "001");
    }

    #[test]
    fn pause_does_not_accumulate_but_keeps_the_session() {
        let mut tracker = PlayTracker::new("library");
        play_for(&mut tracker, "001", 6.0, Some(200.0));
        let before = tracker.current_listened_sec();
        // 暂停 100 拍
        for _ in 0..100 {
            assert!(
                tracker
                    .observe(&state("001", PlayState::Paused, 6.0, Some(200.0)), 0.1)
                    .is_none()
            );
        }
        assert!(
            (tracker.current_listened_sec() - before).abs() < 1e-6,
            "暂停期间不该累加"
        );
        // 会话还在，继续播能接着算
        play_for(&mut tracker, "001", 2.0, Some(200.0));
        assert!(tracker.current_listened_sec() > before);
    }

    #[test]
    fn looping_counts_wall_clock_not_position() {
        // 单句循环：位置在原地打转，但人确实在反复听
        let mut tracker = PlayTracker::new("library");
        for _ in 0..100 {
            // 位置在 10~11 秒之间来回，从不推进
            tracker.observe(&state("001", PlayState::Playing, 10.5, Some(200.0)), 0.1);
        }
        let event = tracker.flush().expect("循环听也算听");
        assert!(
            event.listened_sec >= 9.0,
            "应当累计约 10 秒墙钟，实际 {}",
            event.listened_sec
        );
    }

    #[test]
    fn seeking_forward_does_not_inflate_listened_time() {
        // 拖动进度条：位置瞬间跳几分钟，但没人听
        let mut tracker = PlayTracker::new("library");
        play_for(&mut tracker, "001", 6.0, Some(300.0));
        tracker.observe(&state("001", PlayState::Playing, 280.0, Some(300.0)), 0.1);
        let event = tracker.flush().unwrap();
        assert!(
            event.listened_sec < 10.0,
            "拖动不该被算成收听，实际 {}",
            event.listened_sec
        );
    }

    #[test]
    fn a_long_gap_between_samples_is_not_counted() {
        // 系统休眠、进程被挂起：中间那段空档不能算成在听
        let mut tracker = PlayTracker::new("library");
        play_for(&mut tracker, "001", 6.0, Some(200.0));
        let before = tracker.current_listened_sec();
        tracker.observe(
            &state("001", PlayState::Playing, 3600.0, Some(200.0)),
            3600.0,
        );
        assert!(
            (tracker.current_listened_sec() - before).abs() < 1e-6,
            "异常的采样间隔不该累加"
        );
    }

    #[test]
    fn reaching_the_end_marks_completed() {
        let mut tracker = PlayTracker::new("library");
        for i in 0..100 {
            // 位置一路推到 195/200 = 97.5%
            tracker.observe(
                &state("001", PlayState::Playing, i as f64 * 1.95, Some(200.0)),
                0.1,
            );
        }
        let event = tracker.flush().unwrap();
        assert!(event.completed, "播到 97.5% 应当算听完");
    }

    #[test]
    fn stopping_early_is_not_completed() {
        let mut tracker = PlayTracker::new("library");
        play_for(&mut tracker, "001", 8.0, Some(200.0));
        assert!(!tracker.flush().unwrap().completed);
    }

    #[test]
    fn unknown_duration_never_claims_completion() {
        // 时长未知时不敢说「听完了」，宁可少记
        let mut tracker = PlayTracker::new("library");
        play_for(&mut tracker, "001", 20.0, None);
        assert!(!tracker.flush().unwrap().completed);
    }

    #[test]
    fn completion_survives_a_seek_near_the_end() {
        // 播到结尾又拖回去，仍然算听完——用见过的最大位置判断
        let mut tracker = PlayTracker::new("library");
        for i in 0..100 {
            tracker.observe(
                &state("001", PlayState::Playing, i as f64 * 1.95, Some(200.0)),
                0.1,
            );
        }
        tracker.observe(&state("001", PlayState::Playing, 5.0, Some(200.0)), 0.1);
        assert!(tracker.flush().unwrap().completed);
    }

    #[test]
    fn flush_is_idempotent() {
        let mut tracker = PlayTracker::new("library");
        play_for(&mut tracker, "001", 8.0, Some(200.0));
        assert!(tracker.flush().is_some());
        assert!(tracker.flush().is_none(), "冲刷两次不该记两条");
    }

    #[test]
    fn empty_state_from_the_start_emits_nothing() {
        let mut tracker = PlayTracker::new("library");
        assert!(
            tracker
                .observe(&state("", PlayState::Empty, 0.0, None), 0.1)
                .is_none()
        );
        assert!(tracker.flush().is_none());
    }
}
