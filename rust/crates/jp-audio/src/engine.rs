//! 播放引擎。唯一碰音频设备的模块。
//!
//! 链路：
//!
//! ```text
//! Decoder ─→ RateControlled(WSOLA) ─→ Tapped ─→ 换算成声卡格式 ─→ Player ─→ 声卡
//!                   ↑                    │
//!              倍速（原子变量）        采样副本 → 频谱
//! ```
//!
//! **每首歌进 Player 之前先换算成声卡的采样率和声道数。** Player 是一条一直开着的队列，
//! rodio 只在「段」（span）的边界重新读队列里当前歌的采样率；WSOLA 报的是「没有分段」，
//! 于是本次启动第一首歌的采样率会被一直用下去。曲库里 44.1 kHz 和 48 kHz 混着，
//! 后面采样率不同的歌就整首变调变速（约 1.5 个半音、8.8%）。在这里按每首歌自己的采样率换算，
//! 队列里所有歌格式相同，就不存在沿用错的问题。
//!
//! **位置是「歌里的秒数」，自己数，不用 rodio 的 `get_pos`。**
//! rodio 数的是送进声卡的样本（墙上时间），而 WSOLA 是时间伸缩：1.5 倍速下
//! 每 1 秒输出对应歌里 1.5 秒，于是 rodio 报的位置只有真实位置的 2/3——
//! 歌词按这个位置对齐就会越走越偏。`RateControlled` 每吐一个样本就按当时的倍速
//! 往前记一点，记的是歌里的秒数；跳转也先按倍速换算回去
//! （`rodio_wsola::Wsola::try_seek` 把传进来的时刻当「输出时间」，会乘一次倍速）。
//!
//! **为什么不用 rodio 自带的 `set_speed`**：它的实现是「提高采样率」，
//! 也就是重采样——0.75 倍速会连音高一起降下去。对语言学习工具这是致命的：
//! 慢放是为了听清词，不是为了把人变成低音炮。所以走 WSOLA 做时间伸缩，
//! 变速不变调，和 Python 版 `QMediaPlayer.setPlaybackRate()` 的行为对齐。

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use rodio::source::{SeekError, UniformSourceIterator};
use rodio::{ChannelCount, Decoder, Player, SampleRate, Source};
use rodio_wsola::Wsola;

use crate::state::{LoopRegion, PlayState, PlaybackState, clamp_rate, clamp_volume};
use crate::tap::{TapBuffer, Tapped};

/// 循环看门狗的轮询间隔。
///
/// 20ms 是精度和 CPU 的折中。注意 rodio 会预缓冲，回跳时已经进了输出缓冲的
/// 那一小段仍会播出来，所以实际听感上的循环点会比设定值晚一点——
/// 这个限制 Python 版（150ms QTimer）也有，而且更明显。
const LOOP_POLL: Duration = Duration::from_millis(20);

/// 跳转失败时依次往回退这么多秒再试。
///
/// 坏掉的结尾不只是最后一帧：实测那首 254 秒的 FLAC，倒数 2 秒内怎么跳都失败，
/// 退到倒数 5 秒就正常。退完还不行就当这首放完了。
const SEEK_FALLBACKS: [f64; 3] = [0.5, 2.0, 5.0];

/// 离结尾这么近的跳转，失败了就当「放完了」，不报错
const SEEK_TAIL_SEC: f64 = 10.0;

/// 跳转时离结尾至少留这么多秒。
///
/// 正好跳到结尾时，symphonia 会在「找下一个包」时报错而不是干净地结束；
/// 留一点余量，拖到最右边就是「放完最后一点然后结束」，而不是报错卡住。
const SEEK_END_MARGIN: f64 = 0.05;

/// 每隔这么多样本才去读一次倍速原子变量。
/// 每个样本都读会让原子操作成为热点，而倍速改变是人手操作，几百样本的
/// 延迟（毫秒级）完全感知不到。
const RATE_CHECK_INTERVAL: u32 = 512;

struct Shared {
    /// f32 的 bit 表示。用原子是为了让音频线程无锁读取。
    rate_bits: AtomicU32,
    /// **歌里**的播放位置（秒），f64 的 bit 表示。音频线程每吐一个样本往前记一点。
    /// 不用 rodio 的 `get_pos`——那个数的是墙上时间，倍速一改就和歌词对不上。
    position_bits: AtomicU64,
    song_id: Mutex<String>,
    duration: Mutex<Option<Duration>>,
    loop_region: Mutex<Option<LoopRegion>>,
    /// 有没有加载过音频。用来区分 Empty 和 Ended。
    loaded: AtomicBool,
}

impl Shared {
    fn new() -> Self {
        Self {
            rate_bits: AtomicU32::new(1.0f32.to_bits()),
            position_bits: AtomicU64::new(0.0f64.to_bits()),
            song_id: Mutex::new(String::new()),
            duration: Mutex::new(None),
            loop_region: Mutex::new(None),
            loaded: AtomicBool::new(false),
        }
    }

    fn rate(&self) -> f32 {
        f32::from_bits(self.rate_bits.load(Ordering::Relaxed))
    }

    fn set_rate(&self, rate: f32) {
        self.rate_bits.store(rate.to_bits(), Ordering::Relaxed);
    }

    fn position_sec(&self) -> f64 {
        f64::from_bits(self.position_bits.load(Ordering::Relaxed))
    }

