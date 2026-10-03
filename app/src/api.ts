/**
 * Tauri command 的类型化封装。
 *
 * 这些类型和 `rust/crates/jp-corpus/src/models.rs` 一一对应——那边用了
 * `#[serde(rename_all = "camelCase")]`，所以这里直接是 camelCase。
 *
 * 手写而不是自动生成：类型总共二十来个，生成器（ts-rs / specta）会在构建
 * 链路里多一环，而且它生成的注释不如手写的说得清楚。等类型多到手写会漂移
 * 的时候再上工具。
 */

import { invoke } from "@tauri-apps/api/core";

// ────────────────────────────── 曲库 ──────────────────────────────

export interface Track {
  id: string;
  title: string;
  /** 原始歌手串。合作曲是「A/B」这种形态——结构化版本在 credits 里。 */
  artist: string;
  album: string;
  albumId: number | null;
  year: string;
  genre: string;
  audioPath: string;
  coverPath: string;
  durationSec: number | null;
  /** 0 表示还没有歌词。UI 要据此降级，而不是当成错误。 */
  lineCount: number;
}

export interface Album {
  id: number;
  title: string;
  albumArtist: string;
  year: string;
  artworkPath: string;
  trackCount: number;
  totalDurationSec: number;
}

export interface PersonSummary {
  id: number;
  name: string;
  role: string;
  trackCount: number;
  /** 歌手照片（`artists.image_path`）。只有刮削过的演唱者有，其他人是空串。 */
  imagePath: string;
}

export interface Credit {
  personId: number;
  name: string;
  role: string;
  position: number;
  source: string;
}

export interface Collaborator {
  personId: number;
  name: string;
  sharedTracks: number;
  roles: string[];
  /** 同 `PersonSummary.imagePath`。点合作者会切到他，头像得跟过去。 */
  imagePath: string;
}

export interface LineToken {
  surface: string;
  lemma: string;
  /** UPOS。空串表示分词时没定出词性。 */
  pos: string;
}

export interface LyricLine {
  utteranceId: number;
  lineIdx: number;
  /** null 表示这份 LRC 没有时间轴——不能假设所有歌词都能跟随播放。 */
  timeSec: number | null;
  text: string;
  /** 空数组表示这行还没分词，要降级成纯文本。 */
  tokens: LineToken[];
}

// ────────────────────────────── 检索 ──────────────────────────────

export type MatchField = "surface" | "lemma";

export interface KwicQuery {
  keywords: string[];
  field: MatchField;
  pos?: string | null;
  personIds?: number[];
  crossLine?: boolean;
  dedup?: boolean;
  /** 只留关键词里有假名或汉字的命中（Python「日本語のみ」），先筛再截断 */
  jpOnly?: boolean;
  limit?: number | null;
}

export interface KwicHit {
  songId: string;
  artist: string;
  title: string;
  audioPath: string;
  utteranceId: number;
  lineIdx: number;
  timeSec: number | null;
  text: string;
  left: string;
  keyword: string;
  right: string;
  repeatCount: number;
  /** 这一行做过分词校正。KWIC 里标 ✏。 */
  corrected: boolean;
}

/** 分词校正里的一个词。键名和 Python 存进 token_corrections 的 JSON 一致。 */
export interface TokenEdit {
  surface: string;
  lemma: string;
  pos: string;
}

/** 分词校正编辑器打开一行时拿到的东西。 */
export interface CorrectionView {
  utteranceId: number;
  songId: string;
  artist: string;
  title: string;
  text: string;
  /** 当前生效的分词 */
  tokens: TokenEdit[];
  /** 第一次校正之前的分词，撤销时回到这里 */
  original: TokenEdit[];
  corrected: boolean;
}

/** 编辑器里词性下拉的选项。和 Python 分词校正对话框的 _POS_OPTIONS 一致。 */
export const UPOS_TAGS = [
  "NOUN", "VERB", "ADJ", "ADV", "ADP", "AUX", "CCONJ", "SCONJ",
  "DET", "INTJ", "NUM", "PART", "PRON", "PROPN", "PUNCT", "SYM", "X",
] as const;

export interface LyricHit {
  songId: string;
  artist: string;
  title: string;
  utteranceId: number;
  timeSec: number | null;
  text: string;
  /** bm25 分，越小越相关。短查询走 LIKE 兜底时恒为 0（无相关性排序）。 */
  rank: number;
}

// ────────────────────────────── 全局搜索 ──────────────────────────────

/**
 * 一条全局搜索结果。带标签的联合类型——每种结果的字段和点击行为都不同，
 * 用一个宽表会让每种都带一堆空字段。
 */
export type QuickHit =
  | { kind: "track"; songId: string; title: string; artist: string; album: string; durationSec: number | null }
  | { kind: "album"; albumId: number; title: string; albumArtist: string; trackCount: number }
  | { kind: "person"; personId: number; name: string; roles: string[]; trackCount: number }
  | { kind: "lyric"; songId: string; title: string; artist: string; utteranceId: number; timeSec: number | null; text: string }
  | { kind: "word"; lemma: string; pos: string; freq: number; songCount: number };

/**
 * 按类型分组的搜索结果。
 *
 * 分组而不是混排：跨类型的相关性分数没有可比性（曲名的 LIKE 匹配和歌词的
 * bm25 不是一个量纲），硬凑出的全局排序是假的。
 */
export interface QuickSearchResults {
  tracks: QuickHit[];
  albums: QuickHit[];
  people: QuickHit[];
  words: QuickHit[];
  lyrics: QuickHit[];
}

// ────────────────────────────── 语料 ──────────────────────────────

export interface Overview {
  tracks: number;
  albums: number;
  people: number;
  performers: number;
  composers: number;
  lyricists: number;
  arrangers: number;
  lyricLines: number;
  tokens: number;
  vocabulary: number;
  totalDurationSec: number;
}

