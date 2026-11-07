// 由 rust/crates/jp-api 生成（ts-rs），不要手改。
// 改了 Rust 类型之后跑 `JP_API_BLESS=1 cargo test -p jp-api` 重新生成，连同这个文件一起提交。
// 不重新生成的话 `cargo test --workspace` 会失败。

export type AddArgs = { folders: Array<FolderArg>, };

/**
 * 最近添加的一首。
 */
export type AddedTrack = { songId: string, title: string, artist: string, coverPath: string, addedAt: string, };

export type AppliedView = { refreshed: number, restored: number, relinked: number, movedOut: number, 
/**
 * 对账到确认之间情况变了、没有写的几项
 */
skipped: number, };

/**
 * 确认对账：界面显示的那几组、去掉用户勾掉的。每一项都会重新核对，对不上的跳过。
 */
export type ApplyArgs = { id: number, 
/**
 * 要刷新的文件路径（`changed` 里的 `path`）
 */
refresh?: Array<string>, 
/**
 * 要恢复的歌（`returned`）
 */
restore?: Array<string>, 
/**
 * 要改路径的歌（`moved`）
 */
relink?: Array<RelinkArg>, 
/**
 * 要移出的歌（`gone`）
 */
moveOut?: Array<string>, };

/**
 * 上次听的那首，以及停在哪。
 */
export type ContinueTrack = { songId: string, title: string, artist: string, coverPath: string, durationSec: number | null, 
/**
 * 最后一次播放停下的位置（秒）
 */
positionSec: number, 
/**
 * 最后一次是不是听完了。听完了界面给「再听一遍」，没听完给「从 x:xx 继续」
 */
completed: boolean, lastPlayed: string, playCount: number, 
/**
 * 停下的位置正在唱的那一行（时间戳不晚于停下位置的最后一行）。
 * 没有带时间戳的歌词、或者停在第一行之前时是空串
 */
stoppedLine: string, };

/**
 * 导航栏底部状态点上的数：库里有几首要人处理。只数在库的歌（已移出的不算）
 */
export type CorpusStatus = { 
/**
 * 有歌词但没分词：点词查不了、没有振假名
 */
untokenized: number, 
/**
 * 记的路径上找不到音频文件
 */
missingAudio: number, };

/**
 * 某一天（本地日期）听了多少。
 */
export type DayListening = { listenedSec: number, 
/**
 * 听了几首（去重）
 */
songs: number, 
/**
 * 听到的歌词行数。每次播放算 `[停下位置 - 听了多久, 停下位置]` 这段时间里唱到的行；
 * 同一首听两遍算两遍——这是「听了多少」，不是「听过多少种」
 */
linesHeard: number, 
/**
 * 连续有收听的天数。今天还没听的话从昨天往回数（今天还没过完，不算断）
 */
streakDays: number, };

export type FolderArg = { path: string, 
/**
 * 包括子文件夹。不传当作是
 */
recursive?: boolean, };

/**
 * 首页要的一切。
 *
 * 打成一个命令而不是让前端发六次：首页一进来就要全部数据，
 * 分开发不仅多五次往返，还可能拿到互相不一致的快照
 * （比如统计已经更新、最近播放还是旧的）。
 */
export type HomeSummary = { overview: Overview, 
/**
 * 上次听的那首和停在哪；没有收听记录时是 null
 */
continueTrack: ContinueTrack | null, recentlyPlayed: Array<PlayedTrack>, favorites: Array<Track>, today: DayListening, reach: Reach, recentlyAdded: Array<AddedTrack>, };

export type HomeSummaryArgs = { 
/**
 * 本地相对 UTC 的分钟数（东八区 +480），「今天」按它算。不传按 UTC
 */
utcOffsetMinutes?: number, 
/**
 * 「最近添加」要几首
 */
addedLimit?: number, };

/**
 * 挪了位置的歌。
 */
export type MovedSong = { songId: string, title: string, artist: string, from: string, to: string, 
/**
 * 凭什么认定是同一首（ISRC、录音 ID、曲名歌手时长……）
 */
reason: string, 
/**
 * 原来已经移出了，改路径的同时恢复
 */
wasRemoved: boolean, };

/**
 * 没有参数的命令用它。TS 里是 `Record<symbol, never>`，传 `{}` 或不传都行
 */
export type NoArgs = Record<symbol, never>;

/**
 * Corpus Overview 卡片。
 */
export type Overview = { tracks: number, albums: number, people: number, performers: number, composers: number, lyricists: number, arrangers: number, lyricLines: number, tokens: number, vocabulary: number, totalDurationSec: number, };

/**
 * 最近播放 / 播放排行的一行。
 */
export type PlayedTrack = { songId: string, title: string, artist: string, album: string, coverPath: string, playCount: number, lastPlayed: string, totalListenedSec: number, durationSec: number | null, 
/**
 * 最后一次播放停在哪（秒）
 */
lastPositionSec: number, 
/**
 * 最后一次是不是听完了
 */
lastCompleted: boolean, };

/**
 * 全局搜索的一条结果。
 *
 * 用带标签的枚举而不是「一个宽表 + kind 字段」，是因为前端要按类型
 * 分组显示、点击行为也各不相同；宽表会让每种类型都带一堆用不上的空字段。
 */
export type QuickHit = { "kind": "track", songId: string, title: string, artist: string, album: string, durationSec: number | null, } | { "kind": "album", albumId: number, title: string, albumArtist: string, trackCount: number, } | { "kind": "person", personId: number, name: string, 
/**
 * 这个人担任过的角色，按作品数排序
 */
roles: Array<string>, trackCount: number, } | { "kind": "lyric", songId: string, title: string, artist: string, utteranceId: number, timeSec: number | null, text: string, } | { "kind": "word", lemma: string, pos: string, freq: number, songCount: number, };

