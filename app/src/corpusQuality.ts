/**
 * 语料质量与来源追踪、子语料、可复现分析。后端见
 * `jp-corpus/src/{quality,lineage,analysis}.rs`，设计说明见 `docs/provenance.md`。
 *
 * 类型和 Rust 那边一一对应（camelCase）。纯函数（问题代码的说法、筛选条件的描述）放这里，
 * 有测试钉着；组件只管画。
 */

import {
  call,
  type CollocationQuery,
  type CorpusSnapshot,
  type FrequencyQuery,
  type KeynessQuery,
  type StatsFilter,
  type Subcorpus,
} from "./api";
import { tr } from "./i18n";

// ───────────────────────────── 校验 ─────────────────────────────

export type Severity = "info" | "warning" | "error";

export interface Issue {
  code: string;
  severity: Severity;
  /** 第几行（从 0 起）；整份歌词的问题没有 */
  lineIdx?: number;
  message: string;
}

export interface LanguageGuess {
  code: string;
  confidence: number;
  kanaShare: number;
  hanShare: number;
  latinShare: number;
  hangulShare: number;
}

export interface Validation {
  lineCount: number;
  timedLineCount: number;
  /** 0~1，估计值 */
  completeness: number;
  repeatedLineRatio: number;
  language: LanguageGuess;
  issues: Issue[];
}

/** 和 `quality::ISSUE_CODES` 同一套说法 */
export const ISSUE_LABELS: Record<string, string> = {
  empty: tr("一行歌词都没有"),
  replacement_char: tr("有乱码"),
  not_japanese: tr("看起来不是日文歌词"),
  too_few_lines: tr("行数太少"),
  ends_early: tr("提前结束，可能被截断"),
  untimed: tr("没有时间轴"),
  mixed_timing: tr("部分行没有时间"),
  non_monotonic: tr("时间倒退"),
  beyond_duration: tr("时间超过歌曲长度"),
  duplicate_timestamp: tr("和上一行同一个时间"),
  stacked_duplicate: tr("重复贴了一遍"),
  long_gap: tr("长空档（间奏或漏行）"),
  leftover_markup: tr("混进了时间标记或署名"),
  very_long_line: tr("一行特别长"),
};

export const LANGUAGE_LABELS: Record<string, string> = {
  ja: tr("日文"),
  en: tr("英文"),
  ko: tr("韩文"),
  zh: tr("中文"),
  mixed: tr("混合"),
  unknown: tr("未知"),
  "": tr("未判断"),
};

export const SOURCE_LABELS: Record<string, string> = {
  lrc_file: tr("导入时的 .lrc"),
  embedded: tr("音频内嵌"),
  sibling: tr("音频旁的 .lrc"),
  online: tr("在线"),
  manual_file: tr("你选的文件"),
  restored: tr("从旧版恢复"),
  manual_edit: tr("逐行编辑"),
  legacy: tr("老库（未记来源）"),
};

export const FIELD_SOURCE_LABELS: Record<string, string> = {
  tag: tr("文件标签"),
  filename: tr("文件名"),
  folder: tr("所在目录"),
  manual: tr("你改的"),
  heuristic: tr("按歌词推断"),
  audio: tr("音频文件"),
  itunes: "iTunes",
  musicbrainz: "MusicBrainz",
  deezer: "Deezer",
};

export const TOKEN_STATUS_LABELS: Record<string, string> = {
  auto: tr("自动分词"),
  corrected: tr("校正过"),
  pending: tr("未分词"),
  empty: tr("无实词"),
  unknown: tr("未记录"),
};

export const REVISION_LABELS: Record<string, string> = {
  lyrics_attached: tr("挂上歌词"),
  lyrics_replaced: tr("换了歌词"),
  lyrics_edited: tr("逐行编辑"),
  lyrics_timed: tr("手动打轴"),
  tokens_corrected: tr("校正分词"),
  tokens_reverted: tr("撤销校正"),
  corrections_restored: tr("套回校正"),
  retokenized: tr("重新分词"),
  metadata_edited: tr("改曲目信息"),
  song_deleted: tr("删除"),
  legacy_registered: tr("老库补登记"),
  lyrics_offset: tr("调整歌词偏移"),
};