    fn set_position_sec(&self, seconds: f64) {
        self.position_bits
            .store(seconds.max(0.0).to_bits(), Ordering::Relaxed);
    }
}

/// 包在 WSOLA 外面，从原子变量读倍速。
///
/// `Wsola::set_speed` 要 `&mut self`，而 Source 交给 Player 之后就拿不到句柄了，
/// 所以由这一层在音频线程内部改。
struct RateControlled<S: Source> {
    inner: Wsola<S>,
    shared: Arc<Shared>,
    applied: f32,
    countdown: u32,
    /// 歌里的位置（秒）。自己数一份，免得每个样本都去读原子变量
    position: f64,
}

impl<S: Source> RateControlled<S> {
    fn new(source: S, shared: Arc<Shared>) -> Self {
        let rate = shared.rate();
        Self {
            inner: Wsola::new(source, rate),
            shared,
            applied: rate,
            countdown: 0,
            // **从 0 数起，不要去读 shared 里的位置。** 一个 source 永远是从
            // 它自己的开头开始吐样本的；读 shared 等于把上一首的位置抄过来，
            // 然后每个样本再往上加——换一首歌，进度条和位置就接着上一首走。
            // 表现：播到 4:08 切下一首，新歌的位置报 4:08 并继续往上数，
            // 进度条顶在最右边，歌词跟随把整屏歌词滚到最后一行。
            //
            // 要从中间开始放的情形（变调换版本、放完再拖回去）由 `load_at`
            // 在 append 之后显式 seek 一次，`try_seek` 会把两边都设对。
            position: 0.0,
        }
    }

    fn sync_rate(&mut self) {
        if self.countdown > 0 {
            self.countdown -= 1;
            return;
        }
        self.countdown = RATE_CHECK_INTERVAL;
        let wanted = self.shared.rate();
        if (wanted - self.applied).abs() > f32::EPSILON {
            self.inner.set_speed(wanted);
            self.applied = wanted;
        }
    }

    /// 一个输出样本对应歌里多少秒：倍速 ÷（采样率 × 声道数）。
    /// 1.5 倍速下每吐一个样本，歌里就过去了 1.5 个样本的时间。
    fn seconds_per_sample(&self) -> f64 {
        let rate = f64::from(self.inner.sample_rate().get());
        let channels = f64::from(self.inner.channels().get());
        if rate <= 0.0 || channels <= 0.0 {
            return 0.0;
        }
        f64::from(self.applied) / (rate * channels)
    }
}

impl<S: Source> Iterator for RateControlled<S> {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        self.sync_rate();
        let sample = self.inner.next();
        if sample.is_some() {
            self.position += self.seconds_per_sample();
            self.shared.set_position_sec(self.position);
        }
        sample
    }
}

impl<S: Source> Source for RateControlled<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        // Wsola 报的是「按这个倍速放完要多久」，换回歌本身的长度
        self.inner
            .total_duration()
            .map(|d| d.mul_f32(self.applied.max(f32::EPSILON)))
    }

    /// `pos` 是**歌里**的时刻。`Wsola::try_seek` 把参数当输出时间、会乘一次倍速，
    /// 所以这里先除回去——不除的话 1.5 倍速下拖到 3:00 会去找 4:30，
    /// 超过结尾就直接播完了，表现就是「拖一下卡住」。
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        let speed = self.applied.max(f32::EPSILON);
        self.inner.try_seek(pos.div_f32(speed))?;
        self.position = pos.as_secs_f64();
        self.shared.set_position_sec(self.position);
        Ok(())
    }
}

/// 声卡要的格式
#[derive(Debug, Clone, Copy)]
struct OutputFormat {
    channels: ChannelCount,
    sample_rate: SampleRate,
}

/// 跳转的尝试顺序：先去要去的地方，不行就依次往回退一点。
///
/// 退到负数就不试了（开头本来就跳得动，失败一定是别的原因）。
fn seek_candidates(target_sec: f64) -> Vec<f64> {
    let mut out = vec![target_sec.max(0.0)];
    for back in SEEK_FALLBACKS {
        let candidate = target_sec - back;
        if candidate > 0.0 {
            out.push(candidate);
        }
    }
    out
}

/// 一首歌的播放链：倍速（WSOLA）→ 采样 tap → 换算成声卡格式。见模块注释。
fn playback_chain<S: Source + Send + 'static>(
    source: S,
    shared: &Arc<Shared>,
    tap: &Arc<TapBuffer>,
    output: OutputFormat,
) -> impl Source + Send + 'static {
    let tapped = Tapped::new(RateControlled::new(source, Arc::clone(shared)), Arc::clone(tap));
    UniformSourceIterator::new(tapped, output.channels, output.sample_rate)
}

/// 音频引擎。持有设备句柄，drop 掉就没声音了。
pub struct AudioEngine {
    // 必须持有：这个句柄一 drop，音频流就关了
    _device: rodio::MixerDeviceSink,
    // Arc 是为了和循环看门狗线程共享。Player 内部用原子做控制，
    // 所有方法都取 &self，可以安全共享。
    player: Arc<Player>,
    output: OutputFormat,
    shared: Arc<Shared>,
    tap: Arc<TapBuffer>,
    watcher_stop: Arc<AtomicBool>,
    /// 看门狗线程的句柄。**必须留着**：drop 的时候要等它真的停下来，
    /// 见 `impl Drop`。
    watcher: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// 当前这首的文件和 id。播完之后队列是空的，要重播就得照这个重新装一次
    current: Mutex<Option<(PathBuf, String)>>,
}

