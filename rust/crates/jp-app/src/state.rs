//! 应用状态。
//!
//! **为什么是 `Mutex<Corpus>` 而不是直接放进 managed state**：
//! `rusqlite::Connection` 是 `Send` 但不是 `Sync`（内部有语句缓存的
//! `RefCell`），而 Tauri 的 managed state 要求 `Send + Sync`。
//! `Mutex<T>` 在 `T: Send` 时就是 `Sync`，正好补上。
//!
//! 代价是所有查询串行化。实测单条查询 0.3~3.5ms，桌面端够用了。
//! 真到了要并发的时候，换成连接池（每个连接一个 `Corpus`）即可，
//! command 层的签名不用动。

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{Context, Result};
use jp_audio::{AudioEngine, SpectrumAnalyzer};
use jp_corpus::Corpus;
use jp_tokenizer::Analyzer;

use crate::tracker::PlayTracker;

pub struct AppState {
    pub db_path: PathBuf,
    /// 封面落盘目录，和 Python 侧的 project_paths.COVERS_DIR 一致
    pub covers_dir: PathBuf,
    /// 歌手照片目录，对应 project_paths.ARTISTS_DIR
    pub artists_dir: PathBuf,
    /// 批量刮削作业的运行/取消标志
    scrape_job: std::sync::Arc<crate::scrape::ScrapeJob>,
    /// Anki 导出作业。**和刮削分开**——刮削排的是外部服务的限流队列，
    /// Anki 是本机，两件事同时做没有冲突。
    anki_job: std::sync::Arc<crate::anki::ExportJob>,
    corpus: Mutex<Corpus>,
    /// 分词器是可选的：没有词典时（比如用户没装 Python venv）
    /// 其余功能仍然可用，只有「实时分词」这一个能力降级。
    analyzer: Option<Analyzer>,
    /// 音频引擎同样是可选的：没有声卡（远程桌面、CI、无声卡的虚拟机）
    /// 时应用照常能浏览歌词和语料，只是不能播。Arc 是因为变调渲染线程渲染完要换文件。
    audio: Option<std::sync::Arc<AudioEngine>>,
    /// 变调：渲染、缓存、换文件
    pitch: crate::pitch::PitchControl,
    /// 频谱分析器持有 FFT 计划，构造不便宜，所以缓存下来复用。
    spectrum: Mutex<SpectrumAnalyzer>,
    /// 收听会话统计。由前端的轮询驱动（见 `commands::audio_tick`）。
    tracker: Mutex<PlayTracker>,
    /// 上次 tick 的时刻，用来算墙钟间隔。
    last_tick: Mutex<Option<Instant>>,
    /// 扫描出来、等着确认导入的条目。
    ///
    /// 存的是扫描结果而不是计划：真正导入时会拿当前曲库重新算一遍计划，
    /// 这样即使扫描之后库又变了（比如 PyQt 那边加了歌），id 分配和判重
    /// 也是对的。复核看到的计划是参考，报告里写的才是实际做了什么。
    pending_scan: Mutex<Vec<jp_import::ScannedTrack>>,
    /// 词典库 dictionaries.db：和 corpus.db 放在同一目录、分开存。
    dictionaries_path: PathBuf,
    /// **懒打开**。没用到词典功能就不在项目目录里新建文件——集成测试也拿真实项目目录装配状态。
    dictionaries: Mutex<Option<jp_dict::store::DictionaryStore>>,
    /// 活用规则表只解析一次，之后每次查词复用。
    translator: jp_dict::translator::Translator,
    /// 词典导入作业。和 Anki、刮削各自独立。
    dict_job: std::sync::Arc<crate::dict::ImportJob>,
}

impl AppState {
    pub fn new(project_root: &std::path::Path) -> Result<Self> {
        let db_path = project_root.join("corpus.db");
        let covers_dir = project_root.join("raw").join("covers");
        let artists_dir = project_root.join("raw").join("artists");
        let dictionaries_path = project_root.join("dictionaries.db");
        let corpus = Corpus::open_writable(&db_path)
            .with_context(|| format!("打不开 {}", db_path.display()))?;
        // 新装的程序第一次启动时这里还是个空文件，先把表建齐；已有的库是空操作
        corpus.ensure_schema()?;
        corpus.check_schema()?;

        let analyzer = jp_tokenizer::locate_sudachipy(project_root)
            .and_then(|(res, dict)| Analyzer::from_sudachipy(&res, &dict).ok());
        if analyzer.is_none() {
            eprintln!("[warn] 找不到 Sudachi 词典，实时分词不可用（其余功能正常）");
        }

        let audio = match AudioEngine::new() {
            Ok(engine) => Some(std::sync::Arc::new(engine)),
            Err(err) => {
                eprintln!("[warn] 音频引擎不可用（{err}），播放功能关闭，其余正常");
                None
            }
        };

        Ok(Self {
            db_path,
            covers_dir,
            artists_dir,
            scrape_job: std::sync::Arc::new(crate::scrape::ScrapeJob::default()),
            anki_job: std::sync::Arc::new(crate::anki::ExportJob::default()),
            corpus: Mutex::new(corpus),
            analyzer,
            audio,
            pitch: crate::pitch::PitchControl::new(std::sync::Arc::new(
                crate::pitch::FfmpegRenderer {
                    ffmpeg: jp_anki::audio::find_ffmpeg(project_root),
                    // 和 Python 版同一个目录、同样的文件名，两边的缓存互用
                    cache_dir: project_root.join("output").join("pitch_cache"),
                },
            )),
            spectrum: Mutex::new(SpectrumAnalyzer::new(
                jp_audio::DEFAULT_WINDOW,
                jp_audio::DEFAULT_BANDS,
                44_100,
            )),
            tracker: Mutex::new(PlayTracker::new("app")),
            last_tick: Mutex::new(None),
            pending_scan: Mutex::new(Vec::new()),
            dictionaries_path,
            dictionaries: Mutex::new(None),
            translator: jp_dict::translator::Translator::new(),
            dict_job: std::sync::Arc::new(crate::dict::ImportJob::default()),
        })
    }