export const issueLabel = (code: string) => ISSUE_LABELS[code] ?? code;

/** 完整度的一句话评价 */
export function completenessLabel(value: number | null | undefined): string {
  if (value === null || value === undefined) return tr("未校验");
  if (value >= 0.95) return tr("完整");
  if (value >= 0.8) return tr("基本完整");
  if (value >= 0.5) return tr("可能有缺");
  return tr("很可能有问题");
}

export const percent = (value: number | null | undefined, digits = 0) =>
  value === null || value === undefined ? "—" : `${(value * 100).toFixed(digits)}%`;

// ───────────────────────────── 来源 ─────────────────────────────

export interface TokenizerRun {
  engine: string;
  engineVersion: string;
  dictDescription: string;
  dictCreatedAt: string;
  dictSize: number;
  splitMode: string;
  configHash: string;
  posScheme: string;
}

export interface TokenizerRunRow extends TokenizerRun {
  id: number;
  fingerprint: string;
  firstUsedAt: string;
  lineCount: number;
}

/** 「sudachi.rs 0.6.11 · 词典描述 · Mode C」 */
export function tokenizerLabel(t: TokenizerRun): string {
  const dict = t.dictDescription || tr("未知词典");
  return `${t.engine} ${t.engineVersion} · ${dict} · Mode ${t.splitMode}`;
}

export interface LyricVersion {
  id: number;
  versionNo: number;
  sourceType: string;
  sourceName: string;
  sourceRef: string;
  importedAt: string;
  hasRawText: boolean;
  contentHash: string;
  lineCount: number;
  timedLineCount: number;
  completeness: number | null;
  validation: Validation | null;
  validatedAt: string;
  isCurrent: boolean;
  note: string;
}

export interface LineQuality {
  utteranceId: number;
  lineIdx: number;
  timeSec: number | null;
  timeConfidence: number | null;
  tokenStatus: string | null;
  tokenizerRunId: number | null;
}

export interface FieldProvenance {
  field: string;
  source: string;
  sourceRef: string;
  confidence: number | null;
  updatedAt: string;
}

export interface Revision {
  id: number;
  songId: string;
  songArtist: string;
  songTitle: string;
  kind: string;
  versionId: number | null;
  utteranceId: number | null;
  lineIdx: number | null;
  before: unknown;
  after: unknown;
  actor: string;
  note: string;
  createdAt: string;
}

export interface SongLineage {
  songId: string;
  language: string | null;
  versions: LyricVersion[];
  lines: LineQuality[];
  tokenStatus: Record<string, number>;
  tokenizers: TokenizerRunRow[];
  provenance: FieldProvenance[];
  revisions: Revision[];
  /** 音频文件记录（现在用的排最前）。老后端没有这一项 */
  audioFiles?: AudioFileRow[];
}

/** 一个音频文件（jp_corpus::audio_files）：文件本身的信息，和曲目元数据分开 */
export interface AudioFileRow {
  id: number;
  songId: string;
  path: string;
  size: number | null;
  mtime: number | null;
  durationSec: number | null;
  format: string;
  sampleRate: number | null;
  channels: number | null;
  bitDepth: number | null;
  bitrateKbps: number | null;
  fingerprint: string | null;
  trackStartMs: number;
  status: "present" | "missing" | "replaced";
  firstSeen: string;
  checkedAt: string;
}

export const AUDIO_FILE_STATUS: Record<AudioFileRow["status"], string> = {
  present: tr("在用"),
  missing: tr("找不到"),
  replaced: tr("已换掉"),
};

