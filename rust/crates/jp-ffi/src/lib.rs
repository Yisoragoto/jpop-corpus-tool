//! 给原生 macOS 客户端（SwiftUI）用的 C ABI。
//!
//! ## 为什么是「一个入口 + JSON」
//!
//! 业务逻辑在共享 Rust crate，Tauri 经 command 调用，这里给 Swift 开同样的口子。
//! 没有逐个函数导出 C 签名，也没用 UniFFI 之类的代码生成：
//!
//! - **两个客户端看到的是同一份数据形状。** 参数和返回值都是 serde 序列化的 JSON，
//!   字段名就是 Tauri 前端用的那套 camelCase。Rust 结构体加一个字段，两边自动都有，
//!   不会出现「Tauri 有、Swift 的绑定忘了重新生成」。
//! - **C 层只有五个函数，几乎不会变。** 新功能只在 [`dispatch`] 里加一个分支，
//!   头文件、Swift 的模块映射都不用动。
//! - **代价**是每次调用多一次 JSON 编解码。语料库查询本身是毫秒到几十毫秒，
//!   编解码是微秒级，可以忽略；音频播放不走这里（macOS 用 AVFoundation 原生播放）。
//!
//! ## 约定
//!
//! - 所有字符串都是 UTF-8、以 NUL 结尾。Rust 返回的字符串由调用方用 [`jp_string_free`] 释放。
//! - [`jp_call`] 返回 `{"ok": <结果>}` 或 `{"error": "<一句人话>"}`，**从不返回 NULL**（内存分配失败除外）。
//! - 一个会话一个 SQLite 连接，内部加锁；Swift 可以从任意线程调，用到这条连接的调用是串行的。
//!   不碰这条连接的方法（作业进度、分词词典、查词、扫描目录）不拿这把锁，导入跑着时照样能问进度。
//! - 长作业（导入、词典、刮削、歌词）在后台线程上跑，用自己的连接，界面轮询进度（见 [`jobs`]）。
//! - panic 不会跨过 FFI 边界：被接住，变成一条 error。
//!
//! 头文件在 `include/jp_ffi.h`，手写，和这里的五个函数一一对应。

pub mod jobs;
mod anki;
mod system;
#[cfg(test)]
mod anki_tests;
mod lyrics;
mod library;
#[cfg(test)]
mod library_tests;
#[cfg(test)]
mod lyrics_tests;
#[cfg(test)]
mod import_tests;

use std::ffi::{CStr, CString, c_char};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, ensure};
use jp_corpus::Corpus;
use jp_tokenizer::Analyzer;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::jobs::Job;

/// 两条连接（会话那条、作业那条）同时想写时，后来的那个等多久。
/// 一首歌一个事务，几十毫秒就提交了；给足余量，不让「记一次播放」撞上导入就报错
const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

/// 一个打开的语料库。对 C 来说是不透明指针。
pub struct Session {
    corpus: Mutex<Corpus>,
    /// `corpus.db` 本身。后台作业用它自己开连接
    db_path: PathBuf,
    /// 语料库目录（`corpus.db` 所在的目录）。相对路径和按文件名找音频都从这里开始
    root: Option<PathBuf>,
    /// 用户另外指定的音频目录。库从别的机器拷来时，音频的绝对路径在这台机器上不存在
    audio_roots: Mutex<Vec<PathBuf>>,
    /// 分词器。第一次要用时才加载（207MB 的词典，没用到就别占内存）
    analyzer: Arc<AnalyzerSlot>,
    /// 扫描过、等确认的那一批。存扫描结果而不是计划：导入时按当时的库重新算
    pending_scan: Arc<Mutex<Vec<jp_import::ScannedTrack>>>,
    import_job: Arc<Job>,
    tokenizer_job: Arc<Job>,
    /// 导入 Yomitan 词典（zip）
    dict_job: Arc<Job>,
    /// 批量刮削（曲目、歌手共用：两边都排 MusicBrainz 的队，同时跑只会互相拖慢）
    scrape_job: Arc<Job>,
    lyrics_job: Arc<Job>,
    library_job: Arc<Job>,
    anki_job: Arc<Job>,
    lyric_batch: Arc<jp_lyrics::lyric_upgrade::Batch>,
    lyric_sources: OnceLock<Mutex<jp_lyrics::timed_lyrics::Session>>,
    /// Yomitan 词典库 `dictionaries.db`，和 corpus.db 同一目录、分开存。没导入过就一直是 None
    dictionaries: Mutex<Option<jp_dict::store::DictionaryStore>>,
    translator: jp_dict::translator::Translator,
}

/// 分词器的三种状态：还没找过（外层 None）、找过但没有、有。
#[derive(Default)]
struct AnalyzerSlot(Mutex<Option<Option<Arc<Analyzer>>>>);

impl AnalyzerSlot {
    fn get(&self, root: &Path) -> Option<Arc<Analyzer>> {
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(load_analyzer(root));
        }
        slot.clone().flatten()
    }

    /// 重新找一次（装好词典之后）。返回现在有没有
    fn reload(&self, root: &Path) -> bool {
        let loaded = load_analyzer(root);
        let ready = loaded.is_some();
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(loaded);
        ready
    }
}

/// 和 Tauri 客户端同一个找法：语料库目录里的 `sudachi/`（或 0.1.x 的 venv），再是随程序带的
fn load_analyzer(root: &Path) -> Option<Arc<Analyzer>> {
    jp_tokenizer::locate_sudachipy(root)
        .and_then(|(res, dict)| Analyzer::from_sudachipy(&res, &dict).ok())
        .map(Arc::new)
}

/// 后台作业自己的连接。结构已经由会话打开时迁移好了，这里只开、只设等待
fn open_worker(db_path: &Path) -> Result<Corpus> {
    let corpus = Corpus::open_writable(db_path).with_context(|| format!("打不开 {}", db_path.display()))?;
    corpus.connection().busy_timeout(BUSY_TIMEOUT)?;
    Ok(corpus)
}

