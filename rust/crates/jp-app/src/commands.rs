//! Tauri command 层。
//!
//! 刻意保持极薄：每个 command 就是「取状态 → 调一次 jp-corpus → 返回」。
//! 业务逻辑一律留在 `jp-corpus` / `jp-tokenizer` 里，那两层可以离线测试，
//! 而 command 层测不了（需要 Tauri 运行时）。**能测的东西不要放进测不了的层。**

use jp_corpus::{
    Album, Collaborator, Credit, FacetCount, KwicHit, KwicQuery, LyricHit, LyricLine, Overview,
    PersonSummary, PlayEvent, PlayedTrack, QuickSearchResults, Track, WordFrequency, WordInCorpus,
    YearStats,
};
use serde::Serialize;
use tauri::{Emitter, Manager, State};

use crate::state::AppState;

/// 前端能看懂的错误。
///
/// `anyhow::Error` 不能 `Serialize`，而且直接把内部错误抛给前端也不合适。
/// 这里统一转成一条消息——真正的诊断信息打到 stderr。
#[derive(Debug, Serialize)]
pub struct CommandError {
    message: String,
}

impl From<anyhow::Error> for CommandError {
    fn from(err: anyhow::Error) -> Self {
        crate::log::error(format!("command: {err:?}"));
        Self {
            // `{:#}` 把整条 anyhow 链拼出来。只用 `to_string()` 的话界面上只剩最外层
            // 那句——「下载失败：<地址>」，而真正有用的「Peer disconnected」被盖掉了，
            // 用户和我都只能靠猜。
            message: format!("{err:#}"),
        }
    }
}

impl From<rusqlite::Error> for CommandError {
    fn from(err: rusqlite::Error) -> Self {
        crate::log::error(format!("command: {err:?}"));
        Self {
            message: err.to_string(),
        }
    }
}

type CmdResult<T> = Result<T, CommandError>;

// ────────────────────────────── 曲库 ──────────────────────────────

#[tauri::command]
pub fn list_tracks(state: State<'_, AppState>, limit: Option<i64>) -> CmdResult<Vec<Track>> {
    Ok(state.corpus().tracks(limit.unwrap_or(1000))?)
}

#[tauri::command]
pub fn get_track(state: State<'_, AppState>, song_id: String) -> CmdResult<Option<Track>> {
    Ok(state.corpus().track(&song_id)?)
}

#[tauri::command]
pub fn list_albums(
    state: State<'_, AppState>,
    artist_key: Option<String>,
    limit: Option<i64>,
) -> CmdResult<Vec<Album>> {
    Ok(state
        .corpus()
        .albums(artist_key.as_deref(), limit.unwrap_or(500))?)
}

#[tauri::command]
pub fn album_tracks(state: State<'_, AppState>, album_id: i64) -> CmdResult<Vec<Track>> {
    Ok(state.corpus().album_tracks(album_id)?)
}

#[tauri::command]
pub fn track_credits(state: State<'_, AppState>, song_id: String) -> CmdResult<Vec<Credit>> {
    Ok(state.corpus().credits_for_track(&song_id)?)
}

#[tauri::command]
pub fn people_by_role(
    state: State<'_, AppState>,
    role: String,
    limit: Option<i64>,
) -> CmdResult<Vec<PersonSummary>> {
    Ok(state.corpus().people_by_role(&role, limit.unwrap_or(200))?)
}

#[tauri::command]
pub fn works_by_person(state: State<'_, AppState>, person_id: i64) -> CmdResult<Vec<Track>> {
    Ok(state.corpus().works_by_person(person_id)?)
}

#[tauri::command]
pub fn collaborators(
    state: State<'_, AppState>,
    person_id: i64,
    limit: Option<i64>,
) -> CmdResult<Vec<Collaborator>> {
    Ok(state
        .corpus()
        .collaborators(person_id, limit.unwrap_or(50))?)
}

/// 一首歌的全部歌词行，带分词。整首歌只发 2 次查询。
#[tauri::command]
pub fn lyrics(state: State<'_, AppState>, song_id: String) -> CmdResult<Vec<LyricLine>> {
    Ok(state.corpus().lyrics(&song_id)?)
}

/// 歌词可选的字体：本机装的（DirectWrite），项目自带的（`assets/fonts`），用户导入过的文件。
///
/// 字体文件要让界面按文件加载，这里逐个放行进 asset 协议（只放行这几个文件，不放行目录）。
/// 本机字体列不出来（非常少见）不算失败：照样给出字体文件，原因放进 `problems`。
#[tauri::command]
pub fn fonts_catalog<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    imported: Vec<String>,
) -> CmdResult<crate::fonts::FontCatalog> {
    use tauri::Manager;
    let mut problems = Vec::new();
    let installed = crate::fonts::installed_fonts().unwrap_or_else(|err| {
        problems.push(format!("列不出本机字体：{err:#}"));
        Vec::new()
    });
    let root = state
        .db_path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    let (files, file_problems) =
        crate::fonts::font_files(&root.join("assets").join("fonts"), &imported);
    problems.extend(file_problems);
    let scope = app.asset_protocol_scope();
    for file in &files {
        if let Err(err) = scope.allow_file(&file.path) {
            problems.push(format!("{}：放行失败（{err}）", file.path));
        }
    }
    Ok(crate::fonts::FontCatalog {
        installed,
        files,
        problems,
    })
}

/// 一行歌词的振假名：要注音的那几段在 `text` 里的位置（按字符数）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineFurigana {
    pub utterance_id: i64,
    pub rubies: Vec<jp_tokenizer::furigana::Ruby>,
}

/// 整首歌的振假名。和 Python 版 `_furigana_tokens` 一样现场用 Sudachi（SplitMode C）分词取读音，
/// 全部 8443 行两种模式逐段对过账。一行约 0.02 ms，不入库。
#[tauri::command]
pub fn lyrics_furigana(
    state: State<'_, AppState>,
    song_id: String,
    mode: jp_tokenizer::furigana::FuriganaMode,
) -> CmdResult<Vec<LineFurigana>> {
    let analyzer = state
        .analyzer()
        .ok_or_else(|| anyhow::anyhow!(crate::tokenizer::no_dictionary("振假名")))?;
    let lines = state.corpus().lyrics(&song_id)?;
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let tokens = analyzer.analyze(&line.text)?;
        let segments = jp_tokenizer::furigana::furigana_segments(&line.text, &tokens, mode);
        // 对不上原文就这一行不注音，不猜位置
        let rubies = jp_tokenizer::furigana::ruby_spans(&line.text, &segments).unwrap_or_default();
        out.push(LineFurigana {
            utterance_id: line.utterance_id,
            rubies,
        });
    }
    Ok(out)
}

// ────────────────────────────── 检索 ──────────────────────────────

#[tauri::command]
pub async fn kwic<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    query: KwicQuery,
) -> CmdResult<Vec<KwicHit>> {
    // 常见词一查几千条，「只看日文」时还得全部建好再筛，放进阻塞线程池，不占主线程
    blocking(app, move |state| state.corpus().kwic(&query)).await
}

#[tauri::command]
pub fn search_lyrics(
    state: State<'_, AppState>,
    text: String,
    limit: Option<i64>,
) -> CmdResult<Vec<LyricHit>> {
    Ok(state.corpus().search_lyrics(&text, limit.unwrap_or(50))?)
}

// ────────────────────────────── 语料 ──────────────────────────────

#[tauri::command]
pub fn overview(state: State<'_, AppState>) -> CmdResult<Overview> {
    Ok(state.corpus().overview()?)
}

#[tauri::command]
pub fn timeline(state: State<'_, AppState>) -> CmdResult<Vec<YearStats>> {
    Ok(state.corpus().timeline()?)
}

#[tauri::command]
pub fn word_frequency(
    state: State<'_, AppState>,
    pos: Option<String>,
    limit: Option<i64>,
) -> CmdResult<Vec<WordFrequency>> {
    Ok(state
        .corpus()
        .word_frequency(pos.as_deref(), limit.unwrap_or(300))?)
}

/// Research Mode 的核心：点歌词里一个词，看它在整个语料里的样子。
#[tauri::command]
pub fn word_in_corpus(
    state: State<'_, AppState>,
    lemma: String,
    current_song_id: Option<String>,
    limit: Option<i64>,
) -> CmdResult<WordInCorpus> {
    Ok(state
        .corpus()
        .word_in_corpus(&lemma, current_song_id.as_deref(), limit.unwrap_or(20))?)
}

// ────────────────────────────── 播放历史 ──────────────────────────────

#[tauri::command]
pub fn record_play(state: State<'_, AppState>, event: PlayEvent) -> CmdResult<()> {
    Ok(state.corpus().record_play(&event)?)
}

#[tauri::command]
pub fn recently_played(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> CmdResult<Vec<PlayedTrack>> {
    Ok(state.corpus().recently_played(limit.unwrap_or(20))?)
}

#[tauri::command]
pub fn most_played(
    state: State<'_, AppState>,
    min_listened_sec: Option<f64>,
    limit: Option<i64>,
) -> CmdResult<Vec<PlayedTrack>> {
    Ok(state
        .corpus()
        .most_played(min_listened_sec.unwrap_or(30.0), limit.unwrap_or(20))?)
}

// ────────────────────────────── 分词 ──────────────────────────────

/// 实时分词。用于给还没入库的文本（比如用户粘贴的歌词）做可点击处理。
///
/// 已入库的歌词不要走这里——`lyrics` 直接带回库里的分词，
/// 那份和语料统计是一致的。
#[tauri::command]
pub fn tokenize(state: State<'_, AppState>, text: String) -> CmdResult<Vec<jp_tokenizer::Token>> {
    match state.analyzer() {
        Some(analyzer) => Ok(analyzer.analyze(&text)?),
        None => Err(anyhow::anyhow!(crate::tokenizer::no_dictionary("分词器")).into()),
    }
}

/// 应用自检。前端启动时调一次，缺什么直接告诉用户。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub db_path: String,
    pub tracks: i64,
    pub lyric_lines: i64,
    /// 分词器是否可用。false 时「实时分词」降级，其余功能正常。
    pub tokenizer_ready: bool,
    /// 没有可用输出设备时为 false，UI 据此禁用播放
    pub audio_ready: bool,
    /// 能不能变调：变调要 ffmpeg 渲染，本机没有 ffmpeg 时为 false，UI 据此禁用
    pub pitch_supported: bool,
}