/** 「FLAC · 44.1 kHz · 16 bit · 2 声道 · 912 kbps · 24.3 MB」 */
export function describeAudioFile(f: AudioFileRow): string {
  const parts: string[] = [];
  if (f.format) parts.push(f.format.toUpperCase());
  if (f.sampleRate) parts.push(`${(f.sampleRate / 1000).toFixed(1).replace(/\.0$/, "")} kHz`);
  if (f.bitDepth) parts.push(`${f.bitDepth} bit`);
  if (f.channels) parts.push(tr("{n} 声道", { n: f.channels }));
  if (f.bitrateKbps) parts.push(`${f.bitrateKbps} kbps`);
  if (f.size !== null) parts.push(`${(f.size / 1024 / 1024).toFixed(1)} MB`);
  return parts.join(" · ");
}

export interface SongQuality {
  songId: string;
  title: string;
  artist: string;
  completeness: number | null;
  sourceType: string;
  warnings: number;
  errors: number;
  codes: string[];
}

export interface QualityOverview {
  songs: number;
  songsWithLyrics: number;
  songsWithVersion: number;
  legacyVersions: number;
  avgCompleteness: number | null;
  bySource: [string, number][];
  byLanguage: [string, number][];
  issues: [string, number][];
  tokenStatus: [string, number][];
  linesLowTimeConfidence: number;
  tokenizers: TokenizerRunRow[];
  worst: SongQuality[];
}

// ───────────────────────────── 分析 ─────────────────────────────

/** 统计页那两种用传进来的筛选；analysis.rs 那三种的范围写在 query 自己里面 */
export type AnalysisSpec =
  | { kind: "corpusReport" }
  | { kind: "frequency"; pos: string | null; limit: number }
  | { kind: "corpusFrequency"; query: FrequencyQuery }
  | { kind: "collocations"; query: CollocationQuery }
  | { kind: "keyness"; query: KeynessQuery };

/** 别处存了档，「分析记录」卡据此刷新列表 */
export const ANALYSIS_ARCHIVED = "jpop:analysis-archived";

/** 统计筛选的范围部分（去掉「仅日文」「副歌只算一遍」两个口径） */
export function toSubcorpus(f: StatsFilter): Subcorpus {
  const { jpOnly: _j, distinctLines: _d, ...scope } = f;
  return scope;
}

export interface ScopeSong {
  songId: string;
  versionId: number | null;
  lines: number;
}

export interface Scope {
  songs: ScopeSong[];
  songCount: number;
  lineCount: number;
}

export interface TokenizerUse {
  runs: TokenizerRunRow[];
  unknownLines: number;
}

export interface AnalysisRun {
  id: number;
  label: string;
  spec: AnalysisSpec;
  filter: StatsFilter;
  scope: Scope;
  scopeHash: string;
  tokenizers: TokenizerUse;
  method: Record<string, unknown>;
  result: unknown;
  resultHash: string;
  /** 当时的库指纹（analysis.rs 的 snapshot） */
  snapshot: CorpusSnapshot | null;
  schemaVersion: number;
  appVersion: string;
  rerunOf: number | null;
  createdAt: string;
}

export interface AnalysisSummary {
  id: number;
  kind: AnalysisSpec["kind"];
  label: string;
  filter: StatsFilter;
  songCount: number;
  scopeHash: string;
  resultHash: string;
  appVersion: string;
  rerunOf: number | null;
  createdAt: string;
}

export interface MetricDiff {
  name: string;
  before: number | null;
  after: number | null;
}

export interface WordDiff {
  lemma: string;
  pos: string;
  beforeRank: number | null;
  afterRank: number | null;
  beforeFreq: number | null;
  afterFreq: number | null;
}

export interface Comparison {
  before: AnalysisSummary;
  after: AnalysisSummary;
  sameFilter: boolean;
  sameSpec: boolean;
  sameMethod: boolean;
  sameScope: boolean;
  sameTokenizers: boolean;
  sameSchema: boolean;
  /** 库指纹一样吗；老存档没记指纹时是 null */
  sameCorpus: boolean | null;
  resultIdentical: boolean;
  scope: { added: string[]; removed: string[]; lyricsChanged: string[] };
  metrics: MetricDiff[];
  words: WordDiff[];
  reasons: string[];
}