impl Session {
    /// 打开（必要时迁移）一个语料库。和 Tauri 客户端启动时做的一样：
    /// 可写打开、补齐结构、检查结构；比本程序新的库直接拒绝（见 `jp_corpus::migrations`）。
    pub fn open(db_path: &str) -> Result<Self> {
        let path = PathBuf::from(db_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut corpus = Corpus::open_writable(&path).with_context(|| format!("打不开 {}", path.display()))?;
        corpus.migrate().with_context(|| format!("{} 的结构升不上来", path.display()))?;
        corpus.check_schema()?;
        corpus.connection().busy_timeout(BUSY_TIMEOUT)?;
        // 和 Tauri 客户端启动时一样：来源追踪之前导进来的歌补登记一版歌词（docs/provenance.md）。
        // 补不上不影响打开，质量清单里那些歌只是没有版本记录
        let _ = corpus.backfill_lineage();
        let session = Self {
            corpus: Mutex::new(corpus),
            root: path.parent().map(PathBuf::from),
            db_path: path,
            audio_roots: Mutex::new(Vec::new()),
            analyzer: Arc::default(),
            pending_scan: Arc::default(),
            import_job: Arc::default(),
            tokenizer_job: Arc::default(),
            dict_job: Arc::default(),
            scrape_job: Arc::default(),
            lyrics_job: Arc::default(),
            library_job: Arc::default(),
            anki_job: Arc::default(),
            lyric_batch: Arc::default(),
            lyric_sources: OnceLock::new(),
            dictionaries: Mutex::new(None),
            translator: jp_dict::translator::Translator::new(),
        };
        jp_diagnostics::log::init(&session.library_root());
        jp_lyrics::set_logger(|level, message| match level {
            jp_lyrics::LogLevel::Warn => jp_diagnostics::log::warn(message),
            jp_lyrics::LogLevel::Error => jp_diagnostics::log::error(message),
        });
        jp_diagnostics::log::info("打开 macOS 语料库");
        session.lyric_sources();
        Ok(session)
    }

    /// 语料库目录。分词词典（`sudachi/`）、Yomitan 词典库都在这里
    fn library_root(&self) -> PathBuf {
        match &self.root {
            Some(root) if !root.as_os_str().is_empty() => root.clone(),
            _ => PathBuf::from("."),
        }
    }

    fn lyric_sources(&self) -> &Mutex<jp_lyrics::timed_lyrics::Session> {
        self.lyric_sources.get_or_init(|| Mutex::new(jp_lyrics::timed_lyrics::Session::load(&self.library_root())))
    }

    /// 刮下来的封面、歌手照片放哪（和 Windows 版一样：`raw/covers`、`raw/artists`）
    fn covers_dir(&self) -> PathBuf {
        self.library_root().join("raw").join("covers")
    }

    fn artists_dir(&self) -> PathBuf {
        self.library_root().join("raw").join("artists")
    }

    /// 要联网的单次操作用的连接：自己开一条，不占会话那条（识别要排 MusicBrainz 的队，封面一张十几秒）
    fn network_connection(&self) -> Result<Corpus> {
        open_worker(&self.db_path)
    }

    /// Yomitan 词典库的位置：语料库目录里的 `dictionaries.db`（和 Windows 版一样）
    fn dictionaries_path(&self) -> PathBuf {
        self.library_root().join("dictionaries.db")
    }

    /// 词典库被别的连接改过（导入、删除、排序）之后，丢掉缓存的那条连接，下次查词重新打开
    fn forget_dictionaries(&self) {
        *self.dictionaries.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// 改词典库（启用、排序、删除）：单开一条可写连接，改完让查词那条重新打开
    fn edit_dictionaries(&self, f: impl FnOnce(&mut jp_dict::store::DictionaryStore) -> Result<()>) -> Result<()> {
        ensure!(!self.dict_job.view().running, "正在导入词典，等导入完成再改");
        let path = self.dictionaries_path();
        ensure!(path.exists(), "还没有导入过词典");
        let mut store = jp_dict::store::DictionaryStore::open(&path)?;
        f(&mut store)?;
        self.forget_dictionaries();
        Ok(())
    }

    fn analyzer(&self) -> Option<Arc<Analyzer>> {
        self.analyzer.get(&self.library_root())
    }

    /// 借出词典库。库文件还不存在时返回 `Ok(None)`：没导入过词典，不必为此建一个空库
    fn with_dictionaries<T>(
        &self,
        f: impl FnOnce(&jp_dict::store::DictionaryStore) -> Result<T>,
    ) -> Result<Option<T>> {
        let mut guard = self.dictionaries.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            let path = self.dictionaries_path();
            if !path.exists() {
                return Ok(None);
            }
            *guard = Some(jp_dict::store::DictionaryStore::open(&path)?);
        }
        match guard.as_ref() {
            Some(store) => f(store).map(Some),
            None => Ok(None),
        }
    }

    /// 调一个方法。参数和返回值都是 JSON。纯 Rust，测试直接调它
    pub fn call(&self, method: &str, args: Value) -> Result<Value> {
        dispatch(self, method, args)
    }
}

fn arg<T: for<'de> Deserialize<'de>>(args: &Value, key: &str) -> Result<T> {
    let value = args.get(key).cloned().unwrap_or(Value::Null);
    serde_json::from_value(value).with_context(|| format!("参数 {key} 不对"))
}

fn arg_or<T: for<'de> Deserialize<'de>>(args: &Value, key: &str, default: T) -> Result<T> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => serde_json::from_value(v.clone()).with_context(|| format!("参数 {key} 不对")),
    }
}

/// 所有方法。名字和 Tauri command 对齐（camelCase），参数名也一样
/// 存档里记的程序版本：客户端传了 `appVersion` 就用它（macOS 应用的版本），否则记核心的版本
fn app_version(args: &Value) -> String {
    args.get("appVersion")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("core {}", env!("CARGO_PKG_VERSION")))
}