export interface YearStats {
  year: string;
  trackCount: number;
  albumCount: number;
  vocabulary: number;
}

export interface WordFrequency {
  lemma: string;
  pos: string;
  freq: number;
  songCount: number;
  surfaces: string[];
}

/** 统计页、报告共用的筛选。演唱者用 people.id，空数组是全部 */
export interface StatsFilter {
  performerIds: number[];
  jpOnly: boolean;
}

/** 统计表一行：词元 × 词性，带 JLPT（本地 jlpt_cache，没有是空串） */
export interface FrequencyRow extends WordFrequency {
  jlpt: string;
}

export interface PosShare {
  pos: string;
  tokenCount: number;
  tokenPct: number;
  typeCount: number;
}

export interface CoverageRow {
  n: number;
  pct: number;
}

/** 语料统计报告。字段和 Python `CorpusReportWorker` 一一对应 */
export interface CorpusReport {
  songCount: number;
  utteranceCount: number;
  tokenCount: number;
  typeCount: number;
  ttr: number;
  /** 词次不到 1000 时没有 */
  sttr: number | null;
  sttrChunk: number | null;
  hapaxCount: number;
  hapaxRatio: number;
  avgTypesPerSong: number;
  avgTokensPerLine: number;
  posDist: PosShare[];
  coverage: CoverageRow[];
  /** [词元, 次数]，前 20 */
  topWords: [string, number][];
}

export interface StatsReport {
  report: CorpusReport;
  /** 筛选的演唱者名字 */
  performers: string[];
  jpOnly: boolean;
  /** 导出 TXT 的内容，后端和报告一起生成（和 Python 版逐字节对过） */
  text: string;
}

export interface PosCount {
  pos: string;
  count: number;
}

export interface WordExample {
  songId: string;
  artist: string;
  title: string;
  utteranceId: number;
  timeSec: number | null;
  text: string;
}

export interface WordInCorpus {
  lemma: string;
  occurrences: number;
  songCount: number;
  artistCount: number;
  firstYear: string | null;
  lastYear: string | null;
  posDistribution: PosCount[];
  examples: WordExample[];
}

// ────────────────────────────── 播放历史 ──────────────────────────────

export interface PlayEvent {
  songId: string;
  listenedSec: number;
  positionSec: number;
  completed: boolean;
  source: string;
}

export interface PlayedTrack {
  songId: string;
  title: string;
  artist: string;
  album: string;
  coverPath: string;
  playCount: number;
  lastPlayed: string;
  totalListenedSec: number;
}

/** 现在用的是哪个语料库目录，以及是怎么找到的 */
export interface LibraryRootInfo {
  path: string;
  /** env：环境变量；settings：设置里选的；nextToExe / workingDir：自动找到的；default：默认数据目录 */
  source: "env" | "settings" | "nextToExe" | "workingDir" | "default";
  /** 设置里记着的目录，没设过是空串 */
  remembered: string;
  ready: boolean;
}

export interface HealthReport {
  dbPath: string;
  tracks: number;
  lyricLines: number;
  /** false 时「实时分词」降级，其余功能正常。 */
  tokenizerReady: boolean;
  /** 没有可用输出设备时为 false。UI 据此禁用播放，而不是给一个点了没反应的按钮。 */
  audioReady: boolean;
  /** 变调尚未实现。为 false 时不要显示变调控件。 */
  pitchSupported: boolean;
}

// ────────────────────────────── 播放 ──────────────────────────────

export type PlayState = "empty" | "playing" | "paused" | "ended";

export interface LoopRegion {
  startSec: number;
  endSec: number;
}

export interface PlaybackState {
  songId: string;
  playState: PlayState;
  positionSec: number;
  /** 解码器给不出时长的格式会是 null，进度条要能降级。 */
  durationSec: number | null;
  rate: number;
  volume: number;
  loopRegion: LoopRegion | null;
  /** 变调半音数，−6…+6，0 是原调。全局生效，之后打开的歌也按这个调 */
  pitchSemitones: number;
  /** 正在渲染这个调的音频（没缓存过的调要先渲染几秒） */
  pitchRendering: boolean;
  /** 最近一次变调失败（已退回原调）。id 变了才提示 */
  pitchError: { id: number; message: string } | null;
}

/** 按流派 / 年代分组的统计。 */
export interface FacetCount {
  /** 年代是「2010s」，流派是原始字符串；空值归到「未分类」「年代不详」 */
  label: string;
  trackCount: number;
  totalDurationSec: number;
}

/** 首页要的一切。一次拿全，避免六次往返和不一致的快照。 */
export interface HomeSummary {
  overview: Overview;
  recentlyPlayed: PlayedTrack[];
  mostPlayed: PlayedTrack[];
  favorites: Track[];
  albums: Album[];
  genres: FacetCount[];
  decades: FacetCount[];
  topWords: WordFrequency[];
}

/** 时长回填的结果。每一种失败单独计数——笼统的「成功 N 个」用户无从修。 */
export interface DurationBackfillReport {
  scanned: number;
  written: number;
  /** 路径失效，文件不在了 */
  missing: number;
  /** 能解码但解码器给不出时长 */
  unknown: number;
  /** 打不开或解不了 */
  failed: number;
  /** 前几条失败详情 */
  samples: string[];
}

/** 倍速档位。和 Rust 侧 state.rs 的钳制范围一致。 */
export const RATE_STEPS = [0.5, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 2.0] as const;

/** 变调档位，和 Python 版一样 −6…+6 半音 */
export const PITCH_STEPS = [-6, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6] as const;

export function pitchLabel(semitones: number): string {
  return semitones === 0 ? "原调" : `${semitones > 0 ? "+" : ""}${semitones} 半音`;
}

// ────────────────────────────── 调用 ──────────────────────────────

/** Rust 侧的 CommandError 形状。 */
interface CommandError {
  message: string;
}