#[tauri::command]
pub fn health(state: State<'_, AppState>) -> CmdResult<HealthReport> {
    let overview = state.corpus().overview()?;
    Ok(HealthReport {
        db_path: state.db_path.display().to_string(),
        tracks: overview.tracks,
        lyric_lines: overview.lyric_lines,
        tokenizer_ready: state.analyzer().is_some(),
        audio_ready: state.audio().is_some(),
        pitch_supported: state.audio().is_some() && state.pitch().available(),
    })
}

// ────────────────────────────── 播放 ──────────────────────────────
//
// 音频引擎不认识曲库，曲库也不认识音频引擎。两者由这一层接起来：
// command 拿 song_id 去 corpus 查 audio_path，再把路径交给引擎。
// 这正是要求书第十四条要的解耦。

fn audio<'a>(state: &'a State<'_, AppState>) -> Result<&'a jp_audio::AudioEngine, CommandError> {
    audio_engine(state).map_err(Into::into)
}

/// 同上，但拿的是 `&AppState`——放进阻塞线程池的命令只有这个
fn audio_engine(state: &AppState) -> anyhow::Result<&jp_audio::AudioEngine> {
    state
        .audio()
        .ok_or_else(|| anyhow::anyhow!("音频引擎不可用（没有可用的输出设备）"))
}

/// 按 song_id 加载，从 `position_sec`（默认开头）开始，`autoplay`（默认真）决定播不播。路径从曲库查，引擎只认路径。
///
/// 位置和播不播要一次传进来：设了变调而还没有缓存时要先渲染，渲染好才真正加载，
/// 分开再发的 seek / pause 那时还落不到播放器上。
#[tauri::command]
pub fn audio_load(
    state: State<'_, AppState>,
    song_id: String,
    position_sec: Option<f64>,
    autoplay: Option<bool>,
) -> CmdResult<()> {
    let track = state
        .corpus()
        .track(&song_id)?
        .ok_or_else(|| anyhow::anyhow!("找不到曲目 {song_id}"))?;
    if track.audio_path.trim().is_empty() {
        return Err(anyhow::anyhow!("《{}》没有音频文件", track.title).into());
    }
    let path = std::path::PathBuf::from(&track.audio_path);
    if !path.exists() {
        // 库里记的路径失效是常见情况（文件被移动/删除），
        // 要说清楚是哪个文件，而不是笼统报「加载失败」
        return Err(anyhow::anyhow!("音频文件不存在：{}", track.audio_path).into());
    }
    let deck = state
        .deck()
        .ok_or_else(|| anyhow::anyhow!("音频引擎不可用（没有可用的输出设备）"))?;
    state.pitch().load(
        deck,
        &song_id,
        &path,
        position_sec.unwrap_or(0.0),
        autoplay.unwrap_or(true),
    )?;
    Ok(())
}

// 播放 / 暂停 / 跳转：正在等变调渲染时先记下来，渲染好才生效

#[tauri::command]
pub fn audio_play(state: State<'_, AppState>) -> CmdResult<()> {
    let engine = audio(&state)?;
    if !state.pitch().intercept_play(Some(true)) {
        engine.play();
    }
    Ok(())
}

#[tauri::command]
pub fn audio_pause(state: State<'_, AppState>) -> CmdResult<()> {
    let engine = audio(&state)?;
    if !state.pitch().intercept_play(Some(false)) {
        engine.pause();
    }
    Ok(())
}

#[tauri::command]
pub fn audio_toggle(state: State<'_, AppState>) -> CmdResult<()> {
    let engine = audio(&state)?;
    if !state.pitch().intercept_play(None) {
        engine.toggle();
    }
    Ok(())
}

#[tauri::command]
pub fn audio_stop(state: State<'_, AppState>) -> CmdResult<()> {
    let engine = audio(&state)?;
    state.pitch().stop();
    engine.stop();
    Ok(())
}

/// 跳转。**异步**：`try_seek` 要等音频线程真的跳完才返回（解码器慢的时候是几十毫秒），
/// 同步命令跑在主线程上，拖进度条时整个界面会跟着顿。
#[tauri::command]
pub async fn audio_seek<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    position_sec: f64,
) -> CmdResult<()> {
    blocking(app, move |state| {
        let engine = audio_engine(state)?;
        if !state.pitch().intercept_seek(position_sec) {
            engine.seek(position_sec)?;
        }
        Ok(())
    })
    .await
}

/// 变调，−6…+6 半音，0 是原调。全局生效：正在放的这首跟着换，之后打开的歌也按这个调。
/// 没有缓存时在后台渲染（一首歌几秒），状态里 `pitchRendering` 为真；失败会退回原调，原因在 `pitchError`。
#[tauri::command]
pub fn audio_set_pitch(state: State<'_, AppState>, semitones: i32) -> CmdResult<()> {
    let deck = state
        .deck()
        .ok_or_else(|| anyhow::anyhow!("音频引擎不可用（没有可用的输出设备）"))?;
    state.pitch().set_semitones(deck, semitones)?;
    Ok(())
}

/// 设置倍速。**不改变音高**（WSOLA 时间伸缩）。
#[tauri::command]
pub fn audio_set_rate(state: State<'_, AppState>, rate: f32) -> CmdResult<()> {
    audio(&state)?.set_rate(rate);
    Ok(())
}

#[tauri::command]
pub fn audio_set_volume(state: State<'_, AppState>, volume: f32) -> CmdResult<()> {
    audio(&state)?.set_volume(volume);
    Ok(())
}

/// 单句循环：把区间设成那一句的起止。两个参数都省略则取消循环。
///
/// 返回 false 表示区间太短被拒——UI 应当提示，而不是当作成功。
#[tauri::command]
pub fn audio_set_loop(
    state: State<'_, AppState>,
    start_sec: Option<f64>,
    end_sec: Option<f64>,
) -> CmdResult<bool> {
    let region = match (start_sec, end_sec) {
        (Some(a), Some(b)) => Some((a, b)),
        _ => None,
    };
    Ok(audio(&state)?.set_loop(region))
}

/// 播放状态快照。前端轮询它来驱动进度条和歌词跟随。
///
/// 引擎不可用时返回默认状态而不是报错——UI 不该为了显示一个空进度条
/// 去处理异常。
#[tauri::command]
pub fn audio_state(state: State<'_, AppState>) -> CmdResult<jp_audio::PlaybackState> {
    Ok(state.playback_state())
}

/// 频谱帧。没有音频时返回全零，同样不报错。
#[tauri::command]
pub fn audio_spectrum(state: State<'_, AppState>) -> CmdResult<Vec<f32>> {
    let Some(engine) = state.audio() else {
        return Ok(vec![0.0; jp_audio::DEFAULT_BANDS]);
    };
    let mut analyzer = state.spectrum();
    let window = analyzer.window_len();
    Ok(analyzer.analyze(&engine.samples(window)))
}

// ────────────────────────────── 维护 ──────────────────────────────

/// 扫描 `duration_sec` 为空的曲目，读出时长写回库。
///
/// 实现在 `maintenance` 里（不依赖 Tauri，可离线测试，
/// 也能用 `cargo run -p jp-app --example backfill_durations` 跑）。
// 逐个音频文件解码探时长，整库是分钟级——同步 command 跑在主线程上会让整个窗口冻住
#[tauri::command(async)]
pub fn backfill_durations(
    state: State<'_, AppState>,
) -> CmdResult<crate::maintenance::DurationBackfillReport> {
    Ok(crate::maintenance::backfill_durations(&mut state.corpus())?)
}

/// 有歌词但一个 token 都没有的歌。
///
/// 这些歌是在没有分词词典的时候入库的，表现是**点词查不了、也没有振假名**——
/// 界面上这两件事都按 token 渲染。
#[tauri::command]
pub fn tokenize_missing_list(
    state: State<'_, AppState>,
) -> CmdResult<Vec<jp_import::maintain::Untokenized>> {
    Ok(jp_import::maintain::untokenized_songs(state.corpus().connection())?)
}

/// 给那些歌补上分词。一个事务，中途出错整个回滚。
///
/// 几十首歌几千行要分词，走 `spawn_blocking`。
#[tauri::command]
pub async fn tokenize_missing_run<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> CmdResult<jp_import::maintain::Tokenized> {
    let handle = app.clone();
    blocking(app, move |state| {
        anyhow::ensure!(
            state.analyzer().is_some(),
            "没有分词词典，先在「分词词典」那一行下一份"
        );
        tokenize_backlog(state, &handle)
    })
    .await
}

/// 有词典的话，把「有歌词、没分词」的歌补上分词；没词典或者没有缺的就什么都不做。
///
/// **为什么不只靠设置页那张卡**：没词典时入库的歌，词典装好以后不会自己好，
/// 而那张卡藏在「曲库维护」里——实际发生过：迁移时旧库里有分词的同名歌被判成
/// 「已在库中」跳过，留下的正好是没分词的那一份，16 首歌查不了词、没有振假名，
/// 用户只看得到症状。所以词典一装上（下载、复制、迁移带过来）就顺手补。
///
/// 一个事务：中途出错整个回滚，库里还是原来那样，设置页那张卡还能再点。
/// 只写 `tokens`（外加按原文套回分词校正），歌词和 metadata 一个字不动。
fn tokenize_backlog<R: tauri::Runtime>(
    state: &AppState,
    handle: &tauri::AppHandle<R>,
) -> anyhow::Result<jp_import::maintain::Tokenized> {
    let Some(analyzer) = state.analyzer() else {
        return Ok(Default::default());
    };
    let mut corpus = state.corpus();
    if jp_import::maintain::untokenized_songs(corpus.connection())?.is_empty() {
        return Ok(Default::default());
    }
    let tx = corpus.connection_mut().transaction()?;
    let report = jp_import::maintain::tokenize_missing(&tx, &analyzer, |song| {
        let _ = handle.emit(
            "tokenize://progress",
            format!("{} · {}", song.artist, song.title),
        );
    })?;
    tx.commit()?;
    if report.songs > 0 {
        crate::log::info(format!(
            "补齐分词：{} 首、{} 行、{} 个词",
            report.songs, report.lines, report.tokens
        ));
    }
    Ok(report)
}