fn dispatch(session: &Session, method: &str, args: Value) -> Result<Value> {
    if let Some(value) = dispatch_unlocked(session, method, &args)? {
        return Ok(value);
    }
    let corpus = session.corpus.lock().map_err(|_| anyhow!("语料库连接已损坏"))?;
    let v = match method {
        "version" => json!({ "core": env!("CARGO_PKG_VERSION"), "schema": jp_corpus::migrations::CURRENT }),
        "health" => {
            let overview = corpus.overview()?;
            json!({
                "tracks": overview.tracks,
                "lyricLines": overview.lyric_lines,
                "dbPath": session.db_path.display().to_string(),
                "tokenizerReady": session.analyzer().is_some(),
                "audioReady": arg_or(&args, "audioAvailable", false)?,
                "tokens": overview.tokens,
                "root": session.root.as_ref().map(|p| p.display().to_string()),
            })
        }
        "overview" => serde_json::to_value(corpus.overview()?)?,
        "snapshot" => serde_json::to_value(corpus.snapshot()?)?,

        // 曲库
        "tracks" => serde_json::to_value(corpus.tracks(arg_or(&args, "limit", 100_000)?)?)?,
        "track" => serde_json::to_value(corpus.track(&arg::<String>(&args, "songId")?)?)?,
        "lyrics" => serde_json::to_value(corpus.lyrics(&arg::<String>(&args, "songId")?)?)?,
        "lyricTimeline" => serde_json::to_value(corpus.lyric_timeline(&arg::<String>(&args, "songId")?)?)?,
        "tokenizeMissingList" => serde_json::to_value(jp_import::maintain::untokenized_songs(corpus.connection())?)?,
        "trackCredits" => serde_json::to_value(corpus.credits_for_track(&arg::<String>(&args, "songId")?)?)?,
        "performers" => serde_json::to_value(corpus.people_by_role("performer", arg_or(&args, "limit", 500)?)?)?,
        "albums" => serde_json::to_value(corpus.albums(
            arg_or::<Option<String>>(&args, "artistKey", None)?.as_deref(),
            arg_or(&args, "limit", 2000)?,
        )?)?,
        "albumTracks" => serde_json::to_value(corpus.album_tracks(arg(&args, "albumId")?)?)?,
        "peopleDirectory" => serde_json::to_value(corpus.people_directory()?)?,
        "collaborationEdges" => serde_json::to_value(corpus.collaboration_edges()?)?,
        "timelineReleases" => serde_json::to_value(corpus.timeline_releases()?)?,
        "timeline" => serde_json::to_value(corpus.timeline()?)?,
        "personById" => serde_json::to_value(corpus.person_by_id(arg(&args, "personId")?)?)?,
        "personWorks" => serde_json::to_value(corpus.person_works(arg(&args, "personId")?)?)?,
        "collaborators" => serde_json::to_value(corpus.collaborators(arg(&args, "personId")?, arg_or(&args, "limit", 24)?)?)?,

        // 首页（同 Tauri 的 home_summary）。utcOffsetMinutes：本地相对 UTC 的分钟数，「今天」按它算
        "homeSummary" => json!({
            "overview": corpus.overview()?,
            "continueTrack": corpus.continue_track()?,
            "recentlyPlayed": corpus.recently_played(12)?,
            "favorites": corpus.favorite_tracks(12)?,
            "today": corpus.today_listening(arg_or(&args, "utcOffsetMinutes", 0)?)?,
            "reach": corpus.reach()?,
            "recentlyAdded": corpus.recently_added(arg_or(&args, "addedLimit", 8)?)?,
        }),
        "isFavorite" => json!(corpus.is_favorite(&arg::<String>(&args, "entityType")?, &arg::<String>(&args, "entityId")?)?),
        "toggleFavorite" => json!(corpus.toggle_favorite(&arg::<String>(&args, "entityType")?, &arg::<String>(&args, "entityId")?)?),

        // 检索
        "kwic" => serde_json::to_value(corpus.kwic(&arg(&args, "query")?)?)?,
        "searchLyrics" => serde_json::to_value(
            corpus.search_lyrics(&arg::<String>(&args, "text")?, arg_or(&args, "limit", 500)?)?,
        )?,
        "quickSearch" => serde_json::to_value(
            corpus.quick_search(&arg::<String>(&args, "query")?, arg_or(&args, "perKind", None)?)?,
        )?,
        "wordInCorpus" => {
            let lemma = arg::<String>(&args, "lemma")?;
            let current = arg_or::<Option<String>>(&args, "currentSongId", None)?;
            let limit = arg_or(&args, "limit", 24)?;
            // scope：只在这块子语料里看（Tauri 那边同名参数）
            match arg_or::<Option<jp_corpus::subcorpus::Subcorpus>>(&args, "scope", None)?.filter(|s| !s.is_all()) {
                Some(scope) => serde_json::to_value(corpus.word_in_subcorpus(&lemma, current.as_deref(), limit, &scope)?)?,
                None => serde_json::to_value(corpus.word_in_corpus(&lemma, current.as_deref(), limit)?)?,
            }
        }

        // 定位：检索结果 ↔ 音频
        "lineSpans" => serde_json::to_value(corpus.line_spans(&arg::<String>(&args, "songId")?)?)?,
        "listenWindow" => serde_json::to_value(corpus.listen_window(
            arg(&args, "utteranceId")?,
            arg_or(&args, "before", 0)?,
            arg_or(&args, "after", 0)?,
        )?)?,

        // 分析
        "wordFrequency" => serde_json::to_value(corpus.word_frequency(arg_or::<Option<String>>(&args, "pos", None)?.as_deref(), arg_or(&args, "limit", 300)?)?)?,
        "frequency" => serde_json::to_value(corpus.frequency(&arg(&args, "query")?)?)?,
        "statsReport" => serde_json::to_value(jp_corpus::stats::stats_report(
            corpus.connection(), &arg_or(&args, "filter", jp_corpus::stats::StatsFilter::default())?,
        )?)?,
        "statsFrequency" => {
            let pos = arg_or::<Option<String>>(&args, "pos", None)?.filter(|p| !p.is_empty());
            serde_json::to_value(jp_corpus::stats::frequency_table(
                corpus.connection(), pos.as_deref(),
                &arg_or(&args, "filter", jp_corpus::stats::StatsFilter::default())?,
                arg_or::<usize>(&args, "limit", 300)?.clamp(1, 5000),
            )?)?
        }
        "collocations" => serde_json::to_value(corpus.collocations(&arg(&args, "query")?)?)?,
        "keyness" => serde_json::to_value(corpus.keyness(&arg(&args, "query")?)?)?,
        "ngrams" => serde_json::to_value(corpus.ngrams(&arg(&args, "query")?)?)?,
        "lexicalProfile" => serde_json::to_value(corpus.lexical_profile(&arg(&args, "query")?)?)?,
        "inflection" => serde_json::to_value(corpus.inflection(&arg(&args, "query")?)?)?,
        "cqlSearch" => serde_json::to_value(corpus.cql(&arg(&args, "query")?)?)?,
        "kwicBreakdown" => serde_json::to_value(corpus.breakdown(&arg(&args, "query")?)?)?,
        "collocationNetwork" => serde_json::to_value(corpus.collocation_network(&arg(&args, "query")?)?)?,
        "orthography" => serde_json::to_value(corpus.orthography(&arg(&args, "query")?)?)?,
        "songProfile" => serde_json::to_value(
            corpus.song_profile(&arg::<String>(&args, "songId")?, arg_or(&args, "limit", 12)?)?,
        )?,

        // 语料质量与来源追踪（docs/provenance.md）。只读的这几个两个客户端共用；
        // 换歌词、逐行编辑要落盘 .lrc，还在 jp-app 里，macOS 暂时没有
        "qualityOverview" => serde_json::to_value(jp_corpus::lineage::quality_overview(
            corpus.connection(),
            arg_or(&args, "limit", 50usize)?.clamp(1, 500),
        )?)?,
        "songLineage" => serde_json::to_value(jp_corpus::lineage::song_lineage(
            corpus.connection(),
            &arg::<String>(&args, "songId")?,
        )?)?,
        "lyricVersionRaw" => serde_json::to_value(
            jp_corpus::lineage::version_raw(corpus.connection(), arg(&args, "versionId")?)?.and_then(|(_, raw)| raw),
        )?,
        "artistProvenance" => serde_json::to_value(jp_corpus::lineage::provenance(
            corpus.connection(),
            "artist",
            &arg::<String>(&args, "name")?,
        )?)?,

        // 子语料与分析存档（同名的 Tauri command：subcorpus_* / analysis_*）
        "subcorpusFacets" => serde_json::to_value(jp_corpus::runs::facets(corpus.connection())?)?,
        "subcorpusPreview" => {
            let mut scope = jp_corpus::runs::scope(corpus.connection(), &arg(&args, "filter")?)?;
            scope.songs.clear();
            serde_json::to_value(scope)?
        }
        "subcorpusList" => serde_json::to_value(jp_corpus::runs::subcorpora(corpus.connection())?)?,
        "subcorpusSave" => serde_json::to_value(jp_corpus::runs::save_subcorpus(
            corpus.connection(),
            &arg::<String>(&args, "name")?,
            &arg(&args, "filter")?,
            &arg_or::<String>(&args, "note", String::new())?,
        )?)?,
        "subcorpusDelete" => json!(jp_corpus::runs::delete_subcorpus(corpus.connection(), arg(&args, "id")?)?),
        "analysisRun" => serde_json::to_value(jp_corpus::runs::run(
            corpus.connection(),
            &arg(&args, "spec")?,
            &arg_or(&args, "filter", jp_corpus::stats::StatsFilter::default())?,
            &arg_or::<String>(&args, "label", String::new())?,
            &app_version(&args),
            None,
        )?)?,
        "analysisList" => serde_json::to_value(jp_corpus::runs::list(corpus.connection(), arg_or(&args, "limit", 100)?)?)?,
        "analysisGet" => serde_json::to_value(jp_corpus::runs::get(corpus.connection(), arg(&args, "id")?)?)?,
        "analysisRerun" => serde_json::to_value(jp_corpus::runs::rerun(
            corpus.connection(),
            arg(&args, "id")?,
            &app_version(&args),
        )?)?,
        "analysisCompare" => {
            let get = |key: &str| -> Result<jp_corpus::runs::AnalysisRun> {
                let id: i64 = arg(&args, key)?;
                jp_corpus::runs::get(corpus.connection(), id)?.ok_or_else(|| anyhow!("找不到分析记录 {id}"))
            };
            serde_json::to_value(jp_corpus::runs::compare(&get("before")?, &get("after")?))?
        }
        "analysisDelete" => json!(jp_corpus::runs::delete(corpus.connection(), arg(&args, "id")?)?),

        // 播放历史（和 Tauri 客户端写同一张表，首页「最近播放」两边通用）
        "recordPlay" => {
            corpus.record_play(&arg(&args, "event")?)?;
            Value::Null
        }

        // 振假名（同 Tauri 的 lyrics_furigana）：现场用 Sudachi 取读音，一行约 0.02 ms，不入库
        "lyricsFurigana" => {
            let song_id: String = arg(&args, "songId")?;
            let mode: jp_tokenizer::furigana::FuriganaMode = arg_or(&args, "mode", Default::default())?;
            let analyzer = session
                .analyzer()
                .ok_or_else(|| anyhow!(jp_setup::sudachi::no_dictionary("振假名")))?;
            let mut out = Vec::new();
            for line in corpus.lyrics(&song_id)? {
                let tokens = analyzer.analyze(&line.text)?;
                let segments = jp_tokenizer::furigana::furigana_segments(&line.text, &tokens, mode);
                // 对不上原文就这一行不注音，不猜位置
                let rubies = jp_tokenizer::furigana::ruby_spans(&line.text, &segments).unwrap_or_default();
                out.push(json!({ "utteranceId": line.utterance_id, "rubies": rubies }));
            }
            Value::Array(out)
        }

        // 刮削：只读的和改一下状态的（要联网的在 dispatch_unlocked）
        "scrapeSummary" => serde_json::to_value(jp_scrape::summary(corpus.connection())?)?,
        "completeness" => serde_json::to_value(corpus.completeness()?)?,
        "scrapeReviewQueue" => serde_json::to_value(jp_scrape::review_queue(
            corpus.connection(),
            arg_or(&args, "statuses", None)?,
            arg_or(&args, "limit", None)?,
        )?)?,
        "scrapeSkip" => {
            let file_path: String = arg(&args, "filePath")?;
            let reason: Option<String> = arg_or(&args, "reason", None)?;
            jp_scrape::skip(corpus.connection(), &file_path, reason.as_deref())?;
            Value::Null
        }
        "scrapeRetryFailed" => json!(jp_scrape::retry_failed(corpus.connection())?),

        // 媒体路径
        "setAudioRoots" => {
            let roots: Vec<String> = arg_or(&args, "roots", Vec::new())?;
            *session.audio_roots.lock().map_err(|_| anyhow!("锁坏了"))? = roots.into_iter().map(PathBuf::from).collect();
            Value::Null
        }
        "resolveMedia" => {
            let stored: String = arg(&args, "path")?;
            let roots = session.audio_roots.lock().map_err(|_| anyhow!("锁坏了"))?.clone();
            let found = jp_corpus::media::resolve(&stored, session.root.as_deref(), &roots);
            json!(found.map(|p| p.display().to_string()))
        }
        other => return Err(anyhow!("没有这个方法：{other}")),
    };
    Ok(v)
}