export type QuickSearchArgs = { query: string, 
/**
 * 每种结果最多几条；不传用 jp-corpus 的默认
 */
perKind?: number, };

/**
 * 全局搜索结果，按类型分组。
 *
 * 分组而不是混在一起排序：跨类型的相关性分数没有可比性
 * （曲名匹配的 0.9 和歌词 bm25 的 -8.3 不是一个量纲），
 * 硬排出来的顺序是假的。分组让用户自己按类型找。
 */
export type QuickSearchResults = { tracks: Array<QuickHit>, albums: Array<QuickHit>, people: Array<QuickHit>, words: Array<QuickHit>, lyrics: Array<QuickHit>, };

/**
 * 听过的歌覆盖了语料里多少实词。
 */
export type Reach = { 
/**
 * 听满 [`HEARD_MIN_SEC`] 秒的歌有几首
 */
heardSongs: number, 
/**
 * 这些歌里出现过的实词词元数（去重）
 */
heardLemmas: number, 
/**
 * 全语料的实词词元数（去重）
 */
totalLemmas: number, };

export type RelinkArg = { songId: string, to: string, };

/**
 * 已移出的一首。
 */
export type RemovedSong = { id: string, title: string, artist: string, audioPath: string, removedAt: string, 
/**
 * 它原来属于哪个文件夹（那个文件夹也移除了的话是空）
 */
source: string, 
/**
 * 音频文件现在还在不在
 */
audioPresent: boolean, };

export type SetEnabledArgs = { id: number, enabled: boolean, };

/**
 * 一首歌和它的文件。
 */
export type SongFile = { songId: string, title: string, artist: string, path: string, };

export type SongIdsArgs = { ids: Array<string>, };

/**
 * 一个音乐文件夹。
 */
export type Source = { id: number, rootPath: string, label: string, recursive: boolean, enabled: boolean, createdAt: string, lastScannedAt: string | null, lastScanError: string | null, };

export type SourceIdArgs = { id: number, };

/**
 * 一个文件夹的对账。
 */
export type SourcePlanView = { source: Source, 
/**
 * 库里没有的文件（新歌、疑似重复、导入时会跳过的都在这里）。
 * 不在这里导入：界面拿这些路径去走导入复核（`scan_files`）
 */
newFiles: Array<string>, 
/**
 * 其中确定是新歌的几首
 */
newSongs: number, 
/**
 * 文件变了（换了压制、改了标签）：刷新文件信息
 */
changed: Array<SongFile>, 
/**
 * 移出过、文件又回来了
 */
returned: Array<SongFile>, moved: Array<MovedSong>, 
/**
 * 文件不见了
 */
gone: Array<SongFile>, 
/**
 * 音频没变、旁边的歌词文件新放进来或改过（`path` 是歌词文件）。
 * 确认后对这些歌（和 `changed` 里的歌）调歌词同步，挂成新的一版
 */
lyricsChanged: Array<SongFile>, unchanged: number, 
/**
 * 文件夹里一个音频都没扫到（可能是盘换了内容）
 */
nothingFound: boolean, };

/**
 * 界面上的一行：来源本身，加上它下面有几首歌。
 */
export type SourceView = { 
/**
 * 在库的歌
 */
songs: number, 
/**
 * 已移出（`deleted_at` 不空）的歌
 */
removed: number, 
/**
 * 文件夹现在在不在（盘没插、挪走了是 false）
 */
present: boolean, id: number, rootPath: string, label: string, recursive: boolean, enabled: boolean, createdAt: string, lastScannedAt: string | null, lastScanError: string | null, };

/**
 * 一首曲目。对应 `songs` 表。
 */
export type Track = { id: string, title: string, 
/**
 * 原始歌手串。合作曲是「A/B」这种形态——结构化的版本在 `credits` 里。
 */
artist: string, album: string, albumId: number | null, year: string, genre: string, audioPath: string, coverPath: string, durationSec: number | null, 
/**
 * 歌词行数。0 表示还没有歌词，UI 要据此降级而不是当成错误。
 */
lineCount: number, };

export type TrackArgs = { songId: string, };

/**
 * Analytics 页的 Timeline 一格。
 */
export type YearStats = { year: string, trackCount: number, albumCount: number, vocabulary: number, };

/** Tauri 命令名 → 参数和返回值。`api.ts` 的 `command()` 按它做类型检查 */
export interface Commands {
  overview: { args: NoArgs; output: Overview };
  timeline: { args: NoArgs; output: Array<YearStats> };
  corpus_status: { args: NoArgs; output: CorpusStatus };
  get_track: { args: TrackArgs; output: Track | null };
  quick_search: { args: QuickSearchArgs; output: QuickSearchResults };
  home_summary: { args: HomeSummaryArgs; output: HomeSummary };
  library_sources: { args: NoArgs; output: Array<SourceView> };
  library_sources_add: { args: AddArgs; output: Array<SourceView> };
  library_source_set_enabled: { args: SetEnabledArgs; output: Array<SourceView> };
  library_source_plan: { args: SourceIdArgs; output: SourcePlanView };
  library_source_apply: { args: ApplyArgs; output: AppliedView };
  library_removed_songs: { args: NoArgs; output: Array<RemovedSong> };
  library_restore_songs: { args: SongIdsArgs; output: number };
}