function isCommandError(value: unknown): value is CommandError {
  return typeof value === "object" && value !== null && "message" in value;
}

/**
 * 统一把 Rust 的错误对象转成 Error。
 *
 * Tauri 的 invoke 在 command 返回 Err 时 reject 的是**序列化后的对象**，
 * 不是 Error 实例——直接扔给 React 会得到 "[object Object]"。
 */
/**
 * 播放控制命令完成时发的信号：播放状态的轮询听到就立刻补拉一次（见 `usePlaybackState`）。
 *
 * 闲着（暂停、停止）的时候轮询降到 1Hz；没有这一下，点了「播放」要等最多一秒
 * 进度条才动。用 window 事件而不是直接调 hook：api 不该反过来依赖 hook。
 */
export const PLAYBACK_KICK = "jpop:playback-kick";

function kicked<T>(pending: Promise<T>): Promise<T> {
  return pending.finally(() => window.dispatchEvent(new Event(PLAYBACK_KICK)));
}

export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    if (isCommandError(err)) throw new Error(err.message);
    throw err instanceof Error ? err : new Error(String(err));
  }
}

export const api = {
  health: () => call<HealthReport>("health"),
  /** 语料库目录的现状（设置页显示用） */
  libraryRoot: () => call<LibraryRootInfo>("library_root"),
  /** 换一个语料库目录；传 null 清掉。返回一句给用户看的提示，**要重启才生效** */
  setLibraryRoot: (path: string | null) => call<string>("set_library_root", { path }),

  // 曲库
  listTracks: (limit?: number) => call<Track[]>("list_tracks", { limit }),
  getTrack: (songId: string) => call<Track | null>("get_track", { songId }),
  listAlbums: (artistKey?: string, limit?: number) =>
    call<Album[]>("list_albums", { artistKey, limit }),
  albumTracks: (albumId: number) => call<Track[]>("album_tracks", { albumId }),
  trackCredits: (songId: string) => call<Credit[]>("track_credits", { songId }),
  tokenCorrection: (utteranceId: number) =>
    call<CorrectionView | null>("token_correction", { utteranceId }),
  saveTokenCorrection: (utteranceId: number, tokens: TokenEdit[]) =>
    call<null>("save_token_correction", { utteranceId, tokens }),
  revertTokenCorrection: (utteranceId: number) =>
    call<boolean>("revert_token_correction", { utteranceId }),
  peopleByRole: (role: string, limit?: number) =>
    call<PersonSummary[]>("people_by_role", { role, limit }),
  /** 按 id 取人。命令面板定位到具体人物时用。 */
  personById: (personId: number) =>
    call<PersonSummary | null>("person_by_id", { personId }),
  worksByPerson: (personId: number) => call<Track[]>("works_by_person", { personId }),
  collaborators: (personId: number, limit?: number) =>
    call<Collaborator[]>("collaborators", { personId, limit }),
  lyrics: (songId: string) => call<LyricLine[]>("lyrics", { songId }),

  // 检索
  kwic: (query: KwicQuery) => call<KwicHit[]>("kwic", { query }),
  searchLyrics: (text: string, limit?: number) =>
    call<LyricHit[]>("search_lyrics", { text, limit }),
  /** Cmd+K 全局搜索。跨曲目/专辑/人物/词汇/歌词。 */
  quickSearch: (query: string, perKind?: number) =>
    call<QuickSearchResults>("quick_search", { query, perKind }),

  // 语料
  overview: () => call<Overview>("overview"),
  timeline: () => call<YearStats[]>("timeline"),
  wordFrequency: (pos?: string, limit?: number) =>
    call<WordFrequency[]>("word_frequency", { pos, limit }),
  wordInCorpus: (lemma: string, currentSongId?: string, limit?: number) =>
    call<WordInCorpus>("word_in_corpus", { lemma, currentSongId, limit }),

  // 统计
  /** 统计表：词元 × 词性的频次，带 JLPT。默认前 300 */
  statsFrequency: (filter: StatsFilter, pos?: string, limit?: number) =>
    call<FrequencyRow[]>("stats_frequency", { filter, pos: pos || null, limit }),
  /** 语料统计报告，连同导出用的 TXT */
  statsReport: (filter: StatsFilter) => call<StatsReport>("stats_report", { filter }),
  /** 把导出内容写到保存对话框选的路径。只接受 .txt / .csv / .html */
  exportText: (path: string, content: string) => call<void>("export_text", { path, content }),

  // 播放
  /** 位置和播不播一次传：设了变调时要先渲染，渲染好才真正加载 */
  audioLoad: (songId: string, opts: { positionSec?: number | null; autoplay?: boolean } = {}) =>
    kicked(call<void>("audio_load", { songId, positionSec: opts.positionSec ?? null, autoplay: opts.autoplay ?? true })),
  audioSetPitch: (semitones: number) => kicked(call<void>("audio_set_pitch", { semitones })),
  audioPlay: () => kicked(call<void>("audio_play")),
  audioPause: () => kicked(call<void>("audio_pause")),
  audioToggle: () => kicked(call<void>("audio_toggle")),
  audioStop: () => kicked(call<void>("audio_stop")),
  audioSeek: (positionSec: number) => kicked(call<void>("audio_seek", { positionSec })),
  audioSetRate: (rate: number) => kicked(call<void>("audio_set_rate", { rate })),
  audioSetVolume: (volume: number) => kicked(call<void>("audio_set_volume", { volume })),
  /** 返回 false 表示区间太短被拒绝。两个参数都省略 = 取消循环。 */
  audioSetLoop: (startSec?: number, endSec?: number) =>
    kicked(call<boolean>("audio_set_loop", { startSec, endSec })),
  /** 无副作用的状态读取。 */
  audioState: () => call<PlaybackState>("audio_state"),
  /**
   * 状态读取 **+ 走一拍收听统计**。轮询要用这个——
   * 换歌/停止时它会把上一段写进 play_history，首页的「最近播放」靠它。
   */
  audioTick: () => call<PlaybackState>("audio_tick"),
  audioSpectrum: () => call<number[]>("audio_spectrum"),

  // 首页 / 收藏
  homeSummary: () => call<HomeSummary>("home_summary"),
  /** 切换收藏，返回切换后的状态。 */
  toggleFavorite: (entityType: string, entityId: string) =>
    call<boolean>("toggle_favorite", { entityType, entityId }),
  isFavorite: (entityType: string, entityId: string) =>
    call<boolean>("is_favorite", { entityType, entityId }),

  // 维护
  /** 幂等：只填 duration_sec 为空的行。 */
  backfillDurations: () => call<DurationBackfillReport>("backfill_durations"),

  // 播放历史
  recordPlay: (event: PlayEvent) => call<void>("record_play", { event }),
  recentlyPlayed: (limit?: number) => call<PlayedTrack[]>("recently_played", { limit }),
  mostPlayed: (minListenedSec?: number, limit?: number) =>
    call<PlayedTrack[]>("most_played", { minListenedSec, limit }),

  // 导入：扫描 -> 复核 -> 确认
  /** 扫描目录并和现有曲库比对。**只读，什么都不写。** */
  scanFolder: (path: string, maxDepth?: number) =>
    call<ScanResult>("scan_folder", { path, maxDepth }),
  /** 扫描选中的若干**文件**（也能夹着目录）。同样只读。 */
  scanFiles: (paths: string[], maxDepth?: number) =>
    call<ScanResult>("scan_files", { paths, maxDepth }),
  /** 执行上一次扫描的结果。写库就在这一步。 */
  runImport: (tokenize?: boolean) => call<ImportReport>("run_import", { tokenize }),

  // 歌词：补齐 / 自己导入
  /** 库里还没有歌词的歌。 */
  lyricsMissing: () => call<MissingLyrics[]>("lyrics_missing"),
  /** 批量补齐，后台跑。进度走 lyrics://progress 事件。 */
  lyricsFillStart: (songIds?: string[]) => call<void>("lyrics_fill_start", { songIds }),
  lyricsFillCancel: () => call<void>("lyrics_fill_cancel"),
  lyricsFillRunning: () => call<boolean>("lyrics_fill_running"),
  /** 补一首：先看音频旁边有没有 .lrc，没有再上网搜。挑不出来返回 null。 */
  lyricsFillOne: (songId: string) => call<AttachedLyrics | null>("lyrics_fill_one", { songId }),
  /** 用自己的一份歌词文件覆盖这首歌的歌词。 */
  lyricsImportFile: (songId: string, path: string) =>
    call<AttachedLyrics>("lyrics_import_file", { songId, path }),

  // 数据迁移
  /** 预览：把那个目录的数据搬过来会发生什么。只读。 */
  migratePlan: (path: string) => call<MigratePlan>("migrate_plan", { path }),
  /** 真的搬。进度走 migrate://progress 事件。 */
  migrateRun: (path: string, options: { dictionaries: boolean; history: boolean }) =>
    call<MigrateOutcome>("migrate_run", { path, options }),

  // 分词词典
  /** 当前语料库目录里有没有 Sudachi 词典。 */
  tokenizerStatus: () => call<TokenizerStatus>("tokenizer_status", {}),
  /** 把别处那一份 Sudachi 词典复制进当前语料库目录，并立刻装上（207MB，要等几秒）。 */
  tokenizerInstall: (path: string) => call<InstalledDict>("tokenizer_install", { path }),
  /** 去网上下一份词典（43MB，解开 207MB）。进度走 tokenizer://progress 事件。 */
  tokenizerDownload: () => call<InstalledDict>("tokenizer_download", {}),
  // 诊断
  /** 攒一段诊断文本：版本、库、能力、最近日志。不联网。 */
  diagnosticsReport: () => call<string>("diagnostics_report", {}),
  /** 把那段文本存到 path。 */
  diagnosticsSave: (path: string) => call<string>("diagnostics_save", { path }),

  /** 有歌词但没分词的歌。这些歌点词查不了、也没有振假名。 */
  tokenizeMissingList: () => call<UntokenizedSong[]>("tokenize_missing_list", {}),
  /** 给那些歌补上分词。进度走 tokenize://progress 事件。 */
  tokenizeMissingRun: () => call<Tokenized>("tokenize_missing_run", {}),

  // 检查更新
  /** 问一次 GitHub Releases：有没有比正在跑的这一版更新的。 */
  updateCheck: () => call<UpdateStatus>("update_check"),
  /** 最近几次发布，给「查看更新日志」用。 */
  updateChangelog: (limit?: number) => call<ReleaseInfo[]>("update_changelog", { limit }),
  /**
   * 下载安装包。进度走 update://progress 事件，返回落盘路径。
   * 只传版本号：下哪个文件、期望的哈希都由后端按它自己那次检查决定，且必须比当前版本新。
   */
  updateDownload: (version: string) => call<string>("update_download", { version }),
  /**
   * 拉起安装程序并退出应用（NSIS 要替换正在运行的 exe）。
   * 不收路径：后端只启动它自己这一轮下好、校验过的那个文件。
   */
  updateInstall: () => call<void>("update_install", {}),
  /** 丢弃上一次扫描的结果。 */
  cancelImport: () => call<void>("cancel_import"),

  // 刮削：识别 / 补 metadata / 封面
  scrapeSummary: () => call<ScrapeSummaryCounts>("scrape_summary"),
  /** 复核队列。默认取「需确认」的。 */
  scrapeReviewQueue: (statuses?: ScrapeStatusName[], limit?: number) =>
    call<ReviewItem[]>("scrape_review_queue", { statuses, limit }),
  /** 某个文件的尝试历史。「为什么反复失败」用它回答。 */
  scrapeAttempts: (filePath: string, limit?: number) =>
    call<ScrapeAttempt[]>("scrape_attempts", { filePath, limit }),
  /** 刮一首，同步，一两秒。 */
  scrapeTrack: (songId: string) => call<ScrapeSummaryCounts>("scrape_track", { songId }),
  /** 批量刮削，后台跑。返回这次要刮几首，进度走 scrape://progress 事件。 */
  scrapeStart: (onlyMissing?: boolean, limit?: number) =>
    call<number>("scrape_start", { onlyMissing, limit }),
  scrapeCancel: () => call<void>("scrape_cancel"),
  scrapeIsRunning: () => call<boolean>("scrape_is_running"),
  /** 用户在复核界面选定一条候选。真的会写 songs、信用和封面。 */
  scrapeAccept: (filePath: string, candidateIndex: number) =>
    call<ManualApply>("scrape_accept", { filePath, candidateIndex }),
  scrapeSkip: (filePath: string, reason?: string) =>
    call<void>("scrape_skip", { filePath, reason }),
  /** 把失败的打回待刮。needs-review 的不动——那缺的是决策不是重试。 */
  scrapeRetryFailed: () => call<number>("scrape_retry_failed"),

  // 歌手照片 / 专辑封面
  /** 库里的歌手，连同已经刮到的资料。合作曲的斜杠已拆开。 */
  artistRoster: () => call<ArtistRow[]>("artist_roster"),
  /** 刮一个歌手：MusicBrainz 拿资料和别名，Deezer 拿照片。 */
  scrapeArtist: (name: string) => call<ArtistOutcome>("scrape_artist", { name }),
  /** 批量刮歌手，后台跑。和曲目刮削共用一个作业标志。 */
  scrapeArtistsStart: (onlyMissing?: boolean) =>
    call<number>("scrape_artists_start", { onlyMissing }),
  /** 用曲目封面填 albums.artwork_path。不额外发网络请求。 */
  fillAlbumArtwork: () => call<number>("fill_album_artwork"),
  /** 给识别成功但没有封面的歌补封面：用刮削时存下的候选重取，不重新搜索 */
  scrapeBackfillCovers: () => call<CoverBackfill>("scrape_backfill_covers"),

  // Anki
  /** Anki 那边什么情况。连不上不是错误，message 里是处置建议。 */
  ankiStatus: () => call<AnkiStatus>("anki_status"),
  /** 候选词，按语料频次排，标出已有 / 已学过的。 */
  ankiWords: (opts?: {
    pos?: string[];
    minCount?: number;
    songIds?: string[];
    limit?: number;
    skipStudied?: boolean;
  }) => call<WordCandidate[]>("anki_words", opts ?? {}),
  /** 组一张卡看看。**不碰 Anki**，纯预览。 */
  ankiPreview: (lemma: string, pos?: string, maxExamples?: number, maxDicts?: number) =>
    call<AnkiCard>("anki_preview", { lemma, pos, maxExamples, maxDicts }),
  /** 建/更新「JPOP Corpus」笔记类型。返回是不是新建的。 */
  ankiEnsureNoteType: () => call<boolean>("anki_ensure_note_type"),
  /** 批量导出，后台跑。进度走 anki://progress 事件。 */
  ankiExportStart: (
    words: { lemma: string; pos: string }[],
    deck: string,
    maxExamples?: number,
    maxDicts?: number,
    opts?: { dupMode?: AnkiDupMode; dupScope?: AnkiDupScope; clipAudio?: boolean },
  ) =>
    call<number>("anki_export_start", {
      words,
      deck,
      maxExamples,
      maxDicts,
      dupMode: opts?.dupMode,
      dupScope: opts?.dupScope,
      clipAudio: opts?.clipAudio,
    }),
  /** 更新选中的词：只重查词典字段，不动例句和音频。后台跑，进度同样走 anki://progress。 */
  ankiUpdateStart: (words: { lemma: string; pos: string }[], deck: string, dupScope?: AnkiDupScope) =>
    call<number>("anki_update_start", { words, deck, dupScope }),
  /** 范围内有几张 JPOP Corpus 旧卡。只读。 */
  ankiRefreshPreview: (deck: string, scope: AnkiRefreshScope) =>
    call<number>("anki_refresh_preview", { deck, scope }),
  /** 刷新旧牌组。后台跑。 */
  ankiRefreshStart: (deck: string, scope: AnkiRefreshScope) =>
    call<void>("anki_refresh_start", { deck, scope }),
  /** 本机有没有 ffmpeg（能不能切音频片段）。 */
  ankiAudioAvailable: () => call<boolean>("anki_audio_available"),
  ankiCancel: () => call<void>("anki_cancel"),
  ankiIsRunning: () => call<boolean>("anki_is_running"),
  /** 挖词报告：读 collection、写 output/corpus_report.html 并用浏览器打开。now 是本地时间 YYYY-MM-DD HH:MM */
  ankiMiningReport: (now: string, deckContains?: string) =>
    call<MiningReportResult>("anki_mining_report", { now, deckContains }),
};