/// 不碰会话那条连接的方法：导入作业、分词词典、查词。
///
/// 它们不拿语料库的锁，所以导入跑着（或者 207MB 的词典正在复制）的时候，界面照样能问进度、能检索。
/// 不是这一组的方法返回 `Ok(None)`，交给 [`dispatch`] 里拿着锁的那一组。
fn dispatch_unlocked(session: &Session, method: &str, args: &Value) -> Result<Option<Value>> {
    // 语法检查不碰库，导入占用连接时也能即时提示。
    if method == "cqlCheck" {
        jp_corpus::cql::check(&arg::<String>(args, "query")?)?;
        return Ok(Some(Value::Null));
    }
    if let Some(value) = system::dispatch(session, method, args)? { return Ok(Some(value)); }
    if let Some(value) = anki::dispatch(session, method, args)? {
        return Ok(Some(value));
    }
    if let Some(value) = library::dispatch(session, method, args)? {
        return Ok(Some(value));
    }
    if let Some(value) = lyrics::dispatch(session, method, args)? {
        return Ok(Some(value));
    }
    let v = match method {
        // ── 导入：扫描 → 计划（只读）→ 后台写库 ──
        "audioPeaks" => {
            let id: String = arg(args, "songId")?;
            let per_sec: u32 = arg(args, "perSec")?;
            let track = session.network_connection()?.track(&id)?.ok_or_else(|| anyhow!("找不到曲目 {id}"))?;
            ensure!(!track.audio_path.trim().is_empty(), "《{}》没有音频文件", track.title);
            let roots = session.audio_roots.lock().map_err(|_| anyhow!("锁坏了"))?.clone();
            let path = jp_corpus::media::resolve(&track.audio_path, session.root.as_deref(), &roots)
                .unwrap_or_else(|| PathBuf::from(&track.audio_path));
            let pcm = jp_audio::decode_all(&path)?;
            serde_json::to_value(jp_audio::decode::peaks(&pcm, per_sec.clamp(1, 1000)))?
        }
        "scanFolder" => {
            let path: String = arg(args, "path")?;
            ensure!(Path::new(&path).exists(), "路径不存在：{path}");
            serde_json::to_value(plan_targets(session, &[path], arg_or(args, "maxDepth", None)?)?)?
        }
        "scanFiles" => {
            let paths: Vec<String> = arg(args, "paths")?;
            ensure!(!paths.is_empty(), "一个文件都没选");
            for path in &paths {
                ensure!(Path::new(path).exists(), "路径不存在：{path}");
            }
            serde_json::to_value(plan_targets(session, &paths, arg_or(args, "maxDepth", None)?)?)?
        }
        "runImport" => run_import(session, arg_or(args, "tokenize", true)?)?,
        "importProgress" => serde_json::to_value(session.import_job.view())?,
        "cancelImport" => {
            session.import_job.request_cancel();
            Value::Null
        }

        // ── 分词词典（Sudachi）：和 Tauri 客户端同一套定位、下载、校验（jp-setup）──
        "tokenizerStatus" => serde_json::to_value(jp_setup::sudachi::status(&session.library_root()))?,
        "tokenizerDownload" => {
            start_tokenizer_download(session)?;
            Value::Null
        }
        "tokenizerProgress" => serde_json::to_value(session.tokenizer_job.view())?,
        "tokenizerInstall" => {
            let from: String = arg(args, "path")?;
            let root = session.library_root();
            let installed = jp_setup::sudachi::install(&root, Path::new(&from))?;
            ensure!(session.analyzer.reload(&root), "复制完了，但词典还是装不上");
            dictionary_ready(installed, tokenize_backlog(&session.db_path, session.analyzer()))
        }
        "tokenize" => {
            let text: String = arg(args, "text")?;
            let analyzer = session
                .analyzer()
                .ok_or_else(|| anyhow!(jp_setup::sudachi::no_dictionary("分词器")))?;
            serde_json::to_value(analyzer.analyze(&text)?)?
        }
        "tokenizeMissingRun" => {
            let analyzer = session.analyzer();
            ensure!(analyzer.is_some(), "没有分词词典，先下载一份");
            serde_json::to_value(tokenize_backlog(&session.db_path, analyzer)?)?
        }

        // ── 查词（Yomitan 词典库 dictionaries.db，和 Tauri 客户端同一套默认值）──
        "dictList" => serde_json::to_value(session.with_dictionaries(|store| store.dictionaries())?.unwrap_or_default())?,
        "dictLookup" => {
            let text: String = arg(args, "text")?;
            let mode: Option<String> = arg_or(args, "mode", None)?;
            let result = session.with_dictionaries(|store| {
                jp_dict::lookup::lookup(store, &session.translator, &text, mode.as_deref())
            })?;
            serde_json::to_value(result.unwrap_or_default())?
        }
        "dictLookupText" => {
            let text: String = arg(args, "text")?;
            let result = session
                .with_dictionaries(|store| jp_dict::lookup::lookup(store, &session.translator, &text, Some("group")))?
                .unwrap_or_default();
            lookup_text(&result)
        }

        // ── 刮削（jp-scrape，和 Tauri 同一份）。联网的都用自己的连接 ──
        "scrapeAlbumFillStart" => {
            let targets = jp_scrape::album_fill_targets(session.network_connection()?.connection())?;
            let total = targets.len();
            if total > 0 {
                let db = session.db_path.clone();
                start_scrape(session, move |cancelled, progress| jp_scrape::run_album_fill(&db, targets, cancelled, progress))?;
            }
            json!(total)
        }
        "scrapeStart" => {
            let ids = {
                let corpus = session.corpus.lock().map_err(|_| anyhow!("语料库连接已损坏"))?;
                jp_scrape::songs_to_scrape(corpus.connection(), arg_or(args, "onlyMissing", true)?, arg_or(args, "limit", 1000)?)?
            };
            let total = ids.len();
            let (db, covers) = (session.db_path.clone(), session.covers_dir());
            start_scrape(session, move |cancelled, progress| jp_scrape::run_batch(&db, &covers, &ids, None, cancelled, progress))?;
            json!(total)
        }
        "scrapeArtistsStart" => {
            let dir = session.artists_dir();
            let names = {
                let corpus = session.corpus.lock().map_err(|_| anyhow!("语料库连接已损坏"))?;
                jp_scrape::library_artists_with_files(corpus.connection(), arg_or(args, "onlyMissing", true)?, Some(&dir))?
            };
            let total = names.len();
            let db = session.db_path.clone();
            start_scrape(session, move |cancelled, progress| {
                jp_scrape::run_artist_batch(&db, &dir, &names, cancelled, progress)
            })?;
            json!(total)
        }
        "scrapeProgress" => serde_json::to_value(session.scrape_job.view())?,
        "scrapeCancel" => {
            session.scrape_job.request_cancel();
            Value::Null
        }
        "scrapeTrack" => {
            let song_id: String = arg(args, "songId")?;
            let conn = session.network_connection()?;
            serde_json::to_value(jp_scrape::scrape_track(conn.connection(), &session.covers_dir(), &song_id, None)?)?
        }
        "scrapeAccept" => {
            let file_path: String = arg(args, "filePath")?;
            let index: usize = arg(args, "candidateIndex")?;
            let conn = session.network_connection()?;
            serde_json::to_value(jp_scrape::accept_candidate(conn.connection(), &session.covers_dir(), &file_path, index)?)?
        }
        "scrapeArtist" => {
            let name: String = arg(args, "name")?;
            let conn = session.network_connection()?;
            serde_json::to_value(jp_scrape::scrape_artist_one(conn.connection(), &session.artists_dir(), &name)?)?
        }
        "scrapeBackfillCovers" => {
            let conn = session.network_connection()?;
            serde_json::to_value(jp_scrape::backfill_and_fill_albums(conn.connection(), &session.covers_dir())?)?
        }

        // ── 词典管理（同 Tauri 的 dict_import_* / dict_set_* / dict_delete）──
        "dictImport" => {
            let paths: Vec<String> = arg(args, "paths")?;
            ensure!(!paths.is_empty(), "一个词典包都没选");
            start_dict_import(session, paths)?;
            Value::Null
        }
        "dictImportProgress" => serde_json::to_value(session.dict_job.view())?,
        "dictImportCancel" => {
            session.dict_job.request_cancel();
            Value::Null
        }
        "dictSetEnabled" => {
            let id: i64 = arg(args, "id")?;
            let enabled: bool = arg(args, "enabled")?;
            session.edit_dictionaries(|store| store.set_enabled(id, enabled))?;
            Value::Null
        }
        "dictSetOrder" => {
            let ids: Vec<i64> = arg(args, "ids")?;
            session.edit_dictionaries(|store| store.set_order(&ids))?;
            Value::Null
        }
        "dictDelete" => {
            let id: i64 = arg(args, "id")?;
            session.edit_dictionaries(|store| store.delete(id))?;
            Value::Null
        }

        // ── Anki（AnkiConnect，本机 8765 端口）。连不上不是错误，界面显示指引 ──
        "ankiMineCheck" => {
            let model: String = arg(args, "model")?;
            let expressions: Vec<String> = arg(args, "expressions")?;
            serde_json::to_value(jp_anki::lyric_mine::check(&jp_anki::AnkiConnect::local(), &model, &expressions))?
        }
        "ankiMine" => {
            let request: jp_anki::lyric_mine::MineRequest = arg(args, "request")?;
            serde_json::to_value(mine(session, &jp_anki::AnkiConnect::local(), &request)?)?
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}

/// 一键制卡：取例句时拿一下语料库的锁，切音频、传 Anki 时不拿（同 Tauri 的 `anki_mine`）
fn mine(
    session: &Session,
    anki: &jp_anki::AnkiConnect,
    request: &jp_anki::lyric_mine::MineRequest,
) -> Result<jp_anki::mine::MineOutcome> {
    let path = session.dictionaries_path();
    ensure!(path.exists(), "还没有导入过词典，制卡要用词典里的释义");
    let store = jp_dict::store::DictionaryStore::open(&path).context("打不开词典库")?;
    let prepared = {
        let corpus = session.corpus.lock().map_err(|_| anyhow!("语料库连接已损坏"))?;
        jp_anki::lyric_mine::prepare(&corpus, &store, &session.translator, request)?
    };
    // 例句音频按库里记的路径切；库从别的机器拷来时，换成这台机器上找到的文件
    let audio_roots = session.audio_roots.lock().map_err(|_| anyhow!("锁坏了"))?.clone();
    let prepared = prepared.with_media_resolved(|stored| {
        jp_corpus::media::resolve(stored, session.root.as_deref(), &audio_roots).map(|p| p.display().to_string())
    });
    jp_anki::lyric_mine::send(anki, &store, &prepared, request)
}

/// 批量刮削的外壳：进度存进作业状态等界面来问。`stage` 是刚刮完的那首（或那位歌手），
/// 跑完的结果是逐条的记录（状态、采纳的候选、封面下没下下来）
fn start_scrape(
    session: &Session,
    work: impl FnOnce(&dyn Fn() -> bool, &mut dyn FnMut(jp_scrape::ScrapeProgress)) -> Result<bool> + Send + 'static,
) -> Result<()> {
    session.scrape_job.spawn("刮削", move |job| {
        let mut items = Vec::new();
        let cancelled = work(&|| job.cancelled(), &mut |p: jp_scrape::ScrapeProgress| {
            job.report(p.done as u64, p.total as u64, p.title.clone());
            items.push(p);
        })?;
        Ok((json!({ "items": items }), cancelled))
    })
}

/// 逐本导入 Yomitan 词典包（zip）。一本失败不影响后面的；取消时正在导的那本整本回滚。
/// 进度：`done/total` 是这一本里的文件数，`stage` 是「第几本 / 共几本 · 文件名」
fn start_dict_import(session: &Session, paths: Vec<String>) -> Result<()> {
    for path in &paths {
        ensure!(Path::new(path).exists(), "路径不存在：{path}");
    }
    let db = session.dictionaries_path();
    session.dict_job.spawn("词典导入", move |job| {
        let mut store = jp_dict::store::DictionaryStore::open(&db)?;
        let total = paths.len();
        let mut results = Vec::new();
        for (index, path) in paths.iter().enumerate() {
            if job.cancelled() {
                break;
            }
            let name = jp_corpus::media::file_name_of(path).to_owned();
            job.report(0, 0, format!("{}/{} · {name}", index + 1, total));
            let outcome = (|| -> Result<jp_dict::import::ImportSummary> {
                let mut archive = jp_dict::import::open_archive(Path::new(path))?;
                let options = jp_dict::import::ImportOptions {
                    source_path: path.clone(),
                    cancel: Some(job.cancel_flag()),
                    ..Default::default()
                };
                jp_dict::import::import_dictionary(&mut store, archive.as_mut(), &options, &mut |p| {
                    job.report(p.done_files as u64, p.total_files as u64, format!("{}/{} · {name}", index + 1, total));
                })
            })();
            results.push(match outcome {
                Ok(summary) => json!({ "path": path, "summary": summary, "error": null }),
                Err(err) => json!({ "path": path, "summary": null, "error": format!("{err:#}") }),
            });
        }
        Ok((json!({ "results": results }), job.cancelled()))
    })
}

/// 扫 + 比对 + 记下待确认的结果（同 Tauri 的 `scan_folder` / `scan_files`）。
///
/// 翻目录、读 tag 可能要好几秒，这期间不拿会话的锁；只在读现有曲库时拿一下
fn plan_targets(session: &Session, paths: &[String], max_depth: Option<usize>) -> Result<jp_import::ScanResult> {
    let (tracks, ignored) = jp_import::scan_targets(paths, max_depth.unwrap_or(8));
    let index = {
        let corpus = session.corpus.lock().map_err(|_| anyhow!("语料库连接已损坏"))?;
        jp_import::LibraryIndex::from_corpus(&corpus)?
    };
    let result = jp_import::review(&tracks, &index, session.analyzer().is_some(), ignored);
    *session.pending_scan.lock().unwrap_or_else(|e| e.into_inner()) = tracks;
    Ok(result)
}

/// 把扫描过的那一批写进库，后台跑；进度用 `importProgress` 问。
///
/// 和 Tauri 的 `run_import` 同一个 `jp_import::execute_cancellable`：一首歌一个事务，
/// `cancelImport` 停下来时，停在哪儿哪儿之前的就是真导进去了。
fn run_import(session: &Session, tokenize: bool) -> Result<Value> {
    let tracks = session.pending_scan.lock().unwrap_or_else(|e| e.into_inner()).clone();
    ensure!(!tracks.is_empty(), "没有待导入的条目，先调 scanFolder");
    let analyzer = if tokenize { session.analyzer() } else { None };
    let tokenized = analyzer.is_some();
    let db_path = session.db_path.clone();
    let pending = Arc::clone(&session.pending_scan);
    session.import_job.spawn("导入", move |job| {
        let mut corpus = open_worker(&db_path)?;
        // 拿当前曲库重新算计划：扫描到确认之间库可能已经变了
        let index = jp_import::LibraryIndex::from_corpus(&corpus)?;
        let plan = jp_import::plan(&tracks, &index);
        let total = plan.to_import().count() as u64;
        job.report(0, total, "");
        let report = jp_import::execute_cancellable(
            corpus.connection_mut(),
            &plan,
            analyzer.as_deref(),
            |track, done, _| job.report(done as u64, total, track.title.clone()),
            &|| job.cancelled(),
        )?;
        let root = db_path.parent().unwrap_or(Path::new("."));
        jp_scrape::save_imported_covers(corpus.connection(), &root.join("raw/covers"), &report);
        let csv = jp_library::admin::sync_imported(&jp_library::admin::Library {
            corpus: &mut corpus, db_path: &db_path, covers_dir: &root.join("raw/covers"), pitch_cache: &root.join("cache/pitch"),
        }, &report);
        // 导完就清掉，避免同一批被点两次
        pending.lock().unwrap_or_else(|e| e.into_inner()).clear();
        let mut result = serde_json::to_value(&report)?;
        result["csv"] = json!(csv);
        result["imported"] = json!(report.imported());
        result["failed"] = json!(report.failed());
        result["lyricLines"] = json!(report.lyric_lines());
        result["tokens"] = json!(report.tokens());
        Ok((result, report.cancelled))
    })?;
    Ok(json!({ "tokenize": tokenized }))
}

/// 下载分词词典（43MB 下载 + 207MB 解压），后台跑；进度用 `tokenizerProgress` 问。
/// 装好以后顺手把「有歌词、没分词」的歌补上（同 Tauri 的 `tokenizer_download`）
fn start_tokenizer_download(session: &Session) -> Result<()> {
    let root = session.library_root();
    let slot = Arc::clone(&session.analyzer);
    let db_path = session.db_path.clone();
    session.tokenizer_job.spawn("词典下载", move |job| {
        let installed = jp_setup::sudachi::download(&root, |p| job.report(p.received, p.total, p.stage))?;
        ensure!(slot.reload(&root), "下好了，但词典还是装不上");
        job.report(0, 0, "tokenizing");
        Ok((dictionary_ready(installed, tokenize_backlog(&db_path, slot.get(&root))), false))
    })
}

/// 装好词典的结果，和 Tauri 的 `DictionaryReady` 同一个形状。
/// 补分词失败不算装失败：词典确实装好了，原因放进 `tokenizeError`
fn dictionary_ready(installed: jp_setup::sudachi::Installed, backlog: Result<jp_import::maintain::Tokenized>) -> Value {
    let (tokenized, error) = match backlog {
        Ok(report) => (report, String::new()),
        Err(err) => (Default::default(), format!("{err:#}")),
    };
    json!({
        "bytes": installed.bytes,
        "dir": installed.dir,
        "tokenized": tokenized,
        "tokenizeError": error,
    })
}

/// 给「有歌词、没分词」的歌补上分词。自己开连接、一个事务，出错整个回滚。没词典就什么都不做
fn tokenize_backlog(db_path: &Path, analyzer: Option<Arc<Analyzer>>) -> Result<jp_import::maintain::Tokenized> {
    let Some(analyzer) = analyzer else {
        return Ok(Default::default());
    };
    let mut corpus = open_worker(db_path)?;
    if jp_import::maintain::untokenized_songs(corpus.connection())?.is_empty() {
        return Ok(Default::default());
    }
    let tx = corpus.connection_mut().transaction()?;
    let report = jp_import::maintain::tokenize_missing(&tx, &analyzer, |_| {})?;
    tx.commit()?;
    Ok(report)
}

/// 查词结果的精简版：词头、读音、每本词典的纯文本释义。给不渲染 HTML 的界面（macOS 歌词面板）用
fn lookup_text(result: &jp_dict::translator::FindTermsResult) -> Value {
    let entries: Vec<Value> = result
        .dictionary_entries
        .iter()
        .map(|entry| {
            let headwords: Vec<Value> = entry
                .headwords
                .iter()
                .map(|h| json!({ "term": h.term, "reading": h.reading }))
                .collect();
            let definitions: Vec<Value> = entry
                .definitions
                .iter()
                .map(|d| {
                    let text: Vec<String> = d
                        .entries
                        .iter()
                        .map(jp_dict::lookup::plain_text)
                        .filter(|t| !t.is_empty())
                        .collect();
                    json!({ "dictionary": d.dictionary, "text": text, "entries": d.entries })
                })
                .collect();
            json!({ "headwords": headwords, "definitions": definitions })
        })
        .collect();
    json!({ "entries": entries, "originalTextLength": result.original_text_length })
}

// ────────────────────────────── C ABI ──────────────────────────────

fn to_c(s: String) -> *mut c_char {
    // 字符串里不该有 NUL；万一有（歌词里的控制字符），替换掉而不是返回 NULL
    CString::new(s)
        .unwrap_or_else(|e| CString::new(e.into_vec().into_iter().filter(|b| *b != 0).collect::<Vec<u8>>()).unwrap_or_default())
        .into_raw()
}

/// # Safety
/// `ptr` 必须是 NULL 或一个以 NUL 结尾的有效字符串
unsafe fn from_c<'a>(ptr: *const c_char) -> Result<&'a str> {
    if ptr.is_null() {
        return Err(anyhow!("参数是空指针"));
    }
    // SAFETY: 调用方保证以 NUL 结尾且在调用期间有效
    unsafe { CStr::from_ptr(ptr) }.to_str().context("参数不是 UTF-8")
}

fn envelope(result: std::thread::Result<Result<Value>>) -> String {
    match result {
        Ok(Ok(value)) => json!({ "ok": value }).to_string(),
        Ok(Err(err)) => {
            jp_diagnostics::log::error(format!("{err:#}"));
            json!({ "error": format!("{err:#}") }).to_string()
        }
        Err(panic) => {
            let msg = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "未知错误".into());
            jp_diagnostics::log::error(format!("内部错误：{msg}"));
            json!({ "error": format!("内部错误：{msg}") }).to_string()
        }
    }
}

/// 打开语料库。失败返回 NULL，原因写进 `*out_error`（调用方用 [`jp_string_free`] 释放）。
///
/// # Safety
/// `db_path` 是以 NUL 结尾的 UTF-8；`out_error` 为 NULL 或指向可写的指针
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jp_open(db_path: *const c_char, out_error: *mut *mut c_char) -> *mut Session {
    let result = std::panic::catch_unwind(|| {
        // SAFETY: 见函数说明
        let path = unsafe { from_c(db_path) }?;
        Session::open(path)
    });
    match result {
        Ok(Ok(session)) => Box::into_raw(Box::new(session)),
        other => {
            if !out_error.is_null() {
                let message = match other {
                    Ok(Err(err)) => format!("{err:#}"),
                    _ => "打开语料库时内部出错".into(),
                };
                // SAFETY: 调用方保证 out_error 可写
                unsafe { *out_error = to_c(message) };
            }
            std::ptr::null_mut()
        }
    }
}

/// 调一个方法，返回 JSON 信封 `{"ok":…}` / `{"error":"…"}`。用 [`jp_string_free`] 释放。
///
/// # Safety
/// `session` 来自 [`jp_open`] 且未关闭；`method`、`args_json` 是以 NUL 结尾的 UTF-8（`args_json` 可为 NULL，等同 `{}`）
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jp_call(session: *const Session, method: *const c_char, args_json: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Value> {
        if session.is_null() {
            return Err(anyhow!("语料库没打开"));
        }
        // SAFETY: 见函数说明
        let session = unsafe { &*session };
        let method = unsafe { from_c(method) }?;
        let args: Value = if args_json.is_null() {
            json!({})
        } else {
            serde_json::from_str(unsafe { from_c(args_json) }?).context("参数不是合法 JSON")?
        };
        session.call(method, args)
    }));
    to_c(envelope(result))
}