/// 词典刚装上之后顺手补的分词。补失败不算装失败：词典确实装好了，
/// 失败原因放进 `tokenize_error`，设置页「补齐缺失分词」那张卡还能再点。
fn backlog_after_install<R: tauri::Runtime>(
    state: &AppState,
    handle: &tauri::AppHandle<R>,
) -> (jp_import::maintain::Tokenized, String) {
    match tokenize_backlog(state, handle) {
        Ok(report) => (report, String::new()),
        Err(err) => {
            crate::log::warn(format!("词典装好了，但顺手补分词失败（已回滚）：{err:#}"));
            (Default::default(), format!("{err:#}"))
        }
    }
}

/// 装词典的结果：装了什么 + 顺手补了多少首歌的分词。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryReady {
    #[serde(flatten)]
    pub installed: crate::tokenizer::Installed,
    pub tokenized: jp_import::maintain::Tokenized,
    /// 补分词失败的原因；空串表示没失败（包括「没有要补的」）
    pub tokenize_error: String,
}

// ──────────────────────────── 补齐歌词 ────────────────────────────

/// 库里还没有歌词的歌。音频旁边就有 .lrc 的会在 `siblingLrc` 里标出来。
#[tauri::command]
pub fn lyrics_missing(state: State<'_, AppState>) -> CmdResult<Vec<crate::lyrics::MissingLyrics>> {
    Ok(crate::lyrics::missing(state.corpus().connection())?)
}

/// 批量补齐，后台跑。进度走 `lyrics://progress` 事件。
///
/// `songIds` 省略就是「全库缺歌词的都补」。
#[tauri::command]
pub fn lyrics_fill_start<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    song_ids: Option<Vec<String>>,
) -> CmdResult<()> {
    crate::lyrics::spawn_fill(
        app,
        state.lyrics_job(),
        state.db_path.clone(),
        state.lyrics_dir.clone(),
        song_ids,
    )?;
    Ok(())
}

#[tauri::command]
pub fn lyrics_fill_cancel(state: State<'_, AppState>) -> CmdResult<()> {
    state.lyrics_job().request_cancel();
    Ok(())
}

#[tauri::command]
pub fn lyrics_fill_running(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(state.lyrics_job().is_running())
}

/// 单首：先看音频旁边有没有 .lrc，没有再上网搜。挑不出来返回 null。
///
/// 异步 + `spawn_blocking`：联网那一步可能要几秒，占着主线程的话整个界面会卡住。
#[tauri::command]
pub async fn lyrics_fill_one<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    song_id: String,
) -> CmdResult<Option<crate::lyrics::Attached>> {
    blocking(app, move |state| {
        let targets = crate::lyrics::missing(state.corpus().connection())?;
        let Some(target) = targets.into_iter().find(|t| t.song_id == song_id) else {
            anyhow::bail!("{song_id} 已经有歌词了，先清掉再补，或者直接导入一份文件覆盖");
        };
        let provider = crate::lyrics::provider();
        let lyrics_dir = state.lyrics_dir.clone();
        let mut corpus = state.corpus();
        crate::lyrics::fill_one(
            corpus.connection_mut(),
            &lyrics_dir,
            &provider,
            &target,
            state.analyzer().as_deref(),
        )
    })
    .await
}

/// 用户自己选的一份歌词文件（.lrc / .txt）挂到这首歌上。
///
/// **替换**：这首歌已有的歌词行、分词、全文索引先清掉再挂新的，
/// 不会出现两份歌词。用户校正过的分词会按原文套回去，不会丢。
#[tauri::command]
pub async fn lyrics_import_file<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    song_id: String,
    path: String,
) -> CmdResult<crate::lyrics::Attached> {
    blocking(app, move |state| {
        let lyrics_dir = state.lyrics_dir.clone();
        let mut corpus = state.corpus();
        crate::lyrics::attach_from_file(
            corpus.connection_mut(),
            &lyrics_dir,
            &song_id,
            std::path::Path::new(&path),
            state.analyzer().as_deref(),
        )
    })
    .await
}

// ──────────────────────────── 检查更新 ────────────────────────────

/// 问一次 GitHub Releases：有没有比正在跑的这一版更新的。
///
/// 异步 + `spawn_blocking`：网络那一步几百毫秒到几秒，占着主线程界面会卡。
#[tauri::command]
pub async fn update_check<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> CmdResult<crate::update::UpdateStatus> {
    let current = app.package_info().version.to_string();
    let value = tauri::async_runtime::spawn_blocking(move || crate::update::check(&current))
        .await
        .map_err(|err| anyhow::anyhow!("检查更新线程出错：{err}"))??;
    Ok(value)
}

/// 最近几次发布，给「查看更新日志」用。
#[tauri::command]
pub async fn update_changelog(limit: Option<usize>) -> CmdResult<Vec<crate::update::ReleaseView>> {
    let value =
        tauri::async_runtime::spawn_blocking(move || crate::update::changelog(limit.unwrap_or(10)))
            .await
            .map_err(|err| anyhow::anyhow!("读更新日志线程出错：{err}"))??;
    Ok(value)
}

/// 下载安装包。进度走 `update://progress` 事件，返回落盘路径。
///
/// **地址和哈希都来自同一次 `update_check`**：前端把那条资产原样传回来，
/// `download_installer` 再核一遍地址是不是本仓库的、哈希对不对。
#[tauri::command]
pub async fn update_download<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    asset: crate::update::AssetView,
) -> CmdResult<String> {
    let dir = app
        .path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("updates");
    let handle = app.clone();
    let expected = asset.sha256.clone().unwrap_or_default();
    let path = tauri::async_runtime::spawn_blocking(move || {
        crate::update::download_installer(&asset, &dir, |received, total| {
            let _ = handle.emit(
                "update://progress",
                crate::update::DownloadProgress {
                    received,
                    total,
                    stage: "downloading".into(),
                    message: String::new(),
                },
            );
        })
    })
    .await
    .map_err(|err| anyhow::anyhow!("下载线程出错：{err}"))??;
    // 记下来：`update_install` 只认这一个文件
    app.state::<AppState>()
        .updates()
        .remember_download(path.clone(), expected);
    let _ = app.emit(
        "update://progress",
        crate::update::DownloadProgress {
            received: 1,
            total: 1,
            stage: "done".into(),
            message: path.display().to_string(),
        },
    );
    Ok(path.display().to_string())
}

/// 拉起安装程序并退出。**NSIS 要替换正在运行的 exe，所以必须先退。**
///
/// **不收路径**：只启动 `update_download` 这一轮下好、校验过的那个文件
/// （见 [`crate::update::UpdateMemory`]）。路径由前端给的话，前端就能让程序执行任意 exe。
///
/// 退出前把未结束的收听会话冲刷掉，不然这一段听歌记录会丢。
#[tauri::command]
pub fn update_install<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    let path = state.updates().installer_to_launch()?;
    crate::update::launch_installer(&path)?;
    state.flush_session();
    app.exit(0);
    Ok(())
}

// ──────────────────────────── 数据迁移 ────────────────────────────

/// 预览：把 `path` 那个语料库目录的数据搬过来会发生什么。**只读**。
#[tauri::command]
pub async fn migrate_plan<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    path: String,
) -> CmdResult<jp_import::migrate::MigratePlan> {
    blocking(app, move |state| {
        let root = std::path::PathBuf::from(&path);
        anyhow::ensure!(
            crate::migrate::source_db(&root).is_file(),
            "{} 里没有 corpus.db",
            root.display()
        );
        anyhow::ensure!(
            root != state.db_path.parent().unwrap_or(&root),
            "源目录就是当前库，不用迁移"
        );
        crate::migrate::plan(state.corpus().connection(), &root)
    })
    .await
}

/// 真的搬。进度走 `migrate://progress` 事件。
///
/// 两百多万行词条 + 两百首歌的歌词分词，整件事几十秒，所以走 `spawn_blocking`，
/// 并且每一步报一句——否则界面只能干等。
#[tauri::command]
pub async fn migrate_run<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    path: String,
    options: jp_import::migrate::MigrateOptions,
) -> CmdResult<crate::migrate::MigrateOutcome> {
    let handle = app.clone();
    blocking(app, move |state| {
        let source = std::path::PathBuf::from(&path);
        let target_root = state
            .db_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        anyhow::ensure!(source != target_root, "源目录就是当前库，不用迁移");
        let mut corpus = state.corpus();
        let outcome = crate::migrate::run(
            corpus.connection_mut(),
            &source,
            &target_root,
            options,
            |step| {
                let _ = handle.emit("migrate://progress", step.to_string());
            },
        );
        drop(corpus);
        // 搬过来的 sudachi/ 立刻生效，不用重启——振假名是用户马上会去看的东西
        state.reload_analyzer();
        let mut outcome = outcome?;
        // 当前库里没分词的那几首，旧库里的同名歌会被判成「已在库中」跳过，
        // 分词不会跟着过来（两边的歌词往往也不是同一份，逐行对不上，不能硬搬）。
        // 词典现在有了，就拿它们自己的歌词补上。
        let _ = handle.emit("migrate://progress", "正在给没分词的歌补分词…".to_string());
        let (tokenized, tokenize_error) = backlog_after_install(state, &handle);
        outcome.tokenized = tokenized;
        if !tokenize_error.is_empty() {
            outcome
                .warnings
                .push(format!("补分词失败（已回滚，可在「补齐缺失分词」重试）：{tokenize_error}"));
        }
        Ok(outcome)
    })
    .await
}

// ──────────────────────────── 诊断 ────────────────────────────