/** UPOS → 中文标签。只覆盖 UI 里会显示的那些。 */
export const POS_LABELS: Record<string, string> = {
  NOUN: "名词",
  PROPN: "专名",
  VERB: "动词",
  ADJ: "形容词",
  ADV: "副词",
  AUX: "助动词",
  PRON: "代词",
  DET: "连体词",
  NUM: "数词",
  ADP: "助词",
  SCONJ: "接续助词",
  CCONJ: "接续词",
  PART: "终助词",
  INTJ: "感叹词",
};

/** 实词。只有这些在歌词里可点击——助词点开也没有研究价值。 */
export const CONTENT_POS = new Set(["NOUN", "PROPN", "VERB", "ADJ", "ADV"]);

export const ROLE_LABELS: Record<string, string> = {
  lyricist: "作词",
  composer: "作曲",
  arranger: "编曲",
  translator: "translator",
  performer: "演唱",
};

/** 长时段显示：14.7 小时这种。总时长用它，单曲用 formatDuration。 */
export function formatLongDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "—";
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.round((seconds % 3600) / 60);
  if (hours === 0) return `${minutes} 分`;
  return `${hours} 小时 ${minutes} 分`;
}

export function formatDuration(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds) || seconds <= 0) return "—";
  const total = Math.round(seconds);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