/// 关闭语料库。传 NULL 什么都不做。
///
/// # Safety
/// `session` 来自 [`jp_open`]，且之后不再使用
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jp_close(session: *mut Session) {
    if !session.is_null() {
        // SAFETY: 来自 Box::into_raw，只释放一次
        drop(unsafe { Box::from_raw(session) });
    }
}

/// 释放 Rust 返回的字符串。传 NULL 什么都不做。
///
/// # Safety
/// `s` 来自本库返回的字符串，且只释放一次
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jp_string_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: 来自 CString::into_raw
        drop(unsafe { CString::from_raw(s) });
    }
}

/// 在 `dir` 下写一个合成的小语料库（四首歌，见 `jp_corpus::fixture`），返回 `corpus.db` 的路径。
/// 给 Swift 侧的测试和「先看看效果」用；失败返回 NULL。
///
/// # Safety
/// `dir` 是以 NUL 结尾的 UTF-8
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jp_write_fixture(dir: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(|| -> Result<String> {
        // SAFETY: 见函数说明
        let dir = unsafe { from_c(dir) }?;
        Ok(jp_corpus::fixture::write_to(std::path::Path::new(dir))?.display().to_string())
    });
    match result {
        Ok(Ok(path)) => to_c(path),
        _ => std::ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cql_check_uses_shared_parser_without_locking_corpus() {
        let (s, dir) = session();
        let valid = [
            r#"[lemma="君"] [pos="ADP"]"#,
            r#"[pos="VERB" & cform="命令形.*"]"#,
            r#"[lemma="て"] []{0,2} [lemma="しまう"]"#,
            r#"[reading="きみ"]"#,
            r#"[norm="会う"] [lemma="たい"]"#,
            r#"[pos="ADJ"] [pos="NOUN"]"#,
            r#""夢" []? [pos="NOUN"]"#,
        ];
        {
            let _held = s.corpus.lock().unwrap();
            for query in valid {
                assert_eq!(s.call("cqlCheck", json!({"query": query})).unwrap(), Value::Null);
            }
            for query in [r#"[lemma="君""#, r#"[unknown="君"]"#, r#"[lemma="("]"#] {
                let expected = jp_corpus::cql::check(query).unwrap_err().to_string();
                let error = s.call("cqlCheck", json!({"query": query})).unwrap_err();
                assert_eq!(error.to_string(), expected);
            }
            assert!(s.call("cqlCheck", json!({})).is_err());
            assert!(s.call("cqlCheck", json!({"query": 42})).is_err());
        }
        drop(s);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn visualization_methods_match_shared_queries() {
        let (s, dir) = session();
        let edges = s.call("collaborationEdges", json!({})).unwrap();
        let releases = s.call("timelineReleases", json!({})).unwrap();
        let corpus = jp_corpus::Corpus::open(dir.join("corpus.db")).unwrap();
        assert_eq!(edges, serde_json::to_value(corpus.collaboration_edges().unwrap()).unwrap());
        assert_eq!(releases, serde_json::to_value(corpus.timeline_releases().unwrap()).unwrap());
        assert!(!releases.as_array().unwrap().is_empty());
        let rows = s.call("statsFrequency", json!({"pos":"NOUN", "limit": 2})).unwrap();
        assert_eq!(rows, serde_json::to_value(jp_corpus::stats::frequency_table(
            corpus.connection(), Some("NOUN"), &Default::default(), 2,
        ).unwrap()).unwrap());
        assert!(!rows.as_array().unwrap().is_empty());
        assert_eq!(s.call("statsFrequency", json!({"pos":"", "limit":0})).unwrap().as_array().unwrap().len(), 1);
        let report = s.call("statsReport", json!({})).unwrap();
        assert_eq!(report, serde_json::to_value(jp_corpus::stats::stats_report(corpus.connection(), &Default::default()).unwrap()).unwrap());
        assert_eq!(s.call("timeline", json!({})).unwrap(), serde_json::to_value(corpus.timeline().unwrap()).unwrap());
    }

    #[test]
    fn saved_scope_preserves_all_conditions_and_updates_by_name() {
        let (s, _dir) = session();
        let filter = json!({"performerIds":[1], "jpOnly":true, "yearFrom":2010, "yearTo":2024,
            "genres":["J-Pop"], "languages":["ja"], "albumIds":[1], "songIds":["001"],
            "minCompleteness":0.8, "distinctLines":true});
        let saved = s.call("subcorpusSave", json!({"name":"完整范围", "filter":filter})).unwrap();
        for (key, value) in filter.as_object().unwrap() { assert_eq!(&saved["filter"][key], value); }
        let updated = s.call("subcorpusSave", json!({"name":"完整范围", "filter":{"performerIds":[], "jpOnly":false}})).unwrap();
        assert_eq!(saved["id"], updated["id"]);
        assert_eq!(s.call("subcorpusList", json!({})).unwrap().as_array().unwrap().len(), 1);
        assert_eq!(s.call("subcorpusDelete", json!({"id":saved["id"]})).unwrap(), true);
        assert!(s.call("subcorpusList", json!({})).unwrap().as_array().unwrap().is_empty());
    }

    pub(super) fn session() -> (Session, PathBuf) {
        let dir = std::env::temp_dir().join(format!("jp-ffi-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = jp_corpus::fixture::write_to(&dir).unwrap();
        (Session::open(db.to_str().unwrap()).unwrap(), dir)
    }

    /// 每个测试一个目录。不用时间戳：Windows 上两个线程同一刻拿到的纳秒数会相同
    fn rand_suffix() -> usize {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    #[test]
    fn the_minimal_loop_search_kwic_song_position() {
        let (s, dir) = session();
        // 1. 检索
        let hits = s
            .call("kwic", json!({ "query": { "keywords": ["風"], "field": "lemma" } }))
            .unwrap();
        let hit = &hits[0];
        // 2. 命中带着歌、行、音频区间
        let song = hit["songId"].as_str().unwrap();
        let utterance = hit["utteranceId"].as_i64().unwrap();
        assert!(hit["endSec"].as_f64().unwrap() > hit["timeSec"].as_f64().unwrap());
        // 3. 跳到歌：歌词里有这一行
        let lyrics = s.call("lyrics", json!({ "songId": song })).unwrap();
        assert!(lyrics.as_array().unwrap().iter().any(|l| l["utteranceId"].as_i64() == Some(utterance)));
        // 4. 定位：试听区间包住这一行
        let w = s.call("listenWindow", json!({ "utteranceId": utterance, "before": 1, "after": 1 })).unwrap();
        assert_eq!(w["songId"].as_str(), Some(song));
        assert!(w["startSec"].as_f64().unwrap() <= w["focusStartSec"].as_f64().unwrap());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn lyric_timeline_exports_only_valid_units_and_explicit_fallback_modes() {
        let (session, dir) = session();
        {
            let corpus = session.corpus.lock().unwrap();
            corpus.connection().execute_batch("INSERT INTO songs(id,title,artist,duration_sec) VALUES('timing','試験','架空',30);\
                INSERT INTO utterances(id,song_id,line_idx,time_sec,text) VALUES\
                (9901,'timing',0,1,'𠮷😀が光'),(9902,'timing',1,10,'空へ'),(9903,'timing',2,NULL,'文字');\
                INSERT INTO word_timings VALUES(9901,0,2,1,1.4,'lrc'),(9901,2,5,2,2.4,'lrc'),(9902,0,1,10,11,'lrc');").unwrap();
        }
        let json = session.call("lyricTimeline", json!({"songId":"timing"})).unwrap();
        let rows: Vec<jp_corpus::display_timeline::LineTimeline> = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(json[0]["utteranceId"], 9901);
        assert_eq!(json[0]["mode"], "karaoke");
        assert_eq!(json[0]["units"].as_array().unwrap().len(), 2);
        assert_eq!(json[0]["units"][0]["charEnd"], 2);
        assert_eq!(json[0]["units"][0]["endSec"], 1.4);
        assert_eq!(json[1]["mode"], "lineSynced");
        assert_eq!(json[2]["mode"], "plain");
        assert_eq!(json[1]["units"], json!([]));
        assert_eq!(session.call("lyricTimeline", json!({"songId":"missing"})).unwrap(), json!([]));
        drop(session);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn analysis_methods_answer_and_unknown_methods_are_errors() {
        let (s, dir) = session();
        let f = s.call("frequency", json!({ "query": { "scope": { "unit": "distinctLines" }, "limit": 3 } })).unwrap();
        assert_eq!(f["items"].as_array().unwrap().len(), 3);
        let c = s.call("collocations", json!({ "query": { "node": "夜", "minFreq": 1 } })).unwrap();
        assert!(c["nodeFreq"].as_i64().unwrap() >= 2);
        let k = s.call("keyness", json!({ "query": { "target": { "songIds": ["001"] }, "minFreq": 1 } })).unwrap();
        assert!(k["over"].is_array());
        assert!(s.call("songProfile", json!({ "songId": "004" })).unwrap()["repeatedLines"].as_i64().unwrap() >= 1);
        assert!(s.call("noSuchThing", json!({})).is_err());
        assert!(s.call("lyrics", json!({})).is_err(), "缺参数要报错而不是 panic");
        std::fs::remove_dir_all(dir).ok();
    }

    /// 来源追踪和分析存档在 macOS 这边也答得上来：打开时补登记了版本，存档能存 analysis.rs 的分析
    #[test]
    fn provenance_and_archived_analyses_are_reachable_from_the_ffi() {
        let (s, dir) = session();
        let overview = s.call("qualityOverview", json!({})).unwrap();
        assert_eq!(overview["songsWithVersion"], overview["songsWithLyrics"], "打开时就补登记了");
        let lineage = s.call("songLineage", json!({ "songId": "001" })).unwrap();
        assert_eq!(lineage["versions"][0]["sourceType"], "legacy");

        let early = json!({ "yearTo": 2019 });
        let w = s.call("wordInCorpus", json!({ "lemma": "風", "scope": early })).unwrap();
        assert!(w["examples"].as_array().unwrap().iter().all(|e| ["001", "002"].contains(&e["songId"].as_str().unwrap())));

        let spec = json!({ "kind": "corpusFrequency", "query": { "scope": { "subcorpus": early, "unit": "distinctLines" }, "limit": 20 } });
        let run = s.call("analysisRun", json!({ "spec": spec, "appVersion": "mac 0.1" })).unwrap();
        assert_eq!(run["appVersion"], "mac 0.1");
        assert!(run["snapshot"]["fingerprint"].is_string());
        let cmp = s.call("analysisRerun", json!({ "id": run["id"] })).unwrap();
        assert_eq!(cmp["resultIdentical"], json!(true));
        assert!(s.call("analysisList", json!({})).unwrap().as_array().unwrap().len() >= 2);
        std::fs::remove_dir_all(dir).ok();
    }

    /// 首页、专辑、歌手、收藏：macOS 首页和曲库页用的那几组
    #[test]
    fn home_albums_people_and_favorites_answer() {
        let (s, dir) = session();
        s.call("recordPlay", json!({ "event": { "songId": "004", "listenedSec": 40.0, "positionSec": 40.0, "completed": false, "source": "test" } }))
            .unwrap();
        let home = s.call("homeSummary", json!({ "utcOffsetMinutes": 480 })).unwrap();
        assert_eq!(home["continueTrack"]["songId"], "004");
        assert_eq!(home["recentlyPlayed"][0]["songId"], "004");
        assert!(home["overview"]["tracks"].as_i64().unwrap() >= 4);
        assert!(home["today"]["listenedSec"].as_f64().unwrap() >= 40.0);

        let albums = s.call("albums", json!({})).unwrap();
        if let Some(album) = albums.as_array().unwrap().first() {
            let tracks = s.call("albumTracks", json!({ "albumId": album["id"] })).unwrap();
            assert_eq!(tracks.as_array().unwrap().len() as i64, album["trackCount"].as_i64().unwrap());
        }
        let people = s.call("peopleDirectory", json!({})).unwrap();
        let person = &people.as_array().unwrap()[0];
        let works = s.call("personWorks", json!({ "personId": person["id"] })).unwrap();
        assert!(!works.as_array().unwrap().is_empty());
        assert!(works[0]["title"].is_string(), "作品是平铺的 Track 加 roles");
        assert_eq!(s.call("personById", json!({ "personId": person["id"] })).unwrap()["name"], person["name"]);
        assert!(s.call("collaborators", json!({ "personId": person["id"] })).unwrap().is_array());

        let fav = json!({ "entityType": "song", "entityId": "001" });
        let was = s.call("isFavorite", fav.clone()).unwrap().as_bool().unwrap();
        assert_eq!(s.call("toggleFavorite", fav.clone()).unwrap(), json!(!was));
        assert_eq!(s.call("isFavorite", fav.clone()).unwrap(), json!(!was));
        let listed = |s: &Session| s.call("homeSummary", json!({})).unwrap()["favorites"].as_array().unwrap().iter().any(|t| t["id"] == "001");
        assert_eq!(listed(&s), !was);
        assert_eq!(s.call("toggleFavorite", fav).unwrap(), json!(was));
        assert_eq!(listed(&s), was);
        std::fs::remove_dir_all(dir).ok();
    }

    /// 刮削的本地部分（不联网）：统计、复核队列、重试、完整度；批量作业空跑一遍能收尾。
    /// 振假名：有分词词典时位置落在原文里，没有时说清楚缺什么
    #[test]
    fn scraping_bookkeeping_and_furigana_answer_without_the_network() {
        let (s, dir) = session();
        let summary = s.call("scrapeSummary", json!({})).unwrap();
        assert!(summary.is_object(), "{summary}");
        assert!(s.call("scrapeReviewQueue", json!({})).unwrap().as_array().unwrap().is_empty());
        assert_eq!(s.call("scrapeRetryFailed", json!({})).unwrap(), json!(0));
        assert!(s.call("completeness", json!({})).unwrap().is_object());

        assert_eq!(s.call("scrapeStart", json!({ "limit": 0 })).unwrap(), json!(0));
        let mut view = Value::Null;
        for _ in 0..500 {
            view = s.call("scrapeProgress", json!({})).unwrap();
            if view["finished"] == json!(true) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(view["finished"], json!(true), "{view}");
        assert!(view["error"].is_null(), "{view}");
        assert_eq!(view["result"]["items"], json!([]));

        match s.call("lyricsFurigana", json!({ "songId": "004", "mode": "kanji" })) {
            Ok(lines) => {
                let lyrics = s.call("lyrics", json!({ "songId": "004" })).unwrap();
                for (line, f) in lyrics.as_array().unwrap().iter().zip(lines.as_array().unwrap()) {
                    assert_eq!(line["utteranceId"], f["utteranceId"]);
                    let len = line["text"].as_str().unwrap().chars().count() as u64;
                    for r in f["rubies"].as_array().unwrap() {
                        assert!(r["end"].as_u64().unwrap() <= len && !r["reading"].as_str().unwrap().is_empty(), "{f}");
                    }
                }
                let first = &lines[0]["rubies"];
                assert!(first.as_array().unwrap().iter().any(|r| r["reading"] == "よる"), "「雨が降る夜に」的夜要注よる：{first}");
            }
            Err(err) => assert!(err.to_string().contains("词典"), "{err}"),
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_c_abi_round_trips_and_reports_errors_as_json() {
        let dir = std::env::temp_dir().join(format!("jp-ffi-c-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir_c = CString::new(dir.to_str().unwrap()).unwrap();
        unsafe {
            let db = jp_write_fixture(dir_c.as_ptr());
            assert!(!db.is_null());
            let mut err: *mut c_char = std::ptr::null_mut();
            let s = jp_open(db, &mut err);
            assert!(!s.is_null() && err.is_null());
            jp_string_free(db);

            let method = CString::new("tracks").unwrap();
            let out = jp_call(s, method.as_ptr(), std::ptr::null());
            let v: Value = serde_json::from_str(CStr::from_ptr(out).to_str().unwrap()).unwrap();
            assert_eq!(v["ok"].as_array().unwrap().len() as i64, jp_corpus::fixture::TRACKS);
            jp_string_free(out);

            let bad = CString::new("{not json").unwrap();
            let out = jp_call(s, method.as_ptr(), bad.as_ptr());
            let v: Value = serde_json::from_str(CStr::from_ptr(out).to_str().unwrap()).unwrap();
            assert!(v["error"].as_str().unwrap().contains("JSON"));
            jp_string_free(out);
            jp_close(s);

            // 没打开的会话：不崩，报错
            let out = jp_call(std::ptr::null(), method.as_ptr(), std::ptr::null());
            let v: Value = serde_json::from_str(CStr::from_ptr(out).to_str().unwrap()).unwrap();
            assert!(v["error"].is_string());
            jp_string_free(out);
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn audio_paths_from_another_machine_are_found_under_the_library_root() {
        let (s, dir) = session();
        let audio = dir.join("raw").join("audio").join("歌手");
        std::fs::create_dir_all(&audio).unwrap();
        std::fs::write(audio.join("x.flac"), b"x").unwrap();
        let found = s.call("resolveMedia", json!({ "path": r"E:\cloud music\x.flac" })).unwrap();
        assert_eq!(found.as_str().map(PathBuf::from), Some(audio.join("x.flac")));
        std::fs::remove_dir_all(dir).ok();
    }
}
