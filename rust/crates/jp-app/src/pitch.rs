//! 变调（应用层）：什么时候渲染、渲染好之后换哪个文件、接着哪里播。
//!
//! 引擎只播文件（`jp_audio::AudioEngine`），渲染和缓存在 `jp_audio::pitch`。这里把两者接起来，行为照 Python 版：
//!
//! - 半音数是全局的：设了 +2，之后打开的每首歌都按 +2 播，播放条上一直显示着；
//! - 打开一首歌时如果还没有这个调的缓存，**先渲染、渲染好再开播**，不先用原调放一段；
//! - 正在播的时候换调，原来的继续放，渲染好后**接着当时的位置和播放状态**换过去
//!   （Python 版是跳回点击时的位置，这里改成不跳）；
//! - 渲染失败（原文件解不开、缓存目录写不了）就退回原调并说明原因，不悄悄没声音；
//! - 缓存目录有总量上限（设置里能改）：用到哪个缓存就记一笔，渲染完一个新的就把最久没用的清掉。
//!
//! 不跨重启保存：应用启动总是原调，和倍速一样。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Result;
use jp_audio::pitch::{RenderOutcome, clamp_semitones};

/// 播放器里和变调有关的那几个操作。应用里是 `AudioEngine`，测试里是假的。
pub trait Deck: Send + Sync {
    /// 当前加载着的歌；没加载或已停止时 None
    fn loaded_song(&self) -> Option<String>;
    fn position_sec(&self) -> f64;
    fn is_playing(&self) -> bool;
    fn load_at(&self, path: &Path, song_id: &str, position_sec: f64, play: bool) -> Result<()>;
    fn stop(&self);
}

impl Deck for jp_audio::AudioEngine {
    fn loaded_song(&self) -> Option<String> {
        let state = self.state();
        (state.play_state != jp_audio::PlayState::Empty && !state.song_id.is_empty())
            .then_some(state.song_id)
    }

    fn position_sec(&self) -> f64 {
        self.state().position_sec
    }

    fn is_playing(&self) -> bool {
        self.state().play_state == jp_audio::PlayState::Playing
    }

    fn load_at(&self, path: &Path, song_id: &str, position_sec: f64, play: bool) -> Result<()> {
        jp_audio::AudioEngine::load_at(self, path, song_id, position_sec, play)
    }

    fn stop(&self) {
        jp_audio::AudioEngine::stop(self);
    }
}

/// 渲染变调版本。应用里是编进来的 Rubber Band，测试里是假的。
pub trait Renderer: Send + Sync {
    fn cache_path(&self, source: &Path, semitones: i32) -> std::io::Result<PathBuf>;
    fn render(
        &self,
        source: &Path,
        output: &Path,
        semitones: i32,
        cancel: &AtomicBool,
    ) -> Result<RenderOutcome>;
    /// 这个缓存刚被拿去播了。应用里据此记「最近使用时间」，淘汰时按它排先后
    fn used(&self, _cache: &Path) {}
}

/// 变调缓存放哪、最多占多少。设置页改上限时改的就是这里的数，渲染线程下一次淘汰时生效。
pub struct PitchCache {
    pub dir: PathBuf,
    limit_bytes: AtomicU64,
}

impl PitchCache {
    pub fn new(dir: PathBuf, limit_bytes: u64) -> Self {
        Self {
            dir,
            limit_bytes: AtomicU64::new(limit_bytes),
        }
    }

    pub fn limit_bytes(&self) -> u64 {
        self.limit_bytes.load(Ordering::Relaxed)
    }

    pub fn set_limit_bytes(&self, bytes: u64) {
        self.limit_bytes.store(bytes, Ordering::Relaxed);
    }

    pub fn usage(&self) -> jp_audio::pitch::CacheUsage {
        jp_audio::pitch::cache_usage(&self.dir)
    }

    /// 压到上限以内。`keep` 是刚渲染好、马上要播的那个，不删
    pub fn enforce(&self, keep: Option<&Path>) -> jp_audio::pitch::Evicted {
        let evicted = jp_audio::pitch::enforce_limit(&self.dir, self.limit_bytes(), keep);
        if evicted.files > 0 {
            crate::log::info(format!(
                "变调缓存超过上限，清掉最久没用的 {} 个（{} MB），还剩 {} 个（{} MB）",
                evicted.files,
                evicted.bytes / (1024 * 1024),
                evicted.remaining.files,
                evicted.remaining.bytes / (1024 * 1024),
            ));
        }
        evicted
    }
}