// ────────────────────────────── 导入 ──────────────────────────────

/**
 * 对一个扫描到的文件打算做什么。
 *
 * 和 `rust/crates/jp-import/src/plan.rs` 的 `Action` 对应，
 * 那边用了 `#[serde(tag = "kind")]`，所以这里是可辨识联合。
 */
export type ImportAction =
  | { kind: "new"; songId: string }
  | { kind: "alreadyImported"; songId: string }
  | { kind: "possibleDuplicate"; songId: string; existingPath: string }
  | { kind: "duplicateInBatch"; firstPath: string }
  | { kind: "skipped"; reason: string };

export interface PlanItem {
  path: string;
  fileName: string;
  title: string;
  artist: string;
  album: string;
  durationSec: number | null;
  hasLyrics: boolean;
  /** 读 tag 时出的问题。null 表示一切正常。 */
  warning: string | null;
  action: ImportAction;
}

export interface PlanSummary {
  total: number;
  new: number;
  alreadyImported: number;
  possibleDuplicates: number;
  duplicatesInBatch: number;
  skipped: number;
}

export interface ScanResult {
  summary: PlanSummary;
  items: PlanItem[];
  /** 分词器不可用时导入照常，只是不写 tokens。按下导入前要说清楚。 */
  tokenizerReady: boolean;
  /** 选中了但不是音频、因此没进计划的文件名（.lrc、封面图这些）。 */
  ignored: string[];
}