/// 一段纯文本：版本、库在哪、库里有多少、能力、最近的日志。
///
/// **不联网、不自动上报**——攒出来给用户，发不发、发给谁他自己定。
#[tauri::command]
pub fn diagnostics_report<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
) -> CmdResult<String> {
    let version = app.package_info().version.to_string();
    Ok(crate::diagnostics::report(&state, &version, 200))
}

/// 把那段文本存成文件。路径由前端的保存对话框给。
#[tauri::command]
pub fn diagnostics_save<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<String> {
    let version = app.package_info().version.to_string();
    let text = crate::diagnostics::report(&state, &version, 200);
    std::fs::write(&path, text.as_bytes())
        .map_err(|err| anyhow::anyhow!("写不进 {path}：{err}"))?;
    Ok(path)
}

// ──────────────────────────── 分词词典 ────────────────────────────

/// 当前语料库目录里有没有 Sudachi 词典。
#[tauri::command]
pub fn tokenizer_status(state: State<'_, AppState>) -> CmdResult<crate::tokenizer::TokenizerStatus> {
    Ok(crate::tokenizer::status(&state.project_root()))
}

/// 去网上下一份词典，解压进当前语料库目录，并立刻装上。
///
/// 43MB 的下载 + 207MB 的解压，几分钟起步，所以进度走 `tokenizer://progress` 事件。
#[tauri::command]
pub async fn tokenizer_download<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> CmdResult<DictionaryReady> {
    let handle = app.clone();
    blocking(app, move |state| {
        let installed = crate::tokenizer::download(&state.project_root(), |progress| {
            let _ = handle.emit("tokenizer://progress", progress);
        })?;
        anyhow::ensure!(state.reload_analyzer(), "下好了，但词典还是装不上");
        let (tokenized, tokenize_error) = backlog_after_install(state, &handle);
        Ok(DictionaryReady { installed, tokenized, tokenize_error })
    })
    .await
}

/// 把别处的那一份 Sudachi 词典复制进当前语料库目录，并立刻装上。
///
/// 207MB 的复制，所以走 `spawn_blocking`。
#[tauri::command]
pub async fn tokenizer_install<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    path: String,
) -> CmdResult<DictionaryReady> {
    let handle = app.clone();
    blocking(app, move |state| {
        let installed =
            crate::tokenizer::install(&state.project_root(), std::path::Path::new(&path))?;
        anyhow::ensure!(state.reload_analyzer(), "复制完了，但词典还是装不上");
        let (tokenized, tokenize_error) = backlog_after_install(state, &handle);
        Ok(DictionaryReady { installed, tokenized, tokenize_error })
    })
    .await
}

/// 播放状态 + 走一拍收听统计。**前端的轮询应当调这个，不是 `audio_state`。**
///
/// 名字里带 tick 是因为它**有副作用**：会把收听时长累加进会话，
/// 并在换歌/停止时把上一段写进 `play_history`。
/// `audio_state` 保持无副作用，给不想触发统计的场合用。
#[tauri::command]
pub fn audio_tick(state: State<'_, AppState>) -> CmdResult<jp_audio::PlaybackState> {
    Ok(state.tick())
}

// ────────────────────────────── 收藏 ──────────────────────────────

#[tauri::command]
pub fn toggle_favorite(
    state: State<'_, AppState>,
    entity_type: String,
    entity_id: String,
) -> CmdResult<bool> {
    Ok(state.corpus().toggle_favorite(&entity_type, &entity_id)?)
}

#[tauri::command]
pub fn is_favorite(
    state: State<'_, AppState>,
    entity_type: String,
    entity_id: String,
) -> CmdResult<bool> {
    Ok(state.corpus().is_favorite(&entity_type, &entity_id)?)
}

// ────────────────────────────── 首页 ──────────────────────────────

/// 首页要的一切。
///
/// 打成一个 command 而不是让前端发六次：首页一进来就要全部数据，
/// 分开发不仅多五次往返，还可能拿到互相不一致的快照
/// （比如统计已经更新、最近播放还是旧的）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeSummary {
    pub overview: Overview,
    pub recently_played: Vec<PlayedTrack>,
    pub most_played: Vec<PlayedTrack>,
    pub favorites: Vec<Track>,
    pub albums: Vec<Album>,
    pub genres: Vec<FacetCount>,
    pub decades: Vec<FacetCount>,
    /// 语料里词频最高的几个词，作为「从这里开始查」的入口
    pub top_words: Vec<WordFrequency>,
}

#[tauri::command]
pub fn home_summary(state: State<'_, AppState>) -> CmdResult<HomeSummary> {
    let corpus = state.corpus();
    Ok(HomeSummary {
        overview: corpus.overview()?,
        recently_played: corpus.recently_played(12)?,
        most_played: corpus.most_played(30.0, 12)?,
        favorites: corpus.favorite_tracks(12)?,
        albums: corpus.albums(None, 12)?,
        genres: corpus.genres(12)?,
        decades: corpus.decades(12)?,
        top_words: corpus.word_frequency(Some("NOUN"), 24)?,
    })
}

/// 现在用的是哪个语料库目录，以及是怎么找到的。设置页要如实显示
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRootInfo {
    pub path: String,
    pub source: crate::library_root::RootSource,
    /// 设置里记着的那个目录（没设过就是空串）
    pub remembered: String,
    /// 这个目录里现在是不是一个建好表的库
    pub ready: bool,
}

/// 语料库目录的现状。
#[tauri::command]
pub fn library_root(state: State<'_, AppState>) -> CmdResult<LibraryRootInfo> {
    let (path, source) = crate::library_root::locate();
    Ok(LibraryRootInfo {
        path: state
            .db_path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| path.display().to_string()),
        source,
        remembered: crate::library_root::remembered()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        ready: true,
    })
}

/// 换一个语料库目录。传空串表示清掉、回到自动查找。
///
/// **只写设置，不动这次运行**：数据库连接和 asset 放行范围都是启动时定下的，
/// 半路换等于整个 AppState 重建。所以这里返回一句提示，由界面告诉用户重启。
#[tauri::command]
pub fn set_library_root(path: Option<String>) -> CmdResult<String> {
    let Some(path) = path.filter(|p| !p.trim().is_empty()) else {
        crate::library_root::remember(None)?;
        return Ok("已清除，下次启动按默认顺序查找".into());
    };
    let dir = std::path::PathBuf::from(path.trim());
    if !dir.is_dir() {
        return Err(anyhow::anyhow!("{} 不是一个目录", dir.display()).into());
    }
    let has_db = dir.join("corpus.db").is_file();
    if has_db && !crate::library_root::looks_like_library(&dir) {
        // 这里多半是上一版留下的空壳文件；直接用会在启动时缺表退出
        return Err(anyhow::anyhow!(
            "{} 里的 corpus.db 不是一个建好表的语料库。换一个目录，或者把那个文件删掉再选这里（会新建一个空库）。",
            dir.display()
        )
        .into());
    }
    crate::library_root::remember(Some(&dir))?;
    Ok(if has_db {
        format!("下次启动用 {}", dir.display())
    } else {
        format!("下次启动会在 {} 新建一个空库", dir.display())
    })
}

// ────────────────────────────── 全局搜索 ──────────────────────────────

/// Cmd+K 的全局搜索。跨曲目 / 专辑 / 人物 / 词汇 / 歌词，分组返回。
///
/// 分组而不是混排：跨类型的相关性分数没有可比性，硬凑的全局排序是假的。
#[tauri::command]
pub fn quick_search(
    state: State<'_, AppState>,
    query: String,
    per_kind: Option<i64>,
) -> CmdResult<QuickSearchResults> {
    Ok(state.corpus().quick_search(&query, per_kind)?)
}

/// 按 id 取人。命令面板点一条人物结果时用它定位。
#[tauri::command]
pub fn person_by_id(
    state: State<'_, AppState>,
    person_id: i64,
) -> CmdResult<Option<PersonSummary>> {
    Ok(state.corpus().person_by_id(person_id)?)
}

// ────────────────────────────── 导入 ──────────────────────────────

/// 复核用的一条。不把整个 `ScannedTrack` 端上去——那里面一半字段
/// 是推断过程的中间产物，UI 用不上，白白撑大 payload。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItemView {
    pub path: String,
    pub file_name: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_sec: Option<f64>,
    pub has_lyrics: bool,
    /// 读 tag 时出的问题。为空表示一切正常。
    pub warning: Option<String>,
    pub action: jp_import::Action,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub summary: jp_import::PlanSummary,
    pub items: Vec<PlanItemView>,
    /// 分词器可不可用。不可用时导入仍能进行，只是不写 tokens——
    /// UI 要在按下「导入」之前就把这件事说清楚。
    pub tokenizer_ready: bool,
    /// 选中但不是音频、因此没进计划的文件名。
    ///
    /// 单曲导入时用户可能顺手选中了 .lrc 或封面图。默不作声地丢掉
    /// 会让人以为「选了 5 个却只导了 3 个」是 bug，所以原样报出来。
    #[serde(default)]
    pub ignored: Vec<String>,
}

/// 扫描一个目录（或单个音频文件），和现有曲库比对，返回一份计划供复核。
///
/// **只读，什么都不写。** 真正写库要再调一次 `run_import`。
// 遍历整个音乐目录、逐个文件读 tag，几百首要好几秒到几十秒——同步 command 跑在主线程上会让整个窗口冻住
#[tauri::command(async)]
pub fn scan_folder(
    state: State<'_, AppState>,
    path: String,
    max_depth: Option<usize>,
) -> CmdResult<ScanResult> {
    let root = std::path::Path::new(&path);
    if !root.exists() {
        return Err(anyhow::anyhow!("路径不存在：{path}").into());
    }
    Ok(plan_targets(&state, &[path], max_depth)?)
}