    /// 借出语料库。锁中毒时直接恢复——一次查询 panic 不该让整个应用
    /// 从此拒绝服务。
    pub fn corpus(&self) -> std::sync::MutexGuard<'_, Corpus> {
        self.corpus.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn pending_scan(&self) -> std::sync::MutexGuard<'_, Vec<jp_import::ScannedTrack>> {
        self.pending_scan.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn scrape_job(&self) -> std::sync::Arc<crate::scrape::ScrapeJob> {
        self.scrape_job.clone()
    }

    pub fn anki_job(&self) -> std::sync::Arc<crate::anki::ExportJob> {
        self.anki_job.clone()
    }

    pub fn dict_job(&self) -> std::sync::Arc<crate::dict::ImportJob> {
        self.dict_job.clone()
    }

    pub fn dictionaries_path(&self) -> &std::path::Path {
        &self.dictionaries_path
    }

    pub fn translator(&self) -> &jp_dict::translator::Translator {
        &self.translator
    }

    /// 借出词典库。库文件还不存在、且 `create` 为假时返回 `Ok(None)`：
    /// 还没导入过词典，列表和查词都该是空的，不必为此在用户目录里建一个空库。
    pub fn with_dictionaries<T>(
        &self,
        create: bool,
        f: impl FnOnce(&mut jp_dict::store::DictionaryStore) -> Result<T>,
    ) -> Result<Option<T>> {
        let mut guard = self.dictionaries.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            if !create && !self.dictionaries_path.exists() {
                return Ok(None);
            }
            *guard = Some(jp_dict::store::DictionaryStore::open(
                &self.dictionaries_path,
            )?);
        }
        match guard.as_mut() {
            Some(store) => f(store).map(Some),
            None => Ok(None),
        }
    }

    pub fn analyzer(&self) -> Option<&Analyzer> {
        self.analyzer.as_ref()
    }

    pub fn audio(&self) -> Option<&AudioEngine> {
        self.audio.as_deref()
    }

    /// 交给变调控制用的播放器句柄
    pub fn deck(&self) -> Option<std::sync::Arc<dyn crate::pitch::Deck>> {
        self.audio
            .clone()
            .map(|engine| engine as std::sync::Arc<dyn crate::pitch::Deck>)
    }

    pub fn pitch(&self) -> &crate::pitch::PitchControl {
        &self.pitch
    }

    /// 引擎状态加上变调状态
    pub fn playback_state(&self) -> jp_audio::PlaybackState {
        let Some(engine) = self.audio.as_ref() else {
            return jp_audio::PlaybackState::default();
        };
        let mut state = engine.state();
        let pitch = self.pitch.status();
        state.pitch_semitones = pitch.semitones;
        state.pitch_rendering = pitch.rendering;
        state.pitch_error = pitch
            .error
            .map(|(id, message)| jp_audio::PitchError { id, message });
        state
    }

    pub fn spectrum(&self) -> std::sync::MutexGuard<'_, SpectrumAnalyzer> {
        self.spectrum.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 走一拍收听统计：读状态 → 喂 tracker →（有事件就）落库。
    ///
    /// 墙钟间隔在这里算，而不是让调用方传——调用方（前端轮询）报的
    /// 间隔不可靠，窗口最小化时浏览器会节流定时器。
    pub fn tick(&self) -> jp_audio::PlaybackState {
        if self.audio.is_none() {
            return jp_audio::PlaybackState::default();
        }
        let state = self.playback_state();

        let elapsed = {
            let mut last = self.last_tick.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let delta = last
                .map(|t| now.duration_since(t).as_secs_f64())
                .unwrap_or(0.0);
            *last = Some(now);
            delta
        };

        let event = {
            let mut tracker = self.tracker.lock().unwrap_or_else(|e| e.into_inner());
            tracker.observe(&state, elapsed)
        };
        if let Some(event) = event {
            // 写历史失败不该影响播放，记一笔日志就够了
            if let Err(err) = self.corpus().record_play(&event) {
                eprintln!("[warn] 播放历史写入失败: {err}");
            }
        }
        state
    }

    /// 冲刷未结束的收听会话。应用退出前调。
    pub fn flush_session(&self) {
        let event = {
            let mut tracker = self.tracker.lock().unwrap_or_else(|e| e.into_inner());
            tracker.flush()
        };
        if let Some(event) = event
            && let Err(err) = self.corpus().record_play(&event)
        {
            eprintln!("[warn] 退出时播放历史写入失败: {err}");
        }
    }
}