/** 还没有歌词的一首歌（`lyrics_missing`）。 */
export interface MissingLyrics {
  songId: string;
  title: string;
  artist: string;
  durationSec: number | null;
  audioPath: string;
  /** 音频旁边就有一份 .lrc——这种不用上网 */
  siblingLrc: string | null;
}

/** 歌词是哪来的。 */
export type LyricsSource = "sibling" | "netease" | "manual";

/** 挂上歌词之后的结果。 */
export interface AttachedLyrics {
  songId: string;
  source: LyricsSource;
  /** 在线搜到的那条叫什么，本地来源是文件名 */
  matched: string;
  lyricLines: number;
  tokens: number;
  credits: number;
  correctionsRestored: number;
  lrcPath: string;
}

/** 批量补齐歌词的进度事件（`lyrics://progress`）。 */
export interface LyricsProgress {
  done: number;
  total: number;
  songId: string;
  title: string;
  /** filled / notFound / failed */
  status: string;
  source: string;
  matched: string;
  lyricLines: number;
  /** **这一首**的失败原因，成功时是空串 */
  message: string;
  /**
   * 作业**整体**断在半路的原因，正常结束时是空串。
   *
   * 和 `message` 分开是因为原来它俩是同一个字段：整批黄了和某一首没补上
   * 在界面看来一模一样，而这两件事该说的话完全不同。
   * 四个后台作业（刮削 / Anki / 词典 / 补齐歌词）统一用 `error` 这个名字。
   */
  error: string;
  filled: number;
  notFound: number;
  failed: number;
  finished: boolean;
  cancelled: boolean;
}

/** 迁移预览（`migrate_plan`，只读）。 */
export interface MigratePlan {
  sourceSongs: number;
  newSongs: number;
  duplicateSongs: number;
  lyricLines: number;
  tokens: number;
  people: number;
  albums: number;
  credits: number;
  plays: number;
  favorites: number;
  corrections: number;
  dictTerms: number;
  jlptRows: number;
  /** 当前库已经有词典表了——有的话不覆盖 */
  targetHasDictionaries: boolean;
  samples: string[];
}

/** 迁移结果（`migrate_run`）。库里的数字和文件的数字合在一起。 */
export interface MigrateOutcome {
  songs: number;
  lyricLines: number;
  tokens: number;
  people: number;
  albums: number;
  credits: number;
  plays: number;
  favorites: number;
  corrections: number;
  dictTerms: number;
  jlptRows: number;
  skipped: number;
  idMap: [string, string][];
  coversCopied: number;
  lyricsCopied: number;
  artistPhotosCopied: number;
  dictionariesCopied: boolean;
  dictionariesBytes: number;
  /** 分词词典（振假名靠它）搬过来了没有 */
  sudachiCopied: boolean;
  sudachiBytes: number;
  /** 搬完之后顺手补的分词（当前库里有歌词、没分词的那些歌） */
  tokenized: Tokenized;
  warnings: string[];
}

/** 有歌词但没分词的歌（`tokenize_missing_list`）。 */
export interface UntokenizedSong {
  songId: string;
  artist: string;
  title: string;
  lyricLines: number;
}

/** 补分词的结果（`tokenize_missing_run`）。 */
export interface Tokenized {
  songs: number;
  lines: number;
  tokens: number;
  correctionsRestored: number;
}

/** 分词词典（Sudachi）在当前语料库目录里的状态（`tokenizer_status`）。 */
export interface TokenizerStatus {
  ready: boolean;
  dictPath: string | null;
  dictBytes: number;
  /** 哪儿来的：随程序自带 / 语料库目录里的 / 0.1.x 的 venv / 都没有 */
  source: "app" | "library" | "venv" | "none";
  /** 导入进来的那一份会放在哪 */
  bundledDir: string;
}