export interface SavedSubcorpus {
  id: number;
  name: string;
  filter: StatsFilter;
  note: string;
  createdAt: string;
  updatedAt: string;
  songCount: number;
  lineCount: number;
}

export interface FilterFacets {
  yearMin: number | null;
  yearMax: number | null;
  songsWithoutYear: number;
  genres: [string, number][];
  languages: [string, number][];
}

export const KIND_LABELS: Record<AnalysisSpec["kind"], string> = {
  corpusReport: tr("语料报告"),
  frequency: tr("词频表"),
  corpusFrequency: tr("重复感知词频"),
  collocations: tr("搭配"),
  keyness: tr("子语料对比"),
};

/** 报告指标的中文名（对比表里用） */
export const METRIC_LABELS: Record<string, string> = {
  songCount: tr("曲数"),
  utteranceCount: tr("歌词行数"),
  tokenCount: tr("词次"),
  typeCount: tr("词种"),
  ttr: "TTR",
  sttr: "STTR",
  sttrChunk: tr("STTR 段长"),
  hapaxCount: tr("Hapax 数"),
  hapaxRatio: tr("Hapax 比率"),
  avgTypesPerSong: tr("平均每曲词汇量"),
  avgTokensPerLine: tr("平均每行 token"),
};

const SIDE_LABELS: Record<string, string> = { provenance: "", target: tr("目标"), reference: tr("参照") };
const SIDE_FIELDS: Record<string, string> = { songs: tr("曲数"), tokens: tr("词次") };

export const metricLabel = (name: string): string => {
  const [side, field] = name.split(".");
  if (field && side! in SIDE_LABELS && field in SIDE_FIELDS) return `${SIDE_LABELS[side!]}${SIDE_FIELDS[field]}`;
  return baseMetricLabel(name);
};

const baseMetricLabel = (name: string) =>
  METRIC_LABELS[name] ?? (name.startsWith("coverage@") ? tr("Top {slice} 覆盖率", { slice: name.slice(9) }) : name);

/** 一个筛选条件是不是「整个库」（只看范围，不看 jpOnly / distinctLines 这类口径） */
export function isWholeCorpus(f: StatsFilter): boolean {
  return (
    f.performerIds.length === 0 &&
    f.yearFrom == null &&
    f.yearTo == null &&
    !f.genres?.length &&
    !f.languages?.length &&
    !f.albumIds?.length &&
    !f.songIds?.length &&
    f.minCompleteness == null
  );
}

/**
 * 筛选条件的一句话描述。演唱者只有 id，名字由调用方给（`performerNames`），给不了就写「N 位演唱者」。
 */
export function describeFilter(f: StatsFilter, performerNames: string[] = []): string {
  const parts: string[] = [];
  if (f.performerIds.length) {
    parts.push(performerNames.length ? performerNames.join(tr("、")) : tr("{length} 位演唱者", { length: f.performerIds.length }));
  }
  if (f.yearFrom != null && f.yearTo != null) parts.push(f.yearFrom === f.yearTo ? tr("{yearFrom} 年", { yearFrom: f.yearFrom }) : tr("{yearFrom}–{yearTo} 年", { yearFrom: f.yearFrom, yearTo: f.yearTo }));
  else if (f.yearFrom != null) parts.push(tr("{yearFrom} 年起", { yearFrom: f.yearFrom }));
  else if (f.yearTo != null) parts.push(tr("{yearTo} 年前", { yearTo: f.yearTo }));
  if (f.genres?.length) parts.push(f.genres.join(" / "));
  if (f.languages?.length) parts.push(f.languages.map((l) => LANGUAGE_LABELS[l] ?? l).join(" / "));
  if (f.albumIds?.length) parts.push(tr("{length} 张专辑", { length: f.albumIds.length }));
  if (f.songIds?.length) parts.push(tr("{length} 首指定的歌", { length: f.songIds.length }));
  if (f.minCompleteness != null) parts.push(tr("歌词完整度 ≥ {minCompleteness}", { minCompleteness: percent(f.minCompleteness) }));
  const scope = parts.length ? parts.join(" · ") : tr("整个库");
  const method = [f.jpOnly && tr("仅日文"), f.distinctLines && tr("副歌只算一遍")].filter(Boolean).join(tr("，"));
  return method ? `${scope}${tr("（")}${method}${tr("）")}` : scope;
}

