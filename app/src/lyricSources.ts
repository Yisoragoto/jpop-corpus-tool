/**
 * 歌词来源、整体偏移、统一时间轴（Rust：jp_corpus::lyric_sources / lyric_timeline、jp_lyrics::sources）。
 *
 * 时间的三种说法见 lyric_timeline.rs：歌词时间（文件里写的）+ 偏移 = 曲目时间（播放器位置）。
 * 偏移落库时已经加进了 `lyrics` 返回的 timeSec，所以播放同步、点行跳转直接用 timeSec；
 * 这里的时间轴给的是歌词时间，要换成曲目时间用 `trackMs`。
 */

import { call, type AttachedLyrics } from "./api";
import { tr } from "./i18n";

export type SourceKind = "user" | "sibling" | "embedded" | "online" | "legacy";
export type Precision = "none" | "line" | "word";

export interface LyricSourceRow {
  id: number;
  songId: string;
  kind: SourceKind;
  provider: string;
  sourceRef: string;
  role: "original" | "translation";
  language: string;
  sourceType: string;
  rawFormat: string;
  hasRaw: boolean;
  contentHash: string;
  lineCount: number;
  timedLineCount: number;
  precision: Precision;
  userModified: boolean;
  missing: boolean;
  offsetMs: number;
  updatedAt: string;
}

export interface LyricSelection {
  sourceId: number | null;
  translationSourceId: number | null;
  translationOff: boolean;
  chosenBy: "user" | "auto" | "legacy";
  offsetMs: number;
  updatedAt: string;
}

export interface LyricLookup {
  provider: string;
  status: "found" | "not_found" | "failed" | "ambiguous";
  message: string;
  checkedAt: string;
  retryAfter: string;
}

export interface LyricSourcesView {
  songId: string;
  sources: LyricSourceRow[];
  selection: LyricSelection | null;
  recommended: number | null;
  translationSourceId: number | null;
  lookups: LyricLookup[];
}

export interface TimelineWord {
  charStart: number;
  charEnd: number;
  startMs: number;
  endMs: number | null;
}

export interface TimelineLine {
  id: string;
  utteranceId: number | null;
  startMs: number | null;
  endMs: number | null;
  text: string;
  translation: string | null;
  words?: TimelineWord[];
}

export interface LyricTimeline {
  songId: string;
  sourceId: number | null;
  sourceKind: string;
  rawFormat: string;
  precision: Precision;
  offsetMs: number;
  trackStartMs: number;
  durationMs: number | null;
  parserVersion: number;
  lines: TimelineLine[];
  translation: {
    state: "none" | "aligned" | "unaligned";
    language: string;
    aligned: number;
    total: number;
    unalignedLines?: string[];
  };
  warnings?: string[];
}

/** 偏移的上限（和 Rust 的 MAX_OFFSET_MS 一致） */
export const MAX_OFFSET_MS = 30_000;

/** 歌词时间 → 曲目时间（播放器位置）。显示和跳转都走这一个换算 */
export const trackMs = (lyricMs: number, offsetMs: number) => lyricMs + offsetMs;
/** 曲目时间 → 歌词时间 */
export const lyricMs = (trackMsValue: number, offsetMs: number) => trackMsValue - offsetMs;

export const lyricSourcesApi = {
  view: (songId: string) => call<LyricSourcesView>("lyric_source_view", { songId }),
  raw: (sourceId: number) => call<string | null>("lyric_source_raw", { sourceId }),
  sourceTimeline: (sourceId: number) => call<LyricTimeline>("lyric_source_timeline", { sourceId }),
  songTimeline: (songId: string) => call<LyricTimeline>("lyric_song_timeline", { songId }),
  apply: (songId: string, sourceId: number) => call<AttachedLyrics>("lyric_source_apply", { songId, sourceId }),
  auto: (songId: string) => call<AttachedLyrics | null>("lyric_source_auto", { songId }),
  selectTranslation: (songId: string, sourceId: number | null) =>
    call<void>("lyric_translation_select", { songId, sourceId }),
  setOffset: (songId: string, offsetMs: number) => call<number>("lyric_offset_set", { songId, offsetMs }),
};

export const KIND_LABELS: Record<SourceKind, string> = {
  user: tr("你导入或编辑的"),
  sibling: tr("音频旁边的文件"),
  embedded: tr("音频内嵌"),
  online: tr("在线"),
  legacy: tr("升级前已有"),
};

export const PRECISION_LABELS: Record<Precision, string> = {
  word: tr("逐字"),
  line: tr("逐行"),
  none: tr("无时间"),
};

export const CHOSEN_BY_LABELS: Record<LyricSelection["chosenBy"], string> = {
  user: tr("你选定的，自动流程不会换"),
  auto: tr("按优先级自动选的"),
  legacy: tr("升级前就在用的，自动流程不会换"),
};

export const LOOKUP_LABELS: Record<LyricLookup["status"], string> = {
  found: tr("在线找到了"),
  not_found: tr("在线没找到"),
  failed: tr("在线请求失败，可以重试"),
  ambiguous: tr("在线候选对不准，等你挑"),
};

/** 偏移写成「+0.25 秒」 */
export function formatOffset(ms: number): string {
  const sec = (ms / 1000).toFixed(ms % 100 === 0 ? 1 : 2);
  return tr("{sign}{sec} 秒", { sign: ms > 0 ? "+" : "", sec });
}