impl AudioEngine {
    /// 打开默认输出设备。没有声卡（CI、无头环境）时返回 Err，
    /// 调用方应当降级而不是崩溃。
    pub fn new() -> Result<Self> {
        Self::with_tap_capacity(crate::spectrum::DEFAULT_WINDOW * 4)
    }

    pub fn with_tap_capacity(tap_capacity: usize) -> Result<Self> {
        let device = rodio::DeviceSinkBuilder::open_default_sink()
            .context("打不开默认音频输出设备")?;
        let player = Arc::new(Player::connect_new(device.mixer()));
        let output = OutputFormat {
            channels: device.config().channel_count(),
            sample_rate: device.config().sample_rate(),
        };

        let shared = Arc::new(Shared::new());

        let engine = Self {
            _device: device,
            player,
            output,
            shared,
            tap: TapBuffer::new(tap_capacity),
            watcher_stop: Arc::new(AtomicBool::new(false)),
            watcher: Mutex::new(None),
            current: Mutex::new(None),
        };
        engine.spawn_loop_watcher();
        Ok(engine)
    }

    /// 循环看门狗。播到区间末尾就跳回起点。
    ///
    /// 单独一个线程而不是塞进 Source：Source 的 `next()` 里做 seek 会打断
    /// WSOLA 的重叠窗，产生咔哒声。
    fn spawn_loop_watcher(&self) {
        let player = Arc::clone(&self.player);
        let shared = Arc::clone(&self.shared);
        let stop = Arc::clone(&self.watcher_stop);
        let handle = std::thread::Builder::new()
            .name("jp-audio-loop".into())
            .spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    // 分片睡：`Drop` 要 join 这条线程，一口气睡满 LOOP_POLL
                    // 会让每次 drop 都多等一拍
                    for _ in 0..4 {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        std::thread::sleep(LOOP_POLL / 4);
                    }
                    // 必须先把区间取出来再放锁：持锁去做 seek 会挡住
                    // UI 线程的 set_loop。（drop(region) 是无效的——
                    // LoopRegion 是 Copy，drop 掉的是副本不是 guard。）
                    let region = {
                        let Ok(guard) = shared.loop_region.lock() else {
                            continue;
                        };
                        *guard
                    };
                    let Some(region) = region else { continue };
                    if player.is_paused() {
                        continue;
                    }
                    let pos = shared.position_sec();
                    if pos >= region.end_sec {
                        let _ = player.try_seek(Duration::from_secs_f64(region.start_sec));
                    }
                }
            })
            .expect("spawn loop watcher");
        if let Ok(mut slot) = self.watcher.lock() {
            *slot = Some(handle);
        }
    }

    /// 加载并开始播放。会替换当前正在播的内容。
    pub fn load(&self, path: &Path, song_id: &str) -> Result<()> {
        self.load_at(path, song_id, 0.0, true)
    }

    /// 加载后跳到 `position_sec`，`play` 为假时停在暂停。
    ///
    /// 换同一首歌的另一个版本（原调 ↔ 变调缓存）时用：接着原来的位置和播放状态，
    /// 单句循环也保留；换成别的歌才清掉循环。
    pub fn load_at(&self, path: &Path, song_id: &str, position_sec: f64, play: bool) -> Result<()> {
        let file = File::open(path)
            .with_context(|| format!("打不开音频文件 {}", path.display()))?;
        let decoder = Decoder::try_from(file)
            .with_context(|| format!("解码失败 {}", path.display()))?;
        let duration = decoder.total_duration();

        let source = playback_chain(decoder, &self.shared, &self.tap, self.output);

        self.player.clear();
        // 位置先摆到起点。新 source 自己也从 0 数（见 `RateControlled::new`），
        // 这一行管的是 append 到第一个样本吐出来之间那一小段：
        // 不清的话界面会先闪一下上一首的位置。
        self.shared.set_position_sec(0.0);
        self.player.append(source);
        let same_song = self.shared.loaded.load(Ordering::Relaxed)
            && self.shared.song_id.lock().map(|id| *id == song_id).unwrap_or(false);
        if let Ok(mut id) = self.shared.song_id.lock() {
            id.clear();
            id.push_str(song_id);
        }
        if let Ok(mut d) = self.shared.duration.lock() {
            *d = duration;
        }
        // 换歌时清掉循环区间：上一首的句子边界对这一首没有意义
        if !same_song && let Ok(mut region) = self.shared.loop_region.lock() {
            *region = None;
        }
        if let Ok(mut current) = self.current.lock() {
            *current = Some((path.to_path_buf(), song_id.to_owned()));
        }
        self.shared.loaded.store(true, Ordering::Relaxed);
        if position_sec > 0.0 {
            self.seek(position_sec)?;
        }
        if play {
            self.player.play();
        }
        Ok(())
    }

    /// 播完之后队列就空了，`play` / `seek` 作用在空队列上什么都不会发生。
    /// 这时按原文件重新装一次，从 `position_sec` 接着放——「放完了还能拖回去重播」。
    ///
    /// 返回是不是真的重装了。没播完、或者根本没加载过，返回 false，调用方照常走原来的路。
    fn replay_if_ended(&self, position_sec: f64) -> Result<bool> {
        if !self.shared.loaded.load(Ordering::Relaxed) || !self.player.empty() {
            return Ok(false);
        }
        let current = self.current.lock().ok().and_then(|c| c.clone());
        let Some((path, song_id)) = current else { return Ok(false) };
        self.load_at(&path, &song_id, position_sec, true)?;
        Ok(true)
    }

    pub fn play(&self) {
        // 放完之后按播放：从头再来一遍
        if matches!(self.replay_if_ended(0.0), Ok(true)) {
            return;
        }
        self.player.play();
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn toggle(&self) {
        if matches!(self.replay_if_ended(0.0), Ok(true)) {
            return;
        }
        if self.player.is_paused() {
            self.player.play();
        } else {
            self.player.pause();
        }
    }

    pub fn stop(&self) {
        self.player.stop();
        self.shared.loaded.store(false, Ordering::Relaxed);
        self.shared.set_position_sec(0.0);
        if let Ok(mut current) = self.current.lock() {
            *current = None;
        }
        if let Ok(mut region) = self.shared.loop_region.lock() {
            *region = None;
        }
    }

    /// 跳到歌里的第 `position_sec` 秒。
    ///
    /// **结尾附近要能兜住。** 库里有一批 FLAC 结尾是坏的（ffmpeg 报 `invalid residual`），
    /// 实测拖到最后 2 秒内 symphonia 必定报错；旧代码把这个错直接抛给界面，
    /// 播放停在原地不动——用户看到的就是「拖一下卡住」。
    ///
    /// 现在：先夹进 \[0, 时长 − 50ms\]，失败就往回退一点再试
    /// （[`SEEK_FALLBACKS`]），**退到头还是不行就当这首放完了**，不弹报错。
    /// 中间位置跳不动才是真出了问题，照常报错。
    pub fn seek(&self, position_sec: f64) -> Result<()> {
        let target_sec = self.clamp_position(position_sec);
        // 放完之后拖进度条：重新装一次从那里放，而不是报「跳转失败」
        if self.replay_if_ended(target_sec)? {
            return Ok(());
        }

        let mut last_err = None;
        for candidate in seek_candidates(target_sec) {
            match self.player.try_seek(Duration::from_secs_f64(candidate)) {
                Ok(()) => {
                    self.shared.set_position_sec(candidate);
                    return Ok(());
                }
                Err(err) => last_err = Some(err),
            }
        }

        // 只有「往结尾方向跳」才当作放完：中间跳不动说明文件真有问题，要让用户知道
        if self.near_end(target_sec) {
            self.player.clear();
            self.shared
                .set_position_sec(self.duration_sec().unwrap_or(target_sec));
            return Ok(());
        }
        Err(anyhow::anyhow!(
            "跳转失败: {}",
            last_err.map(|e| e.to_string()).unwrap_or_else(|| "未知原因".into())
        ))
    }

    fn duration_sec(&self) -> Option<f64> {
        self.shared
            .duration
            .lock()
            .ok()
            .and_then(|d| d.map(|d| d.as_secs_f64()))
    }

    /// 目标在不在「结尾那一段」里（时长未知时一律当作不是）
    fn near_end(&self, target_sec: f64) -> bool {
        match self.duration_sec() {
            Some(total) => target_sec >= total - SEEK_TAIL_SEC,
            None => false,
        }
    }

    /// 夹到这首歌真实存在的范围里。时长未知（某些流式 mp3）就只保证不小于 0
    fn clamp_position(&self, position_sec: f64) -> f64 {
        let position = position_sec.max(0.0);
        let duration = self
            .shared
            .duration
            .lock()
            .ok()
            .and_then(|d| d.map(|d| d.as_secs_f64()));
        match duration {
            Some(total) if total > SEEK_END_MARGIN => position.min(total - SEEK_END_MARGIN),
            _ => position,
        }
    }

    /// 设置倍速。**不改变音高**——走的是 WSOLA 时间伸缩。
    pub fn set_rate(&self, rate: f32) {
        self.shared.set_rate(clamp_rate(rate));
    }

    pub fn set_volume(&self, volume: f32) {
        self.player.set_volume(clamp_volume(volume));
    }

    /// 设置 A-B 循环。传 None 取消。
    ///
    /// 区间过短（< `MIN_LOOP_SEC`）时会被 `LoopRegion::new` 拒掉，
    /// 这里如实返回 false，让 UI 能提示而不是静默失败。
    pub fn set_loop(&self, region: Option<(f64, f64)>) -> bool {
        let parsed = match region {
            Some((a, b)) => match LoopRegion::new(a, b) {
                Some(r) => Some(r),
                None => return false,
            },
            None => None,
        };
        if let Ok(mut slot) = self.shared.loop_region.lock() {
            *slot = parsed;
            true
        } else {
            false
        }
    }

    /// 当前状态快照。前端轮询这个。
    pub fn state(&self) -> PlaybackState {
        let position_sec = self.shared.position_sec();
        let duration_sec = self
            .shared
            .duration
            .lock()
            .ok()
            .and_then(|d| d.map(|d| d.as_secs_f64()));

        let play_state = if !self.shared.loaded.load(Ordering::Relaxed) {
            PlayState::Empty
        } else if self.player.empty() {
            PlayState::Ended
        } else if self.player.is_paused() {
            PlayState::Paused
        } else {
            PlayState::Playing
        };

        PlaybackState {
            song_id: self
                .shared
                .song_id
                .lock()
                .map(|s| s.clone())
                .unwrap_or_default(),
            play_state,
            position_sec,
            duration_sec,
            rate: self.shared.rate(),
            volume: self.player.volume(),
            loop_region: self.shared.loop_region.lock().ok().and_then(|r| *r),
            pitch_semitones: 0,
            pitch_rendering: false,
            pitch_error: None,
        }
    }

    /// 取最近的采样窗，给频谱用。
    pub fn samples(&self, len: usize) -> Vec<f32> {
        self.tap.latest(len)
    }

    pub fn tap(&self) -> &Arc<TapBuffer> {
        &self.tap
    }
}