/** 复制 / 下载的结果（`tokenizer_install`、`tokenizer_download`）。 */
export interface InstalledDict {
  bytes: number;
  dir: string;
  /** 词典装上之后顺手补的分词（有歌词、没分词的那些歌） */
  tokenized: Tokenized;
  /** 顺手补分词失败的原因；空串是没失败（包括没有要补的） */
  tokenizeError: string;
}

/** 下词典的进度（`tokenizer://progress`）。 */
export interface DictProgress {
  received: number;
  total: number;
  /** downloading / extracting / done */
  stage: string;
}

/** 词典压缩包多大——界面上要先告诉用户这要下多少 */
export const DICT_DOWNLOAD_BYTES = 45_128_544;

/** 后端说「没有词典」时错误里一定有这一句，界面据此换成「下载」那条横幅 */
export const NO_DICTIONARY = "找不到 Sudachi 词典";

/** 一次发布里能装的那个文件（`update_check` 给的，只用来显示；下载时只回传版本号）。 */
export interface UpdateAsset {
  name: string;
  size: number;
  url: string;
  /** GitHub 给的 sha256，装之前要对上 */
  sha256: string | null;
}

/** 一次发布。`notes` 是 Markdown 原文。 */
export interface ReleaseInfo {
  version: string;
  tag: string;
  name: string;
  notes: string;
  publishedAt: string;
  url: string;
  prerelease: boolean;
  /** 能直接装的安装包；只发了 msi 时是 null */
  installer: UpdateAsset | null;
}

export interface UpdateStatus {
  current: string;
  latest: ReleaseInfo | null;
  updateAvailable: boolean;
}

/** 下载进度事件（`update://progress`）。 */
export interface UpdateProgress {
  received: number;
  total: number;
  /** downloading / done */
  stage: string;
  message: string;
}

export const LYRICS_SOURCE_LABELS: Record<LyricsSource, string> = {
  sibling: "音频旁边的 .lrc",
  netease: "在线搜到",
  manual: "手动导入",
};

export type ImportOutcome =
  | {
      kind: "imported";
      lyricLines: number;
      tokens: number;
      credits: number;
      /** 删歌再导入时套回去的分词校正行数 */
      correctionsRestored: number;
    }
  | { kind: "failed"; error: string };

export interface ImportedTrack {
  songId: string;
  path: string;
  title: string;
  outcome: ImportOutcome;
}

export interface ImportReport {
  tracks: ImportedTrack[];
  /**
   * 用户中途点了停止。**`tracks` 里的那些是真导进去了**——一首歌一个事务，
   * 停在哪儿哪儿之前就是完成的。所以结果页要说「停在第 N 首」，
   * 既不能说「全部完成」，也不能说「什么都没做」。
   */
  cancelled: boolean;
  /** metadata/songs.csv 同步：updated / noFile / unchanged，其余是出错原因（库已经导进去了） */
  csv?: string;
}

/** `import://progress`。导入可能要好几分钟，界面得知道走到哪儿了。 */
export interface ImportProgress {
  done: number;
  total: number;
  /** 刚导完的那首 */
  title: string;
  finished: boolean;
  cancelled: boolean;
}

export const ACTION_LABELS: Record<ImportAction["kind"], string> = {
  new: "新增",
  alreadyImported: "已在库中",
  possibleDuplicate: "疑似重复",
  duplicateInBatch: "本批重复",
  skipped: "跳过",
};

// ────────────────────────────── 刮削 ──────────────────────────────

/** 刮削状态。取值和 `rust/crates/jp-scraper/src/status.rs` 一致，直接落库。 */
export type ScrapeStatusName =
  | "pending"
  | "running"
  | "success"
  | "low_confidence"
  | "failed"
  | "skipped";

export interface ScrapeSummaryCounts {
  pending: number;
  running: number;
  success: number;
  lowConfidence: number;
  failed: number;
  skipped: number;
  total: number;
}

/** 一次扣分及其理由。分数只是一个数字的话，没法回答「为什么匹配错了」。 */
export interface Penalty {
  reason: string;
  delta: number;
}

export interface MatchBreakdown {
  title_score: number;
  artist_score: number;
  album_score: number;
  duration_score: number;
  track_number_score: number;
  provider_score: number;
  /** 只包含**实际参与计算**的分项 */
  weights: Record<string, number>;
  penalties: Penalty[];
  final_score: number;
}

export interface ScrapeCandidate {
  provider: string;
  providerId: string;
  title: string;
  artist: string;
  album: string;
  albumArtist: string;
  year: string;
  genre: string;
  durationSec: number | null;
  trackNumber: number | null;
  discNumber: number | null;
  artworkUrl: string;
  thumbUrl: string;
  providerConfidence: number;
  breakdown: MatchBreakdown | null;
}

export interface ReviewItem {
  filePath: string;
  songId: string;
  status: ScrapeStatusName;
  confidence: number;
  provider: string;
  errorType: string;
  errorMessage: string;
  retryCount: number;
  lastAttemptAt: string;
  /** 本地文件现在写的是什么，用来和候选对照 */
  localTitle: string;
  localArtist: string;
  localAlbum: string;
  candidates: ScrapeCandidate[];
  /** 最佳候选的打分解释，一行 */
  explain: string;
}

export interface ScrapeAttempt {
  id: number;
  filePath: string;
  provider: string;
  status: ScrapeStatusName;
  confidence: number;
  errorType: string;
  errorMessage: string;
  attemptedAt: string;
}

/** 批量刮削的进度事件（`scrape://progress`）。 */
export interface ScrapeProgress {
  /** "track" 或 "artist"。两种作业共用一个事件通道。 */
  kind: string;
  done: number;
  total: number;
  songId: string;
  title: string;
  status: string;
  confidence: number;
  matched: string;
  coverSaved: boolean;
  finished: boolean;
  cancelled: boolean;
  /**
   * 整批**断在半路**的原因（库打不开、解析器建不起来……），正常结束时是空串。
   *
   * 以前没有这个字段：出错时后端只往日志里写一行，发过来的终态仍然是
   * `finished: true, cancelled: false`——界面显示「完成」，而其实一首都没刮完。
   */
  error: string;
}

