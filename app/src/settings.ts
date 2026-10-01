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

export interface AppSettings {
  /** 全屏歌词的背景：封面虚化铺满，而不是从封面取一个纯色 */
  stageCoverBlur: boolean;
  /** 虚化强度，px */
  stageBlurRadius: number;
  /** 封面网格先显示歌手，点进去才是他的歌 */
  gridByArtist: boolean;
}

export const STAGE_BLUR_LIMITS = [20, 120] as const;

export const DEFAULT_SETTINGS: AppSettings = {
  stageCoverBlur: true,
  stageBlurRadius: 64,
  gridByArtist: true,
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
      stageCoverBlur: typeof parsed.stageCoverBlur === "boolean" ? parsed.stageCoverBlur : d.stageCoverBlur,
      stageBlurRadius: clamp(parsed.stageBlurRadius, STAGE_BLUR_LIMITS, d.stageBlurRadius),
      gridByArtist: typeof parsed.gridByArtist === "boolean" ? parsed.gridByArtist : d.gridByArtist,
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