/** 把空数组、undefined 去掉，两份条件才比得出「一样」 */
export function normalizeFilter(f: StatsFilter): StatsFilter {
  const out: StatsFilter = { performerIds: [...f.performerIds].sort((a, b) => a - b), jpOnly: f.jpOnly };
  if (f.yearFrom != null) out.yearFrom = f.yearFrom;
  if (f.yearTo != null) out.yearTo = f.yearTo;
  if (f.genres?.length) out.genres = [...f.genres];
  if (f.languages?.length) out.languages = [...f.languages];
  if (f.albumIds?.length) out.albumIds = [...f.albumIds];
  if (f.songIds?.length) out.songIds = [...f.songIds];
  if (f.minCompleteness != null) out.minCompleteness = f.minCompleteness;
  if (f.distinctLines) out.distinctLines = true;
  return out;
}

export const sameFilter = (a: StatsFilter, b: StatsFilter) =>
  JSON.stringify(normalizeFilter(a)) === JSON.stringify(normalizeFilter(b));

// ───────────────────────────── 命令 ─────────────────────────────

export const qualityApi = {
  overview: (limit = 50) => call<QualityOverview>("quality_overview", { limit }),
  songLineage: (songId: string) => call<SongLineage>("song_lineage", { songId }),
  versionRaw: (versionId: number) => call<string | null>("lyric_version_raw", { versionId }),
  restoreVersion: (songId: string, versionId: number) =>
    call<{ lyricLines: number; tokens: number; correctionsRestored: number }>("lyrics_restore_version", { songId, versionId }),
  /** 逐行编辑之后存成新的一版歌词 */
  saveLines: (songId: string, lines: { timeSec: number | null; text: string }[], note = "") =>
    call<{ lyricLines: number; tokens: number; correctionsRestored: number }>("lyrics_save_lines", { songId, lines, note }),
  /** 一个歌手资料各字段的来源 */
  artistProvenance: (name: string) => call<FieldProvenance[]>("artist_provenance", { name }),
  revalidate: (songId?: string) => call<number>("lyrics_revalidate", { songId: songId ?? null }),
  setLanguage: (songId: string, language: string) => call<void>("song_set_language", { songId, language }),

  facets: () => call<FilterFacets>("subcorpus_facets"),
  preview: (filter: StatsFilter) => call<Scope>("subcorpus_preview", { filter }),
  subcorpora: () => call<SavedSubcorpus[]>("subcorpus_list"),
  saveSubcorpus: (name: string, filter: StatsFilter, note = "") =>
    call<SavedSubcorpus>("subcorpus_save", { name, filter, note }),
  deleteSubcorpus: (id: number) => call<boolean>("subcorpus_delete", { id }),

  runAnalysis: (spec: AnalysisSpec, filter: StatsFilter, label = "") =>
    call<AnalysisRun>("analysis_run", { spec, filter, label }),
  analyses: (limit = 100) => call<AnalysisSummary[]>("analysis_list", { limit }),
  analysis: (id: number) => call<AnalysisRun | null>("analysis_get", { id }),
  rerun: (id: number) => call<Comparison>("analysis_rerun", { id }),
  compare: (before: number, after: number) => call<Comparison>("analysis_compare", { before, after }),
  deleteAnalysis: (id: number) => call<boolean>("analysis_delete", { id }),
};