/// 扫描一批**具体的文件**（也允许夹着目录），同上只读。
///
/// 为什么需要它：以前只能选整个文件夹，而「音乐 App 下载目录里新增的那一首」
/// 没法单独导——用户得么把文件挪进一个临时目录，要么扫整个目录再在几百条
/// 计划里找那一条。`scan_dir` 本来就认单个文件，缺的只是一个入口。
// 同 `scan_folder`——同步 command 跑在主线程上会让整个窗口冻住
#[tauri::command(async)]
pub fn scan_files(
    state: State<'_, AppState>,
    paths: Vec<String>,
    max_depth: Option<usize>,
) -> CmdResult<ScanResult> {
    if paths.is_empty() {
        return Err(anyhow::anyhow!("一个文件都没选").into());
    }
    for path in &paths {
        if !std::path::Path::new(path).exists() {
            return Err(anyhow::anyhow!("路径不存在：{path}").into());
        }
    }
    Ok(plan_targets(&state, &paths, max_depth)?)
}

/// 扫 + 比对 + 记下待确认的结果。`scan_folder` 和 `scan_files` 的共同实现。
///
/// 同一个文件被选中两次（选了文件又选了它所在的目录）只算一次：
/// 按路径去重在这里做，不然计划里会出现两条一模一样的、还互相判成「本批重复」。
fn plan_targets(
    state: &AppState,
    paths: &[String],
    max_depth: Option<usize>,
) -> anyhow::Result<ScanResult> {
    let depth = max_depth.unwrap_or(8);
    let mut tracks: Vec<jp_import::ScannedTrack> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut ignored: Vec<String> = Vec::new();

    for path in paths {
        let target = std::path::Path::new(path);
        let found = jp_import::scan_dir(target, depth);
        if found.is_empty() && target.is_file() {
            // 不是音频（.lrc、封面图、歌单文件……）。扫目录时这种文件本来就不计数，
            // 但用户明确选中它的时候要说一声。
            ignored.push(
                target
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.clone()),
            );
            continue;
        }
        for track in found {
            if seen.insert(track.path.clone()) {
                tracks.push(track);
            }
        }
    }
    tracks.sort_by(|a, b| a.path.cmp(&b.path));

    let index = jp_import::LibraryIndex::from_corpus(&state.corpus())?;
    let plan = jp_import::plan(&tracks, &index);

    let items = plan
        .items
        .iter()
        .map(|item| PlanItemView {
            path: item.track.path.clone(),
            file_name: item.track.file_name.clone(),
            title: item.track.title().to_string(),
            artist: item.track.artist().to_string(),
            album: item.track.album().to_string(),
            duration_sec: item.track.duration_sec,
            has_lyrics: item.track.lyrics_path.is_some(),
            warning: item.track.warning.clone(),
            action: item.action.clone(),
        })
        .collect();
    let summary = plan.summary();

    // 记下扫描结果，等确认。存扫描结果而不是计划：导入时会重新算，
    // 免得扫描之后库变了导致 id 分配失效。
    *state.pending_scan() = tracks;

    Ok(ScanResult {
        summary,
        items,
        tokenizer_ready: state.analyzer().is_some(),
        ignored,
    })
}

/// 一次导入的进度。`import://progress` 事件。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub done: usize,
    pub total: usize,
    /// 刚导完的那首，给界面显示「正在导：…」
    pub title: String,
    pub finished: bool,
    pub cancelled: bool,
}

/// 把复核过的那批真正写进库。
///
/// **跑在阻塞线程池里**：几百首歌连带分词要好几分钟，原来这是个同步 command，
/// 直接占着主线程——整个窗口在导完之前一动不动，连进度都显示不了。
///
/// 一首歌一个事务，所以中途 [`cancel_import`] 停下来是安全的：
/// 停在哪儿，哪儿之前的就是真导进去了，之后一行都没写。
#[tauri::command]
pub async fn run_import<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    tokenize: Option<bool>,
) -> CmdResult<crate::library_admin::Maintained<jp_import::ImportReport>> {
    let handle = app.clone();
    blocking(app, move |state| {
        let tracks = state.pending_scan().clone();
        anyhow::ensure!(!tracks.is_empty(), "没有待导入的条目，先调 scan_folder");
        let analyzer = if tokenize.unwrap_or(true) {
            state.analyzer()
        } else {
            None
        };

        let job = state.import_job();
        let guard = job
            .start()
            .ok_or_else(|| anyhow::anyhow!("已经有一个导入在跑"))?;

        let report = {
            let mut corpus = state.corpus();
            // 拿当前曲库重新算计划：扫描到确认之间库可能已经变了
            let index = jp_import::LibraryIndex::from_corpus(&corpus)?;
            let plan = jp_import::plan(&tracks, &index);
            let total = plan.to_import().count();
            jp_import::execute_cancellable(
                corpus.connection_mut(),
                &plan,
                analyzer.as_deref(),
                |track, done, _| {
                    let _ = handle.emit(
                        "import://progress",
                        ImportProgress {
                            done,
                            total,
                            title: track.title.clone(),
                            ..Default::default()
                        },
                    );
                },
                &|| job.cancelled(),
            )?
        };
        drop(guard);
        let _ = handle.emit(
            "import://progress",
            ImportProgress {
                done: report.tracks.len(),
                total: report.tracks.len(),
                finished: true,
                cancelled: report.cancelled,
                ..Default::default()
            },
        );

        // 导完就清掉，避免同一批被点两次
        state.pending_scan().clear();
        // 新歌追加进 metadata/songs.csv（Python 版加歌时也追加）
        let csv = crate::library_admin::sync_imported(state, &report);
        Ok(crate::library_admin::Maintained { report, csv })
    })
    .await
}

// ────────────────────────────── 统计 ──────────────────────────────

/// 统计要扫整张 tokens 表（报告一次一两百毫秒），同步命令会卡住主线程，放进阻塞线程池
async fn blocking<R: tauri::Runtime, T: Send + 'static>(
    app: tauri::AppHandle<R>,
    job: impl FnOnce(&AppState) -> anyhow::Result<T> + Send + 'static,
) -> CmdResult<T> {
    let value = tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        job(&app.state::<AppState>())
    })
    .await
    .map_err(|err| anyhow::anyhow!("统计线程出错：{err}"))??;
    Ok(value)
}

/// 统计表：词元 × 词性的频次，带 JLPT。可按演唱者、只看日文筛选。对应 Python 版统计页。
#[tauri::command]
pub async fn stats_frequency<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    pos: Option<String>,
    filter: Option<jp_corpus::stats::StatsFilter>,
    limit: Option<usize>,
) -> CmdResult<Vec<jp_corpus::stats::FrequencyRow>> {
    blocking(app, move |state| {
        let pos = pos.filter(|p| !p.is_empty());
        jp_corpus::stats::frequency_table(
            state.corpus().connection(),
            pos.as_deref(),
            &filter.unwrap_or_default(),
            limit.unwrap_or(300).clamp(1, 5000),
        )
    })
    .await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsReport {
    pub report: jp_corpus::stats::CorpusReport,
    /// 筛选的演唱者名字，按传进来的顺序；库里找不到的 id 跳过
    pub performers: Vec<String>,
    pub jp_only: bool,
    /// 导出 TXT 的内容，和这份报告一起算好：导出的就是界面上看到的那份
    pub text: String,
}

/// 语料统计报告（TTR / STTR / Hapax / 词性分布 / 覆盖率）。对应 Python 版报告页。
#[tauri::command]
pub async fn stats_report<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    filter: Option<jp_corpus::stats::StatsFilter>,
) -> CmdResult<StatsReport> {
    blocking(app, move |state| {
        let filter = filter.unwrap_or_default();
        let corpus = state.corpus();
        let conn = corpus.connection();
        let mut performers = Vec::new();
        for id in &filter.performer_ids {
            let name: Option<String> = conn
                .query_row("SELECT name FROM people WHERE id = ?1", [id], |r| r.get(0))
                .map(Some)
                .or_else(|err| {
                    if err == rusqlite::Error::QueryReturnedNoRows {
                        Ok(None)
                    } else {
                        Err(err)
                    }
                })?;
            performers.extend(name);
        }
        let report = jp_corpus::stats::corpus_report(conn, &filter)?;
        let text = jp_corpus::stats::report_text(&report, &performers, filter.jp_only);
        Ok(StatsReport {
            report,
            performers,
            jp_only: filter.jp_only,
            text,
        })
    })
    .await
}

/// 挖词报告：读 Anki 的 collection（复制一份再读），写 `output/corpus_report.html` 并用浏览器打开。
/// 对应 PyQt 版菜单「生成挖词报告…」。`now` 是界面给的本地时间（`YYYY-MM-DD HH:MM`）。
#[tauri::command]
pub async fn anki_mining_report<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    now: String,
    deck_contains: Option<String>,
) -> CmdResult<crate::anki_report::MiningReportResult> {
    blocking(app, move |state| {
        let collection = jp_anki::learning::find_collection().ok_or_else(|| {
            anyhow::anyhow!("找不到 Anki 的 collection.anki2：装了 Anki 并且至少打开过一次吗？")
        })?;
        let levels = crate::anki_report::jlpt_levels(state.corpus().connection())?;
        let root = state
            .db_path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_default();
        let mut result = crate::anki_report::generate(
            &root,
            &collection,
            &levels,
            &now,
            deck_contains.as_deref().unwrap_or(""),
        )?;
        if let Err(err) = crate::anki_report::open_in_browser(std::path::Path::new(&result.path)) {
            result.open_error = format!("{err:#}");
        }
        Ok(result)
    })
    .await
}

/// 把界面生成的文本（统计报告 TXT、检索结果 CSV、挖词报告 HTML）写到用户在保存对话框里选的位置。
/// 只写这几种扩展名，免得被拿去覆盖别的文件。
#[tauri::command]
pub fn export_text(path: String, content: String) -> CmdResult<()> {
    let target = std::path::Path::new(&path);
    let ext = target
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !matches!(ext.as_str(), "txt" | "csv" | "html") {
        return Err(anyhow::anyhow!("只能导出 .txt / .csv / .html：{path}").into());
    }
    std::fs::write(target, content).map_err(|err| anyhow::anyhow!("写不了 {path}：{err}"))?;
    Ok(())
}