export const SCRAPE_STATUS_LABELS: Record<ScrapeStatusName, string> = {
  pending: "待刮",
  running: "进行中",
  success: "已匹配",
  low_confidence: "需确认",
  failed: "失败",
  skipped: "已跳过",
};

/** 失败原因 → 该怎么办。分类的意义就在这里。 */
export const ERROR_HINTS: Record<string, string> = {
  timeout: "请求超时，稍后重试",
  rate_limit: "被限流，等一会儿再重试",
  network: "网络不通",
  unavailable: "对方服务暂时不可用",
  invalid_response: "返回结构不对，可能是接口变了",
  not_found: "对方明确说没有这条记录",
  no_results: "搜索没有结果，改写法或换源才有用，重试没用",
  no_match: "有候选但都不够像，需要人工挑",
  download_failed: "元数据对上了，封面没下下来",
  unknown: "未分类的错误",
};

// ────────────────────────── 歌手 / 专辑封面 ──────────────────────────

/** 补封面的结果 */
export interface CoverBackfill {
  /** 检查了几首（库里没有封面的） */
  checked: number;
  filled: number;
  /** 刮削记录里连候选都没有，得重刮 */
  noCandidate: number;
  failed: number;
  samples: string[];
}

export interface ArtistRow {
  name: string;
  imagePath: string;
  /** Group / Person */
  artistType: string;
  country: string;
  formed: string;
  updatedAt: string;
  /** 库里有这个歌手的曲目数 */
  trackCount: number;
  /** 照片文件是不是真的在盘上。库里记了路径但文件被删了也算没有。 */
  hasImage: boolean;
}

export interface ArtistOutcome {
  name: string;
  /** MusicBrainz 认出来的规范名，没认出来时是空串 */
  canonicalName: string;
  artistType: string;
  country: string;
  formed: string;
  imagePath: string;
  notFound: boolean;
}

/** 人工确认之后实际写了什么。 */
export interface ManualApply {
  /** 补进 songs 的字段数（只补空的，不覆盖已有值） */
  fieldsFilled: number;
  /** 标成 source='manual' 的信用条数——自动流程从此不许动它们 */
  creditsProtected: number;
  /** 新加的信用条数（候选里的演唱者本来没有信用记录） */
  creditsAdded: number;
  coverPath: string | null;
}

// ────────────────────────────── Anki ──────────────────────────────

/** 挖词报告生成的结果 */
export interface MiningReportResult {
  path: string;
  collectionPath: string;
  cards: number;
  studied: number;
  words: number;
  artists: number;
  songs: number;
  decks: number;
  /** 有卡的笔记类型 */
  noteTypes: string[];
  /** 浏览器没打开时的原因；文件已经写好了 */
  openError: string;
}

export interface AnkiStatus {
  /** Anki 开着而且装了 AnkiConnect */
  connected: boolean;
  /** 连不上时的处置建议 */
  message: string;
  version: number;
  decks: string[];
  /** 「JPOP Corpus」笔记类型在不在 */
  noteTypeReady: boolean;
  /** 读到的 collection 路径，空表示没找到 */
  collectionPath: string;
  /** 已经进过 Anki 的词数 */
  knownWords: number;
  /** 其中复习过的 */
  studiedWords: number;
}

export interface WordCandidate {
  lemma: string;
  /** UPOS */
  pos: string;
  /** 在语料里出现几次 */
  count: number;
  /** 出现在几首歌里 */
  songCount: number;
  inAnki: boolean;
  /** 复习过。加进去没看过不算。 */
  studied: boolean;
}

export interface CardExample {
  songId: string;
  artist: string;
  title: string;
  timeSec: number | null;
  text: string;
  surface: string;
  audioPath: string;
  endSec: number | null;
  /** 歌词行 id。音频文件名 jpop_{id}.mp3 用它 */
  utteranceId: number;
}

export interface AnkiCard {
  expression: string;
  reading: string;
  meaning: string;
  sentence: string;
  sentenceAudio: string;
  source: string;
  jlpt: string;
  pitch: string;
  freq: string;
  partOfSpeech: string;
  /** 实际查到释义用的形；和词不同时释义顶部有「辞書形：…」 */
  lookupTerm: string;
  examples: CardExample[];
  /** 语料里没有这个词的例句 */
  noExamples: boolean;
  noDefinitions: boolean;
}

/** 导出进度事件（`anki://progress`）。 */
export interface AnkiProgress {
  /** export / update / refresh */
  job: string;
  done: number;
  total: number;
  lemma: string;
  /** added / updated / alreadyComplete / skipped / noExamples / refreshed / notFound / failed */
  outcome: string;
  newSentences: number;
  message: string;
  finished: boolean;
  cancelled: boolean;
  /** 整批中断的原因（比如 Anki 中途关了）。空表示正常结束。四处统一叫 `error`。 */
  error: string;
}

export const ANKI_OUTCOME_LABELS: Record<string, string> = {
  added: "新增",
  updated: "追加例句",
  alreadyComplete: "已是最新",
  skipped: "已有，跳过",
  noExamples: "无例句",
  refreshed: "已刷新",
  notFound: "Anki 里没有",
  failed: "失败",
};

/** 已有这个词时：追加新例句 / 跳过 */
export type AnkiDupMode = "append" | "skip";
/** 查重范围：当前牌组 / 主牌组及子牌组 */
export type AnkiDupScope = "deck" | "root";
/** 刷新旧牌组的范围 */
export type AnkiRefreshScope = "deck" | "deckAndChildren" | "rootAndChildren";