/// 用编进来的 Rubber Band 渲染（`jp_audio::pitch`）。
pub struct LibraryRenderer {
    pub cache: Arc<PitchCache>,
}

impl Renderer for LibraryRenderer {
    fn cache_path(&self, source: &Path, semitones: i32) -> std::io::Result<PathBuf> {
        jp_audio::pitch::cache_path(&self.cache.dir, source, semitones)
    }

    fn render(
        &self,
        source: &Path,
        output: &Path,
        semitones: i32,
        cancel: &AtomicBool,
    ) -> Result<RenderOutcome> {
        let outcome = jp_audio::pitch::render(source, output, semitones, cancel)?;
        if outcome == RenderOutcome::Rendered {
            self.cache.enforce(Some(output));
        }
        Ok(outcome)
    }

    fn used(&self, cache: &Path) {
        jp_audio::pitch::mark_used(cache);
    }
}

/// 给前端看的变调状态
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PitchStatus {
    pub semitones: i32,
    pub rendering: bool,
    /// 最近一次失败：(序号, 原因)。序号变了前端才提示，免得每次轮询都弹
    pub error: Option<(u64, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Start {
    /// 打开这首歌：渲染好后从这里开始，播不播看 autoplay
    Fresh { position_sec: f64, autoplay: bool },
    /// 这首歌正在放，换调：渲染好后接着那时的位置和播放状态
    Swap,
}

struct Job {
    song_id: String,
    source: PathBuf,
    semitones: i32,
    start: Start,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
struct Inner {
    semitones: i32,
    /// 最近打开的歌和它的原文件
    current: Option<(String, PathBuf)>,
    job: Option<Job>,
    error: Option<(u64, String)>,
    error_seq: u64,
}

impl Inner {
    fn cancel_job(&mut self) -> Option<Job> {
        let job = self.job.take();
        if let Some(job) = &job {
            job.cancel.store(true, Ordering::Relaxed);
        }
        job
    }

    fn fail(&mut self, message: String) {
        self.error_seq += 1;
        self.error = Some((self.error_seq, message));
        self.semitones = 0;
    }
}

#[derive(Clone)]
pub struct PitchControl {
    inner: Arc<Mutex<Inner>>,
    renderer: Arc<dyn Renderer>,
}

impl PitchControl {
    pub fn new(renderer: Arc<dyn Renderer>) -> Self {
        Self {
            inner: Arc::default(),
            renderer,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> PitchStatus {
        let inner = self.lock();
        PitchStatus {
            semitones: inner.semitones,
            rendering: inner.job.is_some(),
            error: inner.error.clone(),
        }
    }

    /// 打开一首歌。当前调是原调或已有缓存就直接播；否则停掉正在放的，渲染好再播。
    pub fn load(
        &self,
        deck: Arc<dyn Deck>,
        song_id: &str,
        source: &Path,
        position_sec: f64,
        autoplay: bool,
    ) -> Result<()> {
        let mut inner = self.lock();
        inner.cancel_job();
        inner.current = Some((song_id.to_owned(), source.to_owned()));
        let start = Start::Fresh {
            position_sec,
            autoplay,
        };
        self.play_version(&mut inner, deck, song_id, source, start)
    }

    /// 换调。正在放（或正等着渲染）的那首跟着换。
    pub fn set_semitones(&self, deck: Arc<dyn Deck>, semitones: i32) -> Result<()> {
        let mut inner = self.lock();
        let semitones = clamp_semitones(semitones);
        let pending = inner.cancel_job();
        inner.semitones = semitones;
        let start = match &pending {
            Some(job) => job.start,
            None => Start::Swap,
        };
        let Some((song_id, source)) = inner.current.clone() else {
            return Ok(());
        };
        if start == Start::Swap && deck.loaded_song().as_deref() != Some(song_id.as_str()) {
            // 什么都没在放（停了，或放的不是最近打开的那首）：下次打开时按新调
            return Ok(());
        }
        self.play_version(&mut inner, deck, &song_id, &source, start)
    }

    /// 等渲染时按了播放/暂停、拖了进度：记到渲染好之后。返回 true 表示已经处理，不用再交给播放器。
    pub fn intercept_play(&self, play: Option<bool>) -> bool {
        let mut inner = self.lock();
        match inner.job.as_mut() {
            Some(Job {
                start: Start::Fresh { autoplay, .. },
                ..
            }) => {
                *autoplay = play.unwrap_or(!*autoplay);
                true
            }
            _ => false,
        }
    }

    pub fn intercept_seek(&self, to: f64) -> bool {
        let mut inner = self.lock();
        match inner.job.as_mut() {
            Some(Job {
                start: Start::Fresh { position_sec, .. },
                ..
            }) => {
                *position_sec = to.max(0.0);
                true
            }
            _ => false,
        }
    }

    /// 停止播放：正在等的渲染也不要了
    pub fn stop(&self) {
        self.lock().cancel_job();
    }

    /// 按当前调把这首歌放出来：原调或有缓存立刻换；否则开线程渲染。调用时持有锁。
    fn play_version(
        &self,
        inner: &mut Inner,
        deck: Arc<dyn Deck>,
        song_id: &str,
        source: &Path,
        start: Start,
    ) -> Result<()> {
        let semitones = inner.semitones;
        let ready = if semitones == 0 {
            Some(source.to_owned())
        } else {
            match self.renderer.cache_path(source, semitones) {
                Ok(cache) if is_usable(&cache) => {
                    self.renderer.used(&cache);
                    Some(cache)
                }
                Ok(_) => None,
                Err(err) => {
                    inner.fail(format!("变调失败，按原调播放：读不到音频文件（{err}）"));
                    Some(source.to_owned())
                }
            }
        };
        if let Some(path) = ready {
            return switch_to(deck.as_ref(), &path, song_id, start);
        }

        if matches!(start, Start::Fresh { .. }) {
            deck.stop();
        }
        let cancel = Arc::new(AtomicBool::new(false));
        inner.job = Some(Job {
            song_id: song_id.to_owned(),
            source: source.to_owned(),
            semitones,
            start,
            cancel: cancel.clone(),
        });
        let control = self.clone();
        let source = source.to_owned();
        std::thread::Builder::new()
            .name("jp-pitch-render".into())
            .spawn(move || {
                let outcome = control
                    .renderer
                    .cache_path(&source, semitones)
                    .map_err(anyhow::Error::from)
                    .and_then(|output| {
                        control
                            .renderer
                            .render(&source, &output, semitones, &cancel)
                            .map(|o| (o, output))
                    });
                control.finish(deck, &cancel, outcome);
            })?;
        Ok(())
    }

    /// 渲染线程结束。只认还是当前任务的那一个（按取消标志的指针认）。
    fn finish(
        &self,
        deck: Arc<dyn Deck>,
        token: &Arc<AtomicBool>,
        outcome: Result<(RenderOutcome, PathBuf)>,
    ) {
        let mut inner = self.lock();
        if !inner
            .job
            .as_ref()
            .is_some_and(|job| Arc::ptr_eq(&job.cancel, token))
        {
            return;
        }
        let Some(job) = inner.job.take() else { return };
        let result = match outcome {
            Ok((RenderOutcome::Cancelled, _)) => return,
            Ok((_, output)) => switch_to(deck.as_ref(), &output, &job.song_id, job.start),
            Err(err) => Err(err),
        };
        if let Err(err) = result {
            inner.fail(format!(
                "{:+} 半音的变调失败，已换回原调：{err:#}",
                job.semitones
            ));
            if let Err(err) = switch_to(deck.as_ref(), &job.source, &job.song_id, job.start) {
                inner.error = Some((inner.error_seq, format!("{:#}", err)));
            }
        }
    }
}

/// 把这首歌换成 `path` 这个版本
fn switch_to(deck: &dyn Deck, path: &Path, song_id: &str, start: Start) -> Result<()> {
    match start {
        Start::Fresh {
            position_sec,
            autoplay,
        } => deck.load_at(path, song_id, position_sec, autoplay),
        Start::Swap => {
            if deck.loaded_song().as_deref() != Some(song_id) {
                return Ok(());
            }
            deck.load_at(path, song_id, deck.position_sec(), deck.is_playing())
        }
    }
}

fn is_usable(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.len() > 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    use super::*;

    #[derive(Default)]
    struct FakeDeck {
        /// (文件, 歌, 位置, 播不播)
        loads: Mutex<Vec<(PathBuf, String, f64, bool)>>,
        loaded: Mutex<Option<String>>,
        position: Mutex<f64>,
        playing: AtomicBool,
        stops: Mutex<usize>,
    }

    impl Deck for FakeDeck {
        fn loaded_song(&self) -> Option<String> {
            self.loaded.lock().unwrap().clone()
        }
        fn position_sec(&self) -> f64 {
            *self.position.lock().unwrap()
        }
        fn is_playing(&self) -> bool {
            self.playing.load(Ordering::Relaxed)
        }
        fn load_at(&self, path: &Path, song_id: &str, position_sec: f64, play: bool) -> Result<()> {
            self.loads.lock().unwrap().push((
                path.to_owned(),
                song_id.to_owned(),
                position_sec,
                play,
            ));
            *self.loaded.lock().unwrap() = Some(song_id.to_owned());
            *self.position.lock().unwrap() = position_sec;
            self.playing.store(play, Ordering::Relaxed);
            Ok(())
        }
        fn stop(&self) {
            *self.stops.lock().unwrap() += 1;
            *self.loaded.lock().unwrap() = None;
            self.playing.store(false, Ordering::Relaxed);
        }
    }

    impl FakeDeck {
        fn last_load(&self) -> Option<(PathBuf, String, f64, bool)> {
            self.loads.lock().unwrap().last().cloned()
        }
    }

    /// 渲染要等测试按输出文件名放行（各放各的，取消掉的线程抢不走别人的）
    struct FakeRenderer {
        dir: PathBuf,
        released: Mutex<HashMap<String, Result<(), String>>>,
        started: Mutex<Vec<(PathBuf, i32)>>,
        used: Mutex<Vec<PathBuf>>,
    }

    impl Renderer for FakeRenderer {
        fn cache_path(&self, source: &Path, semitones: i32) -> std::io::Result<PathBuf> {
            let stem = source.file_stem().unwrap().to_string_lossy();
            Ok(self.dir.join(format!("{stem}_{semitones:+}.flac")))
        }
        fn render(
            &self,
            source: &Path,
            output: &Path,
            semitones: i32,
            cancel: &AtomicBool,
        ) -> Result<RenderOutcome> {
            self.started
                .lock()
                .unwrap()
                .push((source.to_owned(), semitones));
            let name = output.file_name().unwrap().to_string_lossy().into_owned();
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(RenderOutcome::Cancelled);
                }
                let released = self.released.lock().unwrap().get(&name).cloned();
                match released {
                    Some(Ok(())) => {
                        std::fs::write(output, b"flac").unwrap();
                        return Ok(RenderOutcome::Rendered);
                    }
                    Some(Err(message)) => anyhow::bail!(message),
                    None => std::thread::sleep(Duration::from_millis(2)),
                }
            }
        }
        fn used(&self, cache: &Path) {
            self.used.lock().unwrap().push(cache.to_owned());
        }
    }

    struct Fixture {
        control: PitchControl,
        deck: Arc<FakeDeck>,
        renderer: Arc<FakeRenderer>,
        dir: PathBuf,
    }

    fn fixture(name: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!("jp-app-pitch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let renderer = Arc::new(FakeRenderer {
            dir: dir.clone(),
            released: Mutex::default(),
            started: Mutex::default(),
            used: Mutex::default(),
        });
        Fixture {
            control: PitchControl::new(renderer.clone()),
            deck: Arc::default(),
            renderer,
            dir,
        }
    }

    impl Fixture {
        fn deck(&self) -> Arc<dyn Deck> {
            self.deck.clone()
        }
        fn song(&self, id: &str) -> PathBuf {
            self.dir.join(format!("{id}.flac"))
        }
        fn release(&self, output: &str, result: Result<(), String>) {
            self.renderer
                .released
                .lock()
                .unwrap()
                .insert(output.to_owned(), result);
        }
        fn wait_idle(&self) {
            let started = Instant::now();
            while self.control.status().rendering {
                assert!(started.elapsed() < Duration::from_secs(5), "渲染没结束");
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        fn wait_render_started(&self, count: usize) {
            let started = Instant::now();
            while self.renderer.started.lock().unwrap().len() < count {
                assert!(started.elapsed() < Duration::from_secs(5), "渲染没开始");
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }

    #[test]
    fn original_key_loads_the_original_file_directly() {
        let f = fixture("original");
        f.control
            .load(f.deck(), "001", &f.song("001"), 12.0, false)
            .unwrap();
        assert_eq!(
            f.deck.last_load(),
            Some((f.song("001"), "001".into(), 12.0, false))
        );
        assert!(!f.control.status().rendering);
    }

    #[test]
    fn opening_a_song_in_a_new_key_waits_for_the_render_then_starts_where_asked() {
        let f = fixture("fresh");
        f.control.set_semitones(f.deck(), 2).unwrap();
        f.deck.load_at(&f.song("000"), "000", 30.0, true).unwrap();

        f.control
            .load(f.deck(), "001", &f.song("001"), 5.0, true)
            .unwrap();
        assert!(f.control.status().rendering);
        assert_eq!(
            *f.deck.stops.lock().unwrap(),
            1,
            "上一首要停，不能先用原调放新歌"
        );
        assert_eq!(f.deck.loads.lock().unwrap().len(), 1);

        // 等渲染的时候按了暂停、拖了进度
        assert!(f.control.intercept_play(Some(false)));
        assert!(f.control.intercept_seek(40.0));
        f.wait_render_started(1);
        f.release("001_+2.flac", Ok(()));
        f.wait_idle();
        assert_eq!(
            f.deck.last_load(),
            Some((f.dir.join("001_+2.flac"), "001".into(), 40.0, false))
        );
        assert_eq!(
            f.control.status(),
            PitchStatus {
                semitones: 2,
                rendering: false,
                error: None
            }
        );
    }

    #[test]
    fn a_cached_key_switches_immediately() {
        let f = fixture("cached");
        std::fs::write(f.dir.join("001_-3.flac"), b"flac").unwrap();
        f.control.set_semitones(f.deck(), -3).unwrap();
        f.control
            .load(f.deck(), "001", &f.song("001"), 0.0, true)
            .unwrap();
        assert_eq!(
            f.deck.last_load(),
            Some((f.dir.join("001_-3.flac"), "001".into(), 0.0, true))
        );
        assert!(f.renderer.started.lock().unwrap().is_empty());
        // 拿缓存去播要记一笔：淘汰时按最近使用时间排，不记的话常听的那首反而先被清掉
        assert_eq!(
            *f.renderer.used.lock().unwrap(),
            [f.dir.join("001_-3.flac")]
        );
    }

    #[test]
    fn changing_key_while_playing_keeps_playing_and_swaps_at_the_current_position() {
        let f = fixture("swap");
        f.control
            .load(f.deck(), "001", &f.song("001"), 0.0, true)
            .unwrap();
        f.control.set_semitones(f.deck(), 1).unwrap();
        assert!(f.control.status().rendering);
        assert_eq!(*f.deck.stops.lock().unwrap(), 0, "换调时原来的要继续放");

        *f.deck.position.lock().unwrap() = 63.5;
        f.wait_render_started(1);
        f.release("001_+1.flac", Ok(()));
        f.wait_idle();
        assert_eq!(
            f.deck.last_load(),
            Some((f.dir.join("001_+1.flac"), "001".into(), 63.5, true))
        );

        // 回到原调：立刻换回原文件，位置接着
        *f.deck.position.lock().unwrap() = 70.0;
        f.control.set_semitones(f.deck(), 0).unwrap();
        assert_eq!(
            f.deck.last_load(),
            Some((f.song("001"), "001".into(), 70.0, true))
        );
    }

    #[test]
    fn switching_song_or_key_during_a_render_cancels_it() {
        let f = fixture("cancel");
        f.control.set_semitones(f.deck(), 4).unwrap();
        f.control
            .load(f.deck(), "001", &f.song("001"), 0.0, true)
            .unwrap();
        f.wait_render_started(1);
        // 渲染中又换了调：旧任务取消，按新调重来，开始参数保留
        f.control.set_semitones(f.deck(), 5).unwrap();
        f.wait_render_started(2);
        // 又换了歌
        f.control
            .load(f.deck(), "002", &f.song("002"), 0.0, true)
            .unwrap();
        f.wait_render_started(3);
        // 被取消的两个即使这时放行也不该再生效
        f.release("001_+4.flac", Ok(()));
        f.release("001_+5.flac", Ok(()));
        f.release("002_+5.flac", Ok(()));
        f.wait_idle();
        assert_eq!(
            f.deck.loads.lock().unwrap().clone(),
            vec![(f.dir.join("002_+5.flac"), "002".into(), 0.0, true)]
        );
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(
            f.deck.loads.lock().unwrap().len(),
            1,
            "取消掉的渲染不能再换文件"
        );
    }

    #[test]
    fn a_failed_render_falls_back_to_the_original_key_and_says_why() {
        let f = fixture("fail");
        f.control.set_semitones(f.deck(), -2).unwrap();
        f.control
            .load(f.deck(), "001", &f.song("001"), 8.0, true)
            .unwrap();
        f.wait_render_started(1);
        f.release("001_-2.flac", Err("解码失败 001.flac".into()));
        f.wait_idle();
        assert_eq!(
            f.deck.last_load(),
            Some((f.song("001"), "001".into(), 8.0, true))
        );
        let status = f.control.status();
        assert_eq!(status.semitones, 0);
        let (seq, message) = status.error.unwrap();
        assert_eq!(seq, 1);
        assert!(
            message.contains("-2 半音") && message.contains("解码失败 001.flac"),
            "{message}"
        );
    }

    /// 真的渲染器（编进来的 Rubber Band），假的播放器：不需要声卡，也不需要 ffmpeg。
    /// 渲染出来的是能解码的 WAV、和原曲一样长；再用到它时记一笔「刚用过」；
    /// 缓存超过上限时清掉最久没用的，刚渲染的那个留着。
    #[test]
    fn the_built_in_renderer_renders_caches_and_evicts_without_ffmpeg() {
        let dir = std::env::temp_dir().join(format!("jp-app-pitch-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let song = |name: &str, hz: f64| {
            let rate = 22_050u32;
            let samples: Vec<f32> = (0..rate as usize * 2)
                .flat_map(|i| {
                    let value = (0.4 * (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(rate)).sin()) as f32;
                    [value, value]
                })
                .collect();
            let path = dir.join(name);
            jp_audio::wav::write_pcm16(&path, &jp_audio::Pcm { samples, channels: 2, rate }).unwrap();
            path
        };
        let (first, second) = (song("001.wav", 440.0), song("002.wav", 330.0));

        let cache = Arc::new(PitchCache::new(dir.join("cache"), jp_audio::pitch::DEFAULT_CACHE_LIMIT_BYTES));
        let control = PitchControl::new(Arc::new(LibraryRenderer { cache: cache.clone() }));
        let deck: Arc<FakeDeck> = Arc::default();
        let wait_idle = || {
            let started = Instant::now();
            while control.status().rendering {
                assert!(started.elapsed() < Duration::from_secs(60), "渲染没结束");
                std::thread::sleep(Duration::from_millis(5));
            }
        };

        control.set_semitones(deck.clone(), 2).unwrap();
        control.load(deck.clone(), "001", &first, 0.0, true).unwrap();
        wait_idle();
        assert_eq!(control.status().error, None);
        let (rendered, id, _, _) = deck.last_load().expect("渲染好了要开播");
        assert_eq!(id, "001");
        assert_eq!(rendered, jp_audio::pitch::cache_path(&cache.dir, &first, 2).unwrap());
        let decoded = jp_audio::decode_all(&rendered).unwrap();
        assert_eq!((decoded.channels, decoded.rate, decoded.frames()), (2, 22_050, 44_100), "变调版要和原曲一样长");
        assert_eq!(cache.usage().files, 1);

        // 再打开同一首：直接用缓存，并把它的最近使用时间改成现在
        let old = std::time::SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options().write(true).open(&rendered).unwrap().set_modified(old).unwrap();
        control.load(deck.clone(), "001", &first, 0.0, true).unwrap();
        assert!(!control.status().rendering, "有缓存就不该再渲染");
        let touched = std::fs::metadata(&rendered).unwrap().modified().unwrap();
        assert!(touched > old + Duration::from_secs(1800), "用过的缓存要记一笔");

        // 上限压到放不下两个：渲染第二首之后，第一首的缓存被清掉，刚渲染的留着
        cache.set_limit_bytes(std::fs::metadata(&rendered).unwrap().len() + 1);
        control.load(deck.clone(), "002", &second, 0.0, true).unwrap();
        wait_idle();
        assert_eq!(control.status().error, None);
        let (latest, id, _, _) = deck.last_load().unwrap();
        assert_eq!(id, "002");
        assert!(latest.is_file(), "刚渲染好、正要播的缓存不能被清掉");
        assert!(!rendered.exists(), "超过上限，最久没用的该清掉");
        assert_eq!(cache.usage().files, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changing_key_with_nothing_loaded_only_remembers_it() {
        let f = fixture("idle");
        f.control.set_semitones(f.deck(), 3).unwrap();
        assert_eq!(f.control.status().semitones, 3);
        assert!(!f.control.status().rendering);
        assert!(f.deck.loads.lock().unwrap().is_empty());
        assert!(
            !f.control.intercept_play(None),
            "没在等渲染时，播放键照常交给播放器"
        );
    }
}