// ────────────────────────────── 曲库维护 ──────────────────────────────

/// 改曲目信息（歌名、歌手、年份、专辑、流派）。署名、专辑、分词校正跟着改，`metadata/songs.csv` 同步。
#[tauri::command]
pub fn library_edit_song(
    state: State<'_, AppState>,
    song_id: String,
    edit: jp_import::maintain::SongEdit,
) -> CmdResult<crate::library_admin::Maintained<jp_import::maintain::EditReport>> {
    Ok(crate::library_admin::edit_song(&state, &song_id, &edit)?)
}

/// 删一首歌：库里挂在它身上的都删（分词校正留着，重导时恢复），不删音频文件。
#[tauri::command]
pub fn library_delete_song(
    state: State<'_, AppState>,
    song_id: String,
) -> CmdResult<crate::library_admin::Maintained<crate::library_admin::DeleteResult>> {
    Ok(crate::library_admin::delete_song(&state, &song_id)?)
}

/// 音频文件找不到的曲目。
#[tauri::command]
pub fn library_missing_audio(
    state: State<'_, AppState>,
) -> CmdResult<Vec<jp_import::maintain::MissingAudio>> {
    Ok(crate::library_admin::missing_audio(&state)?)
}

/// 扫一个目录给丢了音频的歌找新文件（只读）。目录大时要几秒，放进阻塞线程池。
#[tauri::command]
pub async fn library_suggest_relinks<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    folder: String,
) -> CmdResult<Vec<jp_import::maintain::RelinkSuggestion>> {
    let suggestions = tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        crate::library_admin::suggest_relinks(
            &app.state::<AppState>(),
            std::path::Path::new(&folder),
        )
    })
    .await
    .map_err(|err| anyhow::anyhow!("扫描线程出错：{err}"))??;
    Ok(suggestions)
}

/// 把这些歌的音频换成新文件。一首一个事务，逐首报结果。
#[tauri::command]
pub fn library_relink_audio(
    state: State<'_, AppState>,
    links: Vec<crate::library_admin::RelinkRequest>,
) -> CmdResult<crate::library_admin::RelinkBatch> {
    Ok(crate::library_admin::relink_audio(&state, &links)?)
}

/// 取消导入。复核界面点「取消」、以及导入过程中点「停止」都调它。
///
/// 两件事一起做：
///
/// * 丢弃上一次扫描的结果（还没开始导的情况）；
/// * 给正在跑的导入置取消标志——它每导完一首问一次，所以最多再多导一首。
///   **这是安全的**：一首歌一个事务，停在哪儿哪儿之前就是真导进去了。
///
/// 原来它只做第一件事，导入中点「取消」什么都不会发生（而且那时候
/// 整个窗口还是卡住的，连按钮都点不了）。
#[tauri::command]
pub fn cancel_import(state: State<'_, AppState>) -> CmdResult<()> {
    state.import_job().request_cancel();
    state.pending_scan().clear();
    Ok(())
}

// ────────────────────────────── 刮削 ──────────────────────────────

/// 复核队列里的一条：状态 + 候选，一次给全，UI 不用再逐条问。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewItem {
    pub file_path: String,
    pub song_id: String,
    pub status: String,
    pub confidence: f64,
    pub provider: String,
    pub error_type: String,
    pub error_message: String,
    pub retry_count: i64,
    pub last_attempt_at: String,
    /// 本地文件现在写的是什么，用来和候选对照
    pub local_title: String,
    pub local_artist: String,
    pub local_album: String,
    /// 候选列表，已按分数排好
    pub candidates: Vec<jp_scraper::ScrapeCandidate>,
    /// 最佳候选的打分解释
    pub explain: String,
}

#[tauri::command]
pub fn scrape_summary(state: State<'_, AppState>) -> CmdResult<jp_scraper::ScrapeSummary> {
    let corpus = state.corpus();
    let store = jp_scraper::ScrapeStore::new(corpus.connection());
    store.ensure_schema()?;
    Ok(store.summary()?)
}

/// 复核队列。默认取需要人工确认的，也可以点名要别的状态。
#[tauri::command]
pub fn scrape_review_queue(
    state: State<'_, AppState>,
    statuses: Option<Vec<String>>,
    limit: Option<i64>,
) -> CmdResult<Vec<ReviewItem>> {
    let corpus = state.corpus();
    let conn = corpus.connection();
    let store = jp_scraper::ScrapeStore::new(conn);
    store.ensure_schema()?;

    let wanted: Vec<jp_scraper::ScrapeStatus> = statuses
        .unwrap_or_else(|| vec!["low_confidence".into()])
        .iter()
        .map(|s| jp_scraper::ScrapeStatus::from_str_lossy(s))
        .collect();
    let records = store.by_status(&wanted, Some(limit.unwrap_or(200)))?;

    let mut out = Vec::with_capacity(records.len());
    for record in records {
        let candidates: Vec<jp_scraper::ScrapeCandidate> = record
            .candidates
            .clone()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let explain = candidates
            .first()
            .and_then(|c| c.breakdown.as_ref())
            .map(|b| b.explain())
            .unwrap_or_default();
        // 本地写的是什么：取原始元数据而不是可能已被覆盖的 songs 行
        let local = store.original_metadata(&record.file_path).ok().flatten();
        out.push(ReviewItem {
            local_title: local
                .as_ref()
                .map(|t| t.title().to_string())
                .unwrap_or_default(),
            local_artist: local
                .as_ref()
                .map(|t| t.artist().to_string())
                .unwrap_or_default(),
            local_album: local
                .as_ref()
                .map(|t| t.album().to_string())
                .unwrap_or_default(),
            file_path: record.file_path,
            song_id: record.song_id,
            status: record.status.as_str().to_string(),
            confidence: record.confidence,
            provider: record.provider,
            error_type: record.error_type.as_str().to_string(),
            error_message: record.error_message,
            retry_count: record.retry_count,
            last_attempt_at: record.last_attempt_at,
            candidates,
            explain,
        });
    }
    Ok(out)
}

/// 某个文件的尝试历史。「这首为什么反复失败」用它回答。
#[tauri::command]
pub fn scrape_attempts(
    state: State<'_, AppState>,
    file_path: String,
    limit: Option<i64>,
) -> CmdResult<Vec<jp_scraper::ScrapeAttempt>> {
    let corpus = state.corpus();
    let store = jp_scraper::ScrapeStore::new(corpus.connection());
    store.ensure_schema()?;
    Ok(store.attempts(&file_path, limit.unwrap_or(20))?)
}

/// 刮一首。同步跑，一两秒。
#[tauri::command]
pub fn scrape_track(
    state: State<'_, AppState>,
    song_id: String,
) -> CmdResult<jp_scraper::ScrapeSummary> {
    let corpus = state.corpus();
    let conn = corpus.connection();
    let Some(track) = crate::scrape::track_file_for(conn, &song_id)? else {
        return Err(anyhow::anyhow!("库里没有 id={song_id} 这首歌").into());
    };
    let resolver = jp_scraper::default_resolver(false)?;
    let outcome = crate::scrape::scrape_one(conn, &resolver, &song_id, &track)?;
    // 单首是同步的，封面就地下——一张图十几秒，用户点一首歌等得起
    if let Some(candidate) = outcome.accepted
        && let Some(path) =
            crate::scrape::download_cover(&state.covers_dir, &song_id, &outcome.artist, &candidate)
    {
        crate::scrape::record_cover(conn, &song_id, &path)?;
    }
    Ok(jp_scraper::ScrapeStore::new(conn).summary()?)
}

