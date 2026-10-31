/**
 * 应用级偏好：外观、曲库视图里那些不属于某一页的开关。
 *
 * 歌词的字体字号在 `lyricsDisplay.ts`，制卡在 `dict/mine.ts`——那两处各自有自己的存储，
 * 这里只放剩下的。三处都是同一套写法：模块级的一份状态 + `useSyncExternalStore`，
 * 改了立刻生效、自动记住，没有「确定」按钮。
 *
 * 只关乎这台机器上的这个用户，存 localStorage；存不了（隐私模式之类）就只在这次会话里生效。
 */

import { useSyncExternalStore } from "react";

import { PITCH_CACHE_LIMIT_RANGE } from "./pitchCache";

export interface AppSettings {
  /** 外观：跟随系统 / 浅色 / 深色。窗口本身（亚克力、标题栏）也跟着切，见 `theme.ts` */
  theme: "system" | "light" | "dark";
  /** 全屏歌词的背景：封面虚化铺满，而不是从封面取一个纯色 */
  stageCoverBlur: boolean;
  /** 虚化强度，px */
  stageBlurRadius: number;
  /** 播放条上的频谱条。关掉之后连采样轮询一起停，不再每 50ms 问一次引擎 */
  spectrum: boolean;
  /** 启动时问一次 GitHub Releases 有没有新版 */
  autoCheckUpdates: boolean;
  /** 启动检查时弹窗里点了「跳过这个版本」的那一版：这一版不再提醒，更新的版本照样提醒 */
  skippedUpdateVersion: string;
  /** 变调缓存最多占多少 MB，超了从最久没放的清起。启动时和改动时报给后端（见 `pitchCache.ts`） */
  pitchCacheLimitMb: number;
  /** 启动时看一眼语料库的文件夹里有没有还没导入的音频（只读，发现了只提示） */
  autoCheckNewSongs: boolean;
  /** 首页「今日」那个环的目标：每天听多少分钟 */
  dailyGoalMinutes: number;
  /** 统计页词云的字体（CSS 字体名）。可以是自带、导入或本机的任何一种；空串用界面字体 */
  wordCloudFont: string;
  /** 词云放多少个词 */
  wordCloudCount: number;
  /** 词云去掉虚词：单个假名的碎片（僕「ら」、一「つ」）、形式名词（こと・もの・まま）、纯符号 */
  wordCloudHideFunctionWords: boolean;
  /** 自己从词云里拿掉的词（词元） */
  wordCloudHidden: string[];
  /** 词云按词性上色（名词、动词、形容词……各一种颜色）；关着时按名次、前几名染照片的颜色 */
  wordCloudColorByPos: boolean;
  /** 只对完整有效的真实单位时间启用卡拉 OK；其余逐行显示，纯文本不高亮。 */
  karaokeLyrics: boolean;
}

export const STAGE_BLUR_LIMITS = [20, 120] as const;
export const DAILY_GOAL_LIMITS = [5, 180] as const;
export const WORD_CLOUD_COUNT_LIMITS = [30, 250] as const;

export const DEFAULT_SETTINGS: AppSettings = {
  theme: "system",
  stageCoverBlur: true,
  stageBlurRadius: 64,
  spectrum: true,
  autoCheckUpdates: true,
  skippedUpdateVersion: "",
  pitchCacheLimitMb: 1536,
  autoCheckNewSongs: true,
  dailyGoalMinutes: 30,
  // 自带的手写风格字体，装好就有
  wordCloudFont: "Klee One",
  wordCloudCount: 120,
  wordCloudHideFunctionWords: true,
  wordCloudHidden: [],
  wordCloudColorByPos: false,
  karaokeLyrics: true,
};

const STORAGE_KEY = "jp.app.settings";

function clamp(value: unknown, [low, high]: readonly [number, number], fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? Math.min(high, Math.max(low, Math.round(value))) : fallback;
}

/** 读回来的可能是旧版本或被手改过的：逐项校验，坏的用默认值 */
export function parseSettings(raw: string | null): AppSettings {
  if (raw === null) return DEFAULT_SETTINGS;
  try {
    const parsed = JSON.parse(raw) as Partial<Record<keyof AppSettings, unknown>>;
    const d = DEFAULT_SETTINGS;
    return {
      theme: parsed.theme === "light" || parsed.theme === "dark" ? parsed.theme : d.theme,
      stageCoverBlur: typeof parsed.stageCoverBlur === "boolean" ? parsed.stageCoverBlur : d.stageCoverBlur,
      stageBlurRadius: clamp(parsed.stageBlurRadius, STAGE_BLUR_LIMITS, d.stageBlurRadius),
      spectrum: typeof parsed.spectrum === "boolean" ? parsed.spectrum : d.spectrum,
      autoCheckUpdates:
        typeof parsed.autoCheckUpdates === "boolean" ? parsed.autoCheckUpdates : d.autoCheckUpdates,
      skippedUpdateVersion:
        typeof parsed.skippedUpdateVersion === "string" && parsed.skippedUpdateVersion.length <= 64
          ? parsed.skippedUpdateVersion
          : d.skippedUpdateVersion,
      pitchCacheLimitMb: clamp(parsed.pitchCacheLimitMb, PITCH_CACHE_LIMIT_RANGE, d.pitchCacheLimitMb),
      autoCheckNewSongs:
        typeof parsed.autoCheckNewSongs === "boolean" ? parsed.autoCheckNewSongs : d.autoCheckNewSongs,
      dailyGoalMinutes: clamp(parsed.dailyGoalMinutes, DAILY_GOAL_LIMITS, d.dailyGoalMinutes),
      wordCloudFont:
        typeof parsed.wordCloudFont === "string" && parsed.wordCloudFont.length <= 200
          ? parsed.wordCloudFont.trim()
          : d.wordCloudFont,
      wordCloudCount: clamp(parsed.wordCloudCount, WORD_CLOUD_COUNT_LIMITS, d.wordCloudCount),
      wordCloudHideFunctionWords:
        typeof parsed.wordCloudHideFunctionWords === "boolean"
          ? parsed.wordCloudHideFunctionWords
          : d.wordCloudHideFunctionWords,
      wordCloudColorByPos:
        typeof parsed.wordCloudColorByPos === "boolean" ? parsed.wordCloudColorByPos : d.wordCloudColorByPos,
      karaokeLyrics: typeof parsed.karaokeLyrics === "boolean" ? parsed.karaokeLyrics : d.karaokeLyrics,
      wordCloudHidden: Array.isArray(parsed.wordCloudHidden)
        ? [...new Set(parsed.wordCloudHidden.filter((w): w is string => typeof w === "string" && w.length > 0 && w.length <= 64))].slice(0, 2000)
        : d.wordCloudHidden,
    };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

function load(): AppSettings {
  try {
    return parseSettings(localStorage.getItem(STORAGE_KEY));
  } catch {
    return DEFAULT_SETTINGS;
  }
}

let settings = load();
const listeners = new Set<() => void>();

export function getAppSettings(): AppSettings { return settings; }

export function updateSettings(patch: Partial<AppSettings>) {
  settings = parseSettings(JSON.stringify({ ...settings, ...patch }));
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // 存不了就只在这次会话里生效
  }
  for (const listener of listeners) listener();
}

export function useAppSettings(): AppSettings {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => settings,
    () => DEFAULT_SETTINGS,
  );
}