impl Drop for AudioEngine {
    /// 停掉看门狗**并等它真的停下来**，然后才让 `_device` 析构。
    ///
    /// 只置标志不 join 是不够的：看门狗线程拿着 `player`（它建自
    /// `device.mixer()`），而 `_device` 一 drop 音频流就关了。标志置上到线程
    /// 下一次醒来之间，它可能正好在 `player.try_seek()` 里——设备已经没了，
    /// 于是访问越界。
    ///
    /// 这不是只在测试里才会发生：应用退出时 `AppState` drop，走的是同一条路。
    /// 之所以先在 CI 上炸出来，是因为那边并行建了十个引擎又十个一起拆。
    fn drop(&mut self) {
        self.watcher_stop.store(true, Ordering::Relaxed);
        let handle = self.watcher.lock().ok().and_then(|mut slot| slot.take());
        if let Some(handle) = handle {
            // join 失败只说明看门狗自己 panic 过，那就没什么可等的了
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 这台机器到底有没有音频输出设备。**整个进程只试一次。**
    ///
    /// 这一条是 CI 上查了三轮才落到的点：在没有设备的 Windows runner 上，
    /// `open_default_sink()` 第一次老老实实返回 Err（于是测试正常跳过），
    /// **第二次直接把进程打成 STATUS_ACCESS_VIOLATION**。
    /// 日志里的样子是「跳过：打不开默认音频输出设备」、一个测试 ok、然后进程没了。
    ///
    /// 所以：试一次，失败就记住，之后谁都别再试。
    /// 这也正是生产的行为——`AppState::new` 只开一次，失败就把播放功能关掉，
    /// 不会重试。
    fn device_available() -> bool {
        static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *AVAILABLE.get_or_init(|| match AudioEngine::new() {
            Ok(_) => true,
            Err(err) => {
                eprintln!("[skip] 本机没有音频输出设备，引擎相关的用例全部跳过：{err}");
                false
            }
        })
    }

    /// 没有声卡就跳过。CI 和无头环境都没有。
    fn engine() -> Option<AudioEngine> {
        if !device_available() {
            return None;
        }
        match AudioEngine::new() {
            Ok(e) => Some(e),
            Err(err) => {
                eprintln!("[skip] 打不开音频设备：{err}");
                None
            }
        }
    }

    macro_rules! engine_or_skip {
        () => {
            match engine() {
                Some(e) => e,
                None => return,
            }
        };
    }

    #[test]
    fn fresh_engine_reports_empty() {
        let e = engine_or_skip!();
        let state = e.state();
        assert_eq!(state.play_state, PlayState::Empty);
        assert_eq!(state.song_id, "");
        assert_eq!(state.rate, 1.0);
        assert!(state.loop_region.is_none());
    }

    /// `drop` 返回时看门狗线程必须**已经退出**。
    ///
    /// 看门狗拿着 `player`（建自 `device.mixer()`），而 `_device` 一 drop
    /// 音频流就关了。只置停止标志不等它，就会有一段时间线程还在对着
    /// 已经没了的设备做 `try_seek`——CI 上十个引擎一起拆时炸成了
    /// STATUS_ACCESS_VIOLATION。
    ///
    /// 用 `Arc` 的强引用数来验：线程退出时会放掉它那一份，
    /// drop 之后还剩 2 份就说明没等。
    #[test]
    fn dropping_the_engine_waits_for_the_loop_watcher() {
        let e = engine_or_skip!();
        let flag = Arc::clone(&e.watcher_stop);
        drop(e);
        assert_eq!(
            Arc::strong_count(&flag),
            1,
            "drop 返回时看门狗线程还活着，它正拿着已经关掉的设备"
        );
    }

    #[test]
    fn rate_is_clamped_and_reported() {
        let e = engine_or_skip!();
        e.set_rate(0.75);
        assert_eq!(e.state().rate, 0.75);
        e.set_rate(99.0);
        assert_eq!(e.state().rate, crate::state::MAX_RATE);
    }

    #[test]
    fn volume_round_trips() {
        let e = engine_or_skip!();
        e.set_volume(0.4);
        assert!((e.state().volume - 0.4).abs() < 0.01);
    }

    #[test]
    fn loop_region_is_accepted_and_cleared() {
        let e = engine_or_skip!();
        assert!(e.set_loop(Some((10.0, 20.0))));
        assert_eq!(
            e.state().loop_region,
            Some(LoopRegion::new(10.0, 20.0).unwrap())
        );
        assert!(e.set_loop(None));
        assert!(e.state().loop_region.is_none());
    }

    #[test]
    fn degenerate_loop_is_rejected_not_silently_ignored() {
        let e = engine_or_skip!();
        assert!(!e.set_loop(Some((10.0, 10.01))), "过短的区间应当被拒绝");
        assert!(e.state().loop_region.is_none());
    }

    #[test]
    fn loading_a_missing_file_errors_instead_of_panicking() {
        let e = engine_or_skip!();
        assert!(e.load(Path::new("不存在的文件.flac"), "x").is_err());
        // 失败之后引擎仍然可用
        assert_eq!(e.state().play_state, PlayState::Empty);
    }

    /// 440 Hz 正弦，`seconds` 秒，双声道
    fn tone(sample_rate: u32, seconds: f32) -> rodio::buffer::SamplesBuffer {
        let frames = (sample_rate as f32 * seconds) as usize;
        let samples: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let v = (std::f32::consts::TAU * 440.0 * i as f32 / sample_rate as f32).sin() * 0.5;
                [v, v]
            })
            .collect();
        rodio::buffer::SamplesBuffer::new(ChannelCount::new(2).unwrap(), SampleRate::new(sample_rate).unwrap(), samples)
    }