/// 批量刮削，后台跑。进度走 `scrape://progress` 事件。
///
/// `only_missing` 为真（默认）时只刮还没成功过的，重跑幂等。
#[tauri::command]
pub fn scrape_start<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    only_missing: Option<bool>,
    limit: Option<i64>,
) -> CmdResult<usize> {
    let song_ids: Vec<String> = {
        let corpus = state.corpus();
        let conn = corpus.connection();
        jp_scraper::ScrapeStore::new(conn).ensure_schema()?;
        // 只刮还没成功、也没被用户跳过的
        let sql = if only_missing.unwrap_or(true) {
            "SELECT s.id FROM songs s \
             LEFT JOIN scrape_state st ON st.song_id = s.id \
             WHERE st.status IS NULL OR st.status NOT IN ('success','skipped') \
             ORDER BY s.id LIMIT ?1"
        } else {
            "SELECT s.id FROM songs s ORDER BY s.id LIMIT ?1"
        };
        let mut stmt = conn.prepare(sql)?;
        stmt.query_map(rusqlite::params![limit.unwrap_or(1000)], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()?
    };

    let total = song_ids.len();
    crate::scrape::spawn_batch(
        app,
        state.scrape_job(),
        state.db_path.clone(),
        state.covers_dir.clone(),
        song_ids,
    )?;
    Ok(total)
}

#[tauri::command]
pub fn scrape_cancel(state: State<'_, AppState>) -> CmdResult<()> {
    state.scrape_job().request_cancel();
    Ok(())
}

#[tauri::command]
pub fn scrape_is_running(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(state.scrape_job().is_running())
}

/// 用户在复核界面选了一条候选。
///
/// 这是**人工决策**，所以状态直接置为 success 并留痕，
/// 重跑刮削不会把它冲掉。
#[tauri::command]
pub fn scrape_accept(
    state: State<'_, AppState>,
    file_path: String,
    candidate_index: usize,
) -> CmdResult<crate::scrape::ManualApply> {
    let corpus = state.corpus();
    let conn = corpus.connection();
    let store = jp_scraper::ScrapeStore::new(conn);
    let record = store
        .get(&file_path)?
        .ok_or_else(|| anyhow::anyhow!("没有 {file_path} 的刮削记录"))?;
    let candidates: Vec<jp_scraper::ScrapeCandidate> = record
        .candidates
        .clone()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let candidate = candidates
        .get(candidate_index)
        .ok_or_else(|| anyhow::anyhow!("没有第 {candidate_index} 条候选"))?
        .clone();

    // 本地歌手名决定封面落到哪个目录，和自动流程保持一致
    let local_artist = store
        .original_metadata(&file_path)?
        .map(|t| t.artist().to_string())
        .unwrap_or_default();

    let resolved = jp_scraper::ResolvedTrack {
        file_path: file_path.clone(),
        status: jp_scraper::ScrapeStatus::Success,
        confidence: candidate.score(),
        candidate: Some(candidate.clone()),
        candidates,
        error_message: "用户在复核界面确认".into(),
        matched_at: jp_scraper::models::utc_now(),
        ..Default::default()
    };
    store.record(&resolved, &record.song_id)?;

    // 光改状态是不够的：之前 songs 一个字没写、封面也没下，
    // 点「采用」等于什么都没发生。
    if record.song_id.is_empty() {
        return Err(anyhow::anyhow!("{file_path} 还没和 songs 关联，无法写入").into());
    }
    Ok(crate::scrape::apply_manual(
        conn,
        &state.covers_dir,
        &record.song_id,
        &local_artist,
        &candidate,
    )?)
}

/// 用户跳过。重跑时不再自动处理它。
#[tauri::command]
pub fn scrape_skip(
    state: State<'_, AppState>,
    file_path: String,
    reason: Option<String>,
) -> CmdResult<()> {
    let corpus = state.corpus();
    let store = jp_scraper::ScrapeStore::new(corpus.connection());
    store.mark_skipped(&file_path, &reason.unwrap_or_else(|| "用户跳过".into()))?;
    Ok(())
}

/// 把失败的打回待刮，供「重试全部失败」用。
#[tauri::command]
pub fn scrape_retry_failed(state: State<'_, AppState>) -> CmdResult<usize> {
    let corpus = state.corpus();
    let conn = corpus.connection();
    let store = jp_scraper::ScrapeStore::new(conn);
    store.ensure_schema()?;
    // low_confidence 不在内：它缺的是用户决策，不是再跑一次网络请求
    let failed: Vec<String> = store
        .by_status(
            &[
                jp_scraper::ScrapeStatus::Failed,
                jp_scraper::ScrapeStatus::Running,
            ],
            None,
        )?
        .into_iter()
        .map(|r| r.file_path)
        .collect();
    Ok(store.reset(Some(&failed))?)
}

// ────────────────────────── 歌手 / 专辑封面 ──────────────────────────

/// `artists` 表的一行 + 照片在不在。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistRow {
    pub name: String,
    pub image_path: String,
    pub artist_type: String,
    pub country: String,
    pub formed: String,
    pub updated_at: String,
    /// 库里有这个歌手的曲目数
    pub track_count: i64,
    /// 照片文件是不是真的在盘上。库里记了路径但文件被删了也算没有。
    pub has_image: bool,
}

/// 库里的歌手，连同已经刮到的资料。
#[tauri::command]
pub fn artist_roster(state: State<'_, AppState>) -> CmdResult<Vec<ArtistRow>> {
    use rusqlite::OptionalExtension;
    let corpus = state.corpus();
    let conn = corpus.connection();
    let names = crate::scrape::library_artists(conn, false)?;

    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let row = conn
            .query_row(
                "SELECT COALESCE(image_path,''), COALESCE(artist_type,''),
                        COALESCE(country,''), COALESCE(formed,''), COALESCE(updated_at,'')
                 FROM artists WHERE name=?1",
                rusqlite::params![name],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?
            .unwrap_or_default();
        let track_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM songs WHERE artist = ?1 OR artist LIKE ?1 || '/%'
                 OR artist LIKE '%/' || ?1 OR artist LIKE '%/' || ?1 || '/%'",
            rusqlite::params![name],
            |r| r.get(0),
        )?;
        let has_image = !row.0.is_empty() && std::path::Path::new(&row.0).is_file();
        out.push(ArtistRow {
            name,
            image_path: row.0,
            artist_type: row.1,
            country: row.2,
            formed: row.3,
            updated_at: row.4,
            track_count,
            has_image,
        });
    }
    Ok(out)
}

/// 刮一个歌手。同步，两三秒（MusicBrainz 限每秒一次）。
#[tauri::command]
pub fn scrape_artist(
    state: State<'_, AppState>,
    name: String,
) -> CmdResult<crate::scrape::ArtistOutcome> {
    let corpus = state.corpus();
    Ok(crate::scrape::scrape_artist_one(
        corpus.connection(),
        &state.artists_dir,
        &name,
    )?)
}

/// 批量刮歌手，后台跑。和曲目刮削共用一个作业标志——
/// 两边都要排 MusicBrainz 的队，同时跑只会互相拖慢。
#[tauri::command]
pub fn scrape_artists_start<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    only_missing: Option<bool>,
) -> CmdResult<usize> {
    let names = {
        let corpus = state.corpus();
        crate::scrape::library_artists(corpus.connection(), only_missing.unwrap_or(true))?
    };
    let total = names.len();
    crate::scrape::spawn_artist_batch(
        app,
        state.scrape_job(),
        state.db_path.clone(),
        state.artists_dir.clone(),
        names,
    )?;
    Ok(total)
}

/// 用曲目封面填 `albums.artwork_path`。不额外发网络请求。
#[tauri::command]
pub fn fill_album_artwork(state: State<'_, AppState>) -> CmdResult<usize> {
    let corpus = state.corpus();
    Ok(crate::scrape::fill_album_artwork(corpus.connection())?)
}

/// 给识别成功但没有封面的歌补封面。**不重新搜索**，用刮削时存下的候选，
/// 所以快（一首一两个请求），也不会动已经写好的 metadata。
#[tauri::command]
pub async fn scrape_backfill_covers<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> CmdResult<crate::scrape::CoverBackfill> {
    blocking(app, move |state| {
        let covers = state.covers_dir.clone();
        let corpus = state.corpus();
        let report = crate::scrape::backfill_covers(corpus.connection(), &covers, false)?;
        // 曲目封面补上了，专辑封面也跟着能填
        crate::scrape::fill_album_artwork(corpus.connection())?;
        Ok(report)
    })
    .await
}

// ────────────────────────────── Anki ──────────────────────────────

/// Anki 那边现在什么情况。连不上不是错误——界面该显示指引。
#[tauri::command]
pub fn anki_status(_state: State<'_, AppState>) -> CmdResult<crate::anki::AnkiStatus> {
    Ok(crate::anki::status())
}

/// 候选词表，按语料里的频次排，并标出 Anki 里已有 / 已学过的。
#[tauri::command]
pub fn anki_words(
    state: State<'_, AppState>,
    pos: Option<Vec<String>>,
    min_count: Option<i64>,
    song_ids: Option<Vec<String>>,
    limit: Option<i64>,
    skip_studied: Option<bool>,
) -> CmdResult<Vec<jp_anki::WordCandidate>> {
    let defaults = jp_anki::PickOptions::default();
    let options = jp_anki::PickOptions {
        pos: pos.unwrap_or(defaults.pos),
        min_count: min_count.unwrap_or(defaults.min_count),
        song_ids: song_ids.unwrap_or_default(),
        limit: limit.unwrap_or(defaults.limit),
    };
    // 学习状态是直接读文件，不需要 Anki 开着；读不到就当没有
    let learning = jp_anki::learning::load(None, "").ok();
    let corpus = state.corpus();
    let mut words = jp_anki::pick_words(corpus.connection(), &options, learning.as_ref())?;
    if skip_studied.unwrap_or(false) {
        words.retain(|w| !w.studied);
    }
    Ok(words)
}

/// 组一张卡看看。**不碰 Anki**，纯预览。
#[tauri::command]
pub fn anki_preview(
    state: State<'_, AppState>,
    lemma: String,
    pos: Option<String>,
    max_examples: Option<usize>,
    max_dicts: Option<usize>,
) -> CmdResult<jp_anki::Card> {
    let corpus = state.corpus();
    Ok(jp_anki::card::build(
        corpus.connection(),
        &lemma,
        &pos.unwrap_or_default(),
        &[],
        jp_anki::CardOptions {
            max_examples: max_examples.unwrap_or(2).clamp(1, 8),
            max_dicts,
        },
    )?)
}

/// 把「JPOP Corpus」笔记类型建好或更新到最新。返回是不是新建的。
#[tauri::command]
pub fn anki_ensure_note_type(_state: State<'_, AppState>) -> CmdResult<bool> {
    let anki = crate::anki::client();
    jp_anki::export::prepare(&anki).map_err(|e| anyhow::anyhow!("{}", e.advice()).into())
}

/// 批量导出，后台跑。进度走 `anki://progress` 事件。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn anki_export_start<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    words: Vec<crate::anki::ExportItem>,
    deck: String,
    max_examples: Option<usize>,
    max_dicts: Option<usize>,
    dup_mode: Option<jp_anki::DupMode>,
    dup_scope: Option<jp_anki::DupScope>,
    clip_audio: Option<bool>,
) -> CmdResult<usize> {
    if words.is_empty() {
        return Err(anyhow::anyhow!("没有选中任何词").into());
    }
    if deck.trim().is_empty() {
        return Err(anyhow::anyhow!("要先选一个牌组").into());
    }
    let total = words.len();
    let mut options = crate::anki::options(deck, max_examples.unwrap_or(2), max_dicts);
    options.dup_mode = dup_mode.unwrap_or_default();
    options.dup_scope = dup_scope.unwrap_or_default();
    // 有 ffmpeg 就默认切音频，和 Python 的默认一致
    if clip_audio.unwrap_or(true) {
        options.audio = crate::anki::audio_options(&state.db_path);
    }
    crate::anki::spawn_export(app, state.anki_job(), state.db_path.clone(), words, options)?;
    Ok(total)
}

#[tauri::command]
pub fn anki_cancel(state: State<'_, AppState>) -> CmdResult<()> {
    state.anki_job().request_cancel();
    Ok(())
}

#[tauri::command]
pub fn anki_is_running(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(state.anki_job().is_running())
}

/// 有没有 ffmpeg。没有时界面把「音频片段」禁掉，和 Python 一样。
#[tauri::command]
pub fn anki_audio_available(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(crate::anki::audio_options(&state.db_path).is_some())
}

/// 更新选中的词：只重查读音、释义、JLPT、音高、词频、词性，不动例句和音频。后台跑。
#[tauri::command]
pub fn anki_update_start<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    words: Vec<crate::anki::ExportItem>,
    deck: String,
    dup_scope: Option<jp_anki::DupScope>,
) -> CmdResult<usize> {
    if words.is_empty() {
        return Err(anyhow::anyhow!("没有选中任何词").into());
    }
    if deck.trim().is_empty() {
        return Err(anyhow::anyhow!("要先选一个牌组").into());
    }
    let total = words.len();
    crate::anki::spawn_update(
        app,
        state.anki_job(),
        state.db_path.clone(),
        words,
        deck,
        dup_scope.unwrap_or_default(),
    )?;
    Ok(total)
}

/// 刷新之前报个数：范围内有几张 JPOP Corpus 旧卡。只读，不改任何东西。
///
/// 一千多张卡要分十几批问 Anki，放到后台线程里，免得界面卡住。
#[tauri::command]
pub async fn anki_refresh_preview(
    deck: String,
    scope: Option<jp_anki::RefreshScope>,
) -> CmdResult<usize> {
    let scope = scope.unwrap_or_default();
    let count =
        tauri::async_runtime::spawn_blocking(move || crate::anki::refresh_preview(&deck, scope))
            .await
            .map_err(|err| anyhow::anyhow!("统计旧卡时出错：{err}"))??;
    Ok(count)
}

/// 刷新旧牌组：范围内每张 JPOP Corpus 卡重查词典字段，不动例句和音频。后台跑。
#[tauri::command]
pub fn anki_refresh_start<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    deck: String,
    scope: Option<jp_anki::RefreshScope>,
) -> CmdResult<()> {
    if deck.trim().is_empty() {
        return Err(anyhow::anyhow!("要先选一个牌组").into());
    }
    crate::anki::spawn_refresh(
        app,
        state.anki_job(),
        state.db_path.clone(),
        deck,
        scope.unwrap_or_default(),
    )?;
    Ok(())
}

// ────────────────────────────── 分词校正 ──────────────────────────────

/// 打开编辑器时取一行的分词。行不存在返回 null。
#[tauri::command]
pub fn token_correction(
    state: State<'_, AppState>,
    utterance_id: i64,
) -> CmdResult<Option<jp_corpus::CorrectionView>> {
    Ok(state.corpus().token_correction(utterance_id)?)
}

/// 保存校正：改写 tokens 并记下校正，下次检索立即生效。
#[tauri::command]
pub fn save_token_correction(
    state: State<'_, AppState>,
    utterance_id: i64,
    tokens: Vec<jp_corpus::TokenEdit>,
) -> CmdResult<()> {
    Ok(state
        .corpus()
        .save_token_correction(utterance_id, &tokens)?)
}

/// 撤销校正，回到第一次校正前的分词。返回是否真的撤销了。
#[tauri::command]
pub fn revert_token_correction(state: State<'_, AppState>, utterance_id: i64) -> CmdResult<bool> {
    Ok(state.corpus().revert_token_correction(utterance_id)?)
}

// ---------------------------------------------------------------- 词典（Yomitan 格式）

/// 已导入的词典，按界面顺序。还没导入过时返回空表（不新建库文件）。
#[tauri::command]
pub fn dict_list(state: State<'_, AppState>) -> CmdResult<Vec<jp_dict::store::DictionaryInfo>> {
    Ok(state
        .with_dictionaries(false, |store| store.dictionaries())?
        .unwrap_or_default())
}

/// 旧版（PyQt）登记过的词典包，给「从旧版迁移」用。只读 corpus.db。
#[tauri::command]
pub fn dict_legacy_sources(
    state: State<'_, AppState>,
) -> CmdResult<Vec<crate::dict::LegacySource>> {
    let titles: Vec<String> = state
        .with_dictionaries(false, |store| {
            Ok(store.dictionaries()?.into_iter().map(|d| d.title).collect())
        })?
        .unwrap_or_default();
    Ok(crate::dict::legacy_sources(&state.db_path, &titles)?)
}

/// 后台导入一批词典包（.zip 或解压开的目录）。进度走 `dict://progress`，结束发 `dict://done`。
#[tauri::command]
pub fn dict_import_start<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> CmdResult<usize> {
    if paths.is_empty() {
        return Err(anyhow::anyhow!("没有选择词典文件").into());
    }
    let total = paths.len();
    crate::dict::spawn_import(
        app,
        state.dict_job(),
        state.dictionaries_path().to_path_buf(),
        paths,
    )?;
    Ok(total)
}

#[tauri::command]
pub fn dict_import_cancel(state: State<'_, AppState>) -> CmdResult<()> {
    state.dict_job().request_cancel();
    Ok(())
}

#[tauri::command]
pub fn dict_is_importing(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(state.dict_job().is_running())
}

/// 导入是一个几十秒的大事务，这期间的写操作会等锁等到超时。与其让用户等十秒再看报错，不如直接说。
fn ensure_not_importing(state: &AppState, what: &str) -> CmdResult<()> {
    if state.dict_job().is_running() {
        return Err(anyhow::anyhow!("正在导入词典，等导入完成再{what}").into());
    }
    Ok(())
}

#[tauri::command]
pub fn dict_set_enabled(state: State<'_, AppState>, id: i64, enabled: bool) -> CmdResult<()> {
    ensure_not_importing(&state, "启用或停用")?;
    state.with_dictionaries(false, |store| store.set_enabled(id, enabled))?;
    Ok(())
}

/// `ids` 是界面上从上到下的新顺序。
#[tauri::command]
pub fn dict_set_order(state: State<'_, AppState>, ids: Vec<i64>) -> CmdResult<()> {
    ensure_not_importing(&state, "调整顺序")?;
    state.with_dictionaries(false, |store| store.set_order(&ids))?;
    Ok(())
}

#[tauri::command]
pub fn dict_delete(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    ensure_not_importing(&state, "删除")?;
    state.with_dictionaries(false, |store| store.delete(id))?;
    Ok(())
}

/// 从 `text` 开头查词（最多看 16 个字、返回 32 条）。放进阻塞线程池，不占 UI 线程。
#[tauri::command]
pub async fn dict_lookup<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    text: String,
    mode: Option<String>,
) -> CmdResult<jp_dict::translator::FindTermsResult> {
    let result = tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        crate::dict::lookup(&app.state::<AppState>(), &text, mode.as_deref())
    })
    .await
    .map_err(|err| anyhow::anyhow!("查词线程出错：{err}"))??;
    Ok(result)
}

/// 一键制卡（Lapis）。切音频、传文件要一两秒，放进阻塞线程池。
#[tauri::command]
pub async fn anki_mine<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    request: crate::mine::MineRequest,
) -> CmdResult<jp_anki::mine::MineOutcome> {
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        crate::mine::mine(&app.state::<AppState>(), &request)
    })
    .await
    .map_err(|err| anyhow::anyhow!("制卡线程出错：{err}"))??;
    Ok(outcome)
}

/// 这几个词在 Anki 里有没有卡了（查词结果上的制卡按钮用）。
#[tauri::command]
pub async fn anki_mine_check(
    model: String,
    expressions: Vec<String>,
) -> CmdResult<crate::mine::MineCheck> {
    let check = tauri::async_runtime::spawn_blocking(move || {
        crate::mine::check(&crate::anki::client(), &model, &expressions)
    })
    .await
    .map_err(|err| anyhow::anyhow!("查询线程出错：{err}"))?;
    Ok(check)
}

/// 在 Anki 的浏览器里打开这些笔记。
#[tauri::command]
pub async fn anki_browse_notes(note_ids: Vec<i64>) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let ids: Vec<String> = note_ids.iter().map(i64::to_string).collect();
        crate::anki::client()
            .call(
                "guiBrowse",
                serde_json::json!({ "query": format!("nid:{}", ids.join(",")) }),
            )
            .map(|_| ())
            .map_err(|err| anyhow::anyhow!(crate::anki::describe(&err)))
    })
    .await
    .map_err(|err| anyhow::anyhow!("线程出错：{err}"))??;
    Ok(())
}

/// Anki 里的笔记类型（制卡设置里选笔记类型用）。连不上时返回空表。
#[tauri::command]
pub async fn anki_model_names() -> CmdResult<Vec<String>> {
    let names = tauri::async_runtime::spawn_blocking(|| {
        crate::anki::client().model_names().unwrap_or_default()
    })
    .await
    .map_err(|err| anyhow::anyhow!("线程出错：{err}"))?;
    Ok(names)
}

/// 词典里的图片，原样返回字节（前端按扩展名建 Blob，不走 JSON 数组）。
#[tauri::command]
pub fn dict_media(
    state: State<'_, AppState>,
    dictionary: String,
    path: String,
) -> CmdResult<tauri::ipc::Response> {
    let bytes = state
        .with_dictionaries(false, |store| {
            let Some(id) = store
                .dictionaries()?
                .into_iter()
                .find(|d| d.title == dictionary)
                .map(|d| d.id)
            else {
                return Ok(None);
            };
            Ok(store.media(id, &path)?.map(|m| m.data))
        })?
        .flatten()
        .ok_or_else(|| anyhow::anyhow!("词典「{dictionary}」里没有图片 {path}"))?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryStyles {
    pub title: String,
    pub styles: String,
}

/// 启用的词典自带的 styles.css。界面按词典名限定作用域后再注入，免得互相污染。
#[tauri::command]
pub fn dict_styles(state: State<'_, AppState>) -> CmdResult<Vec<DictionaryStyles>> {
    Ok(state
        .with_dictionaries(false, |store| {
            let mut out = Vec::new();
            for d in store
                .dictionaries()?
                .into_iter()
                .filter(|d| d.enabled && d.has_styles)
            {
                out.push(DictionaryStyles {
                    styles: store.styles(d.id)?,
                    title: d.title,
                });
            }
            Ok(out)
        })?
        .unwrap_or_default())
}