    /// 结尾坏掉的文件：跳转要一步步往回退，而不是一次失败就报错
    #[test]
    fn seeking_falls_back_towards_earlier_positions() {
        // 歌中间：目标 + 三个回退点
        assert_eq!(seek_candidates(100.0), vec![100.0, 99.5, 98.0, 95.0]);
        // 靠开头：退成负数的那些不试
        assert_eq!(seek_candidates(1.0), vec![1.0, 0.5]);
        assert_eq!(seek_candidates(0.0), vec![0.0]);
        // 负数的输入夹到 0
        assert_eq!(seek_candidates(-5.0), vec![0.0]);
    }

    /// 前 `switch_sec` 秒 440 Hz，之后 880 Hz，双声道。
    /// 跳转之后听到的是哪一段，就知道有没有跳到该去的地方
    fn two_tone(sample_rate: u32, switch_sec: f32, total_sec: f32) -> rodio::buffer::SamplesBuffer {
        let frames = (sample_rate as f32 * total_sec) as usize;
        let switch = (sample_rate as f32 * switch_sec) as usize;
        let samples: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let hz = if i < switch { 440.0 } else { 880.0 };
                let v = (std::f32::consts::TAU * hz * i as f32 / sample_rate as f32).sin() * 0.5;
                [v, v]
            })
            .collect();
        rodio::buffer::SamplesBuffer::new(
            ChannelCount::new(2).unwrap(),
            SampleRate::new(sample_rate).unwrap(),
            samples,
        )
    }

    /// 用户报「改了倍速歌词对不上」：位置得按**歌里的秒数**走。
    ///
    /// rodio 的 `get_pos` 数的是送进声卡的样本（墙上时间），而 WSOLA 是时间伸缩——
    /// 1.5 倍速放 2 秒，歌里过去的是 3 秒。实测（见 `docs/tauri-shell.md`）旧实现
    /// 在 0.75×/1×/1.5× 下位置都按 1.0 倍速往前走，歌词自然越走越偏。
    #[test]
    fn position_counts_song_seconds_not_wall_clock() {
        const RATE: u32 = 44_100;
        for speed in [0.75f32, 1.0, 1.5, 2.0] {
            let shared = Arc::new(Shared::new());
            shared.set_rate(speed);
            let mut source = RateControlled::new(two_tone(RATE, 30.0, 60.0), Arc::clone(&shared));
            // 拉 1 秒的输出（双声道）
            for _ in 0..RATE * 2 {
                source.next();
            }
            let got = shared.position_sec();
            assert!(
                (got - f64::from(speed)).abs() < 0.02,
                "{speed}× 放 1 秒输出，歌里应当过去 {speed} 秒，实际 {got:.3}"
            );
        }
    }

    /// 跳转的参数是**歌里**的时刻。`Wsola::try_seek` 会把它当输出时间再乘一次倍速，
    /// 不先除回去的话，1.5 倍速拖到 3:00 会去找 4:30——超过结尾就直接播完，
    /// 用户看到的就是「拖一下卡住」。
    #[test]
    fn seeking_lands_on_the_requested_song_second_at_any_speed() {
        const RATE: u32 = 44_100;
        for speed in [1.0f32, 1.5, 2.0] {
            let shared = Arc::new(Shared::new());
            shared.set_rate(speed);
            // 30 秒处从 440 Hz 换成 880 Hz 的 60 秒音频
            let mut source = RateControlled::new(two_tone(RATE, 30.0, 60.0), Arc::clone(&shared));
            source
                .try_seek(Duration::from_secs_f64(40.0))
                .unwrap_or_else(|e| panic!("{speed}× 跳转失败: {e}"));
            assert!(
                (shared.position_sec() - 40.0).abs() < 1e-6,
                "{speed}× 跳转后位置应当是 40 秒，实际 {:.3}",
                shared.position_sec()
            );
            // 跳到了 40 秒（> 30 秒的分界），听到的应当是 880 Hz 而不是静音或 440 Hz
            let out: Vec<f32> = (0..RATE / 2 * 2).map(|_| source.next().unwrap_or(0.0)).collect();
            let hz = frequency(&out, RATE);
            assert!(
                (hz - 880.0).abs() < 25.0,
                "{speed}× 跳到 40 秒应当听到 880 Hz，实际 {hz:.1} Hz"
            );
        }
    }

    /// 左声道过零次数估频率
    fn frequency(interleaved: &[f32], sample_rate: u32) -> f64 {
        let left: Vec<f32> = interleaved.iter().step_by(2).copied().collect();
        let crossings = left.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        crossings as f64 / 2.0 / (left.len() as f64 / sample_rate as f64)
    }

    /// 不开声卡：Player 接在一个 48 kHz 的 mixer 上，手动从 mixer 拉样本，
    /// 连续播两首采样率不同的歌，量输出的音高。
    fn pitch_of_two_songs_in_a_row(first_rate: u32, second_rate: u32) -> (f64, f64) {
        const OUTPUT_RATE: u32 = 48_000;
        let format = OutputFormat {
            channels: ChannelCount::new(2).unwrap(),
            sample_rate: SampleRate::new(OUTPUT_RATE).unwrap(),
        };
        let (mixer, mut output) = rodio::mixer::mixer(format.channels, format.sample_rate);
        let player = Player::connect_new(&mixer);
        let shared = Arc::new(Shared::new());
        let tap = TapBuffer::new(1024);
        player.append(playback_chain(tone(first_rate, 1.0), &shared, &tap, format));
        player.append(playback_chain(tone(second_rate, 1.0), &shared, &tap, format));

        let pull = |seconds: f32, output: &mut rodio::mixer::MixerSource| -> Vec<f32> {
            (0..(OUTPUT_RATE as f32 * seconds) as usize * 2).map(|_| output.next().unwrap_or(0.0)).collect()
        };
        let _ = pull(0.2, &mut output);
        let first = pull(0.5, &mut output);
        let _ = pull(0.5, &mut output);
        let second = pull(0.5, &mut output);
        (frequency(&first, OUTPUT_RATE), frequency(&second, OUTPUT_RATE))
    }

    /// 用户报告「有些歌没动就升调或降调了」：曲库里 44.1 kHz 和 48 kHz 混着，
    /// 后播的歌不能沿用前一首的采样率去换算
    #[test]
    fn songs_with_different_sample_rates_keep_their_pitch() {
        for (a, b) in [(44_100, 48_000), (48_000, 44_100)] {
            let (first, second) = pitch_of_two_songs_in_a_row(a, b);
            assert!((first - 440.0).abs() < 3.0, "{a} Hz 后接 {b} Hz：第一首 {first:.1} Hz");
            assert!((second - 440.0).abs() < 3.0, "{a} Hz 后接 {b} Hz：第二首 {second:.1} Hz");
        }
    }

    /// 拿真实文件核对：`JP_AUDIO_PAIR="先播的文件|后播的文件"`。
    /// 后播那首单独播时输出的一段，和接在前一首后面播时的输出对齐后比波形：速度对，相关系数接近 1；
    /// 速度差 8.8%，半秒里就错开几十毫秒，相关系数很低。旧的播放链（不先换算格式）也跑一遍作对照。
    #[test]
    #[ignore = "需要本机音频文件"]
    fn real_songs_in_a_row_play_at_the_right_speed() {
        const OUTPUT_RATE: usize = 48_000;
        let pair = std::env::var("JP_AUDIO_PAIR").expect("JP_AUDIO_PAIR=先播|后播");
        let (first_path, second_path) = pair.split_once('|').unwrap();
        let open = |path: &str, seconds: u64| {
            Decoder::try_from(File::open(path).unwrap()).unwrap().take_duration(Duration::from_secs(seconds))
        };
        let format = OutputFormat {
            channels: ChannelCount::new(2).unwrap(),
            sample_rate: SampleRate::new(OUTPUT_RATE as u32).unwrap(),
        };

        // 左声道输出的 [from, from + len) 秒
        let render = |preceded: bool, convert: bool, from: f64, len: f64| -> Vec<f32> {
            let (mixer, output) = rodio::mixer::mixer(format.channels, format.sample_rate);
            let player = Player::connect_new(&mixer);
            let shared = Arc::new(Shared::new());
            let tap = TapBuffer::new(1024);
            let append = |source: rodio::source::TakeDuration<Decoder<std::io::BufReader<File>>>| {
                if convert {
                    player.append(playback_chain(source, &shared, &tap, format));
                } else {
                    player.append(Tapped::new(RateControlled::new(source, Arc::clone(&shared)), Arc::clone(&tap)));
                }
            };
            if preceded {
                append(open(first_path, 2));
            }
            append(open(second_path, 10));
            let skip = (from * OUTPUT_RATE as f64) as usize * 2;
            let take = (len * OUTPUT_RATE as f64) as usize * 2;
            output.skip(skip).take(take).step_by(2).collect()
        };

        // 后播那首第 4 秒起的半秒，在接着播的输出里前后 0.15 秒找最像的位置
        const SEARCH: f64 = 0.15;
        for (name, convert) in [("旧播放链", false), ("新播放链", true)] {
            let alone = render(false, convert, 4.0, 0.5);
            let after = render(true, convert, 2.0 + 4.0 - SEARCH, 0.5 + 2.0 * SEARCH);
            let energy = |xs: &[f32]| xs.iter().map(|x| (*x as f64).powi(2)).sum::<f64>();
            let (best_lag, best) = (0..after.len() - alone.len())
                .map(|lag| {
                    let window = &after[lag..lag + alone.len()];
                    let dot: f64 = window.iter().zip(&alone).map(|(a, b)| *a as f64 * *b as f64).sum();
                    (lag, dot / (energy(window) * energy(&alone)).sqrt())
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap();
            let shift_ms = (best_lag as f64 / OUTPUT_RATE as f64 - SEARCH) * 1000.0;
            println!("{name}：最像的位置偏 {shift_ms:+.1} ms，相关系数 {best:.4}");
            if convert {
                assert!(best > 0.99, "新播放链的相关系数只有 {best}");
            }
        }
    }

    /// 写一个 `seconds` 秒的静音 WAV（16 bit 双声道 44.1 kHz）
    fn silent_wav(seconds: u32) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("jp-audio-silence-{}-{seconds}.wav", std::process::id()));
        let data_len = 44_100 * 2 * 2 * seconds;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&44_100u32.to_le_bytes());
        bytes.extend_from_slice(&(44_100u32 * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.resize(bytes.len() + data_len as usize, 0);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn load_at_resumes_position_and_pause_state_and_keeps_the_loop_for_the_same_song() {
        let e = engine_or_skip!();
        let file = silent_wav(6);
        e.load(&file, "song").unwrap();
        assert!(e.set_loop(Some((1.0, 3.0))));

        e.load_at(&file, "song", 2.5, false).unwrap();
        let state = e.state();
        assert_eq!(state.play_state, PlayState::Paused);
        assert!((state.position_sec - 2.5).abs() < 0.2, "位置 {}", state.position_sec);
        assert!(state.loop_region.is_some(), "同一首歌换版本不该丢掉单句循环");

        e.load_at(&file, "other", 0.0, true).unwrap();
        assert_eq!(e.state().play_state, PlayState::Playing);
        assert!(e.state().loop_region.is_none(), "换歌要清掉循环");
        e.stop();
        let _ = std::fs::remove_file(file);
    }

    /// 换一首歌，位置要从头开始数。
    ///
    /// 以前 `RateControlled::new` 把 `shared` 里的位置抄进新 source 当起点，
    /// 于是播到 4:08 切下一首，新歌的位置报 4:08 并接着往上数：
    /// 进度条顶在最右边，歌词跟随把整屏歌词滚到最后一行
    /// （用户报的「播完一首切另一首，歌词总跑到最下面」）。
    #[test]
    fn switching_songs_restarts_the_position_at_zero() {
        let e = engine_or_skip!();
        // 时长各不相同：silent_wav 的文件名只带秒数，和别的测试撞名字时
        // 会被对方删掉（并行跑的时候），表现为「打不开音频文件」
        let first = silent_wav(9);
        let second = silent_wav(10);
        e.load(&first, "A").unwrap();
        e.seek(4.0).unwrap();
        std::thread::sleep(Duration::from_millis(250));
        assert!(e.state().position_sec > 3.5, "前提：A 已经放到靠后的位置");

        e.load(&second, "B").unwrap();
        std::thread::sleep(Duration::from_millis(250));
        let state = e.state();
        assert_eq!(state.song_id, "B");
        assert!(
            state.position_sec < 1.0,
            "换歌之后位置应当从头数，实际 {}",
            state.position_sec
        );

        e.stop();
        let _ = std::fs::remove_file(first);
        let _ = std::fs::remove_file(second);
    }

    /// 等到这首播完（没有声卡时上面已经跳过了）
    fn wait_until_ended(engine: &AudioEngine, seconds: f64) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs_f64(seconds);
        while std::time::Instant::now() < deadline {
            if engine.state().play_state == PlayState::Ended {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn a_finished_song_can_be_dragged_back_and_replayed() {
        let e = engine_or_skip!();
        let file = silent_wav(1);
        e.load(&file, "song").unwrap();
        assert!(wait_until_ended(&e, 6.0), "一秒的音频应当放完");

        // 拖进度条：以前 try_seek 作用在空队列上，报「跳转失败」，位置也不动
        e.seek(0.3).unwrap();
        let state = e.state();
        assert_eq!(state.play_state, PlayState::Playing, "拖回去应当接着播");
        assert_eq!(state.song_id, "song");
        assert!((state.position_sec - 0.3).abs() < 0.25, "位置 {}", state.position_sec);

        // 放完之后按播放：从头再来
        assert!(wait_until_ended(&e, 6.0));
        e.play();
        assert_eq!(e.state().play_state, PlayState::Playing, "放完之后按播放应当重播");

        // 停掉之后不该再自己复活
        e.stop();
        assert_eq!(e.state().play_state, PlayState::Empty);
        e.play();
        assert_eq!(e.state().play_state, PlayState::Empty, "停了就是停了");
        assert!(e.seek(1.0).is_err() || e.state().play_state == PlayState::Empty);
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn tap_starts_silent() {
        let e = engine_or_skip!();
        assert!(e.samples(256).iter().all(|&s| s == 0.0));
    }
}
