/**
 * 曲库歌词的显示设置：字体、字号、振假名、行距、字距。
 *
 * 和 Python 版曲库的「显示」对话框同一组选项（范围照搬它的 `_song_display_settings`），
 * 默认值取新版原来的样子，打开不会突然变样。只关乎这台机器上的这个用户，存 localStorage。
 */

import { useEffect, useRef, useState, useSyncExternalStore } from "react";

import { call } from "./api";

export type FuriganaMode = "kanji" | "word";

export const FURIGANA_MODE_LABELS: Record<FuriganaMode, string> = {
  kanji: "只注汉字",
  word: "整词注音",
};

export interface LyricsDisplay {
  furigana: boolean;
  furiganaMode: FuriganaMode;
  /** 歌词主字体；空串是界面原来的字体 */
  fontFamily: string;
  /** 备用字体（主字体缺字时用）；空串是不设 */
  fallbackFamily: string;
  /** 歌词字号，px */
  fontSize: number;
  /** 振假名字号，px */
  rubySize: number;
  /** 行与行之间额外的空隙，px */
  lineGap: number;
  /** 字距，px */
  letterSpacing: number;
}

/** 可调范围（和 Python 版一致） */
export const DISPLAY_LIMITS = {
  fontSize: [12, 30],
  rubySize: [6, 18],
  lineGap: [0, 36],
  letterSpacing: [0, 12],
} as const satisfies Record<string, readonly [number, number]>;

export const DEFAULT_LYRICS_DISPLAY: LyricsDisplay = {
  furigana: false,
  furiganaMode: "kanji",
  fontFamily: "",
  fallbackFamily: "",
  fontSize: 15,
  rubySize: 9,
  lineGap: 6,
  letterSpacing: 0,
};

const STORAGE_KEY = "jp.lyrics.display";

/** 字体名：去掉引号和反斜杠（要拼进 CSS），限长 */
function cleanFamily(value: unknown): string {
  return typeof value === "string" ? value.replace(/["\\]/g, "").trim().slice(0, 120) : "";
}

function clampNumber(value: unknown, [low, high]: readonly [number, number], fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? Math.min(high, Math.max(low, Math.round(value))) : fallback;
}

/** 读回来的设置可能是旧版本或被手改过的：逐项校验，坏的用默认值 */
export function parseLyricsDisplay(raw: string | null): LyricsDisplay {
  if (raw === null) return DEFAULT_LYRICS_DISPLAY;
  try {
    const parsed = JSON.parse(raw) as Partial<Record<keyof LyricsDisplay, unknown>>;
    const d = DEFAULT_LYRICS_DISPLAY;
    return {
      furigana: typeof parsed.furigana === "boolean" ? parsed.furigana : d.furigana,
      furiganaMode: parsed.furiganaMode === "word" || parsed.furiganaMode === "kanji" ? parsed.furiganaMode : d.furiganaMode,
      fontFamily: cleanFamily(parsed.fontFamily),
      fallbackFamily: cleanFamily(parsed.fallbackFamily),
      fontSize: clampNumber(parsed.fontSize, DISPLAY_LIMITS.fontSize, d.fontSize),
      rubySize: clampNumber(parsed.rubySize, DISPLAY_LIMITS.rubySize, d.rubySize),
      lineGap: clampNumber(parsed.lineGap, DISPLAY_LIMITS.lineGap, d.lineGap),
      letterSpacing: clampNumber(parsed.letterSpacing, DISPLAY_LIMITS.letterSpacing, d.letterSpacing),
    };
  } catch {
    return DEFAULT_LYRICS_DISPLAY;
  }
}

function load(): LyricsDisplay {
  try {
    return parseLyricsDisplay(localStorage.getItem(STORAGE_KEY));
  } catch {
    return DEFAULT_LYRICS_DISPLAY;
  }
}

let display = load();
const listeners = new Set<() => void>();

export function updateLyricsDisplay(patch: Partial<LyricsDisplay>) {
  display = parseLyricsDisplay(JSON.stringify({ ...display, ...patch }));
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(display));
  } catch {
    // 存不了就只在这次会话里生效
  }
  for (const listener of listeners) listener();
}

export function resetLyricsDisplay() {
  // 振假名开关不算「显示样式」，恢复默认时保留
  updateLyricsDisplay({ ...DEFAULT_LYRICS_DISPLAY, furigana: display.furigana });
}

export function useLyricsDisplay(): LyricsDisplay {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => display,
    () => display,
  );
}

/** 一段注音在歌词原文里的位置，按字符（码点）数，左闭右开 */
export interface Ruby {
  start: number;
  end: number;
  reading: string;
}

export interface LineFurigana {
  utteranceId: number;
  rubies: Ruby[];
}

export const furiganaApi = {
  /** 整首歌的振假名，现场用 Sudachi 算（和 Python 版逐段一致） */
  song: (songId: string, mode: FuriganaMode) => call<LineFurigana[]>("lyrics_furigana", { songId, mode }),
};

/** 记住最近几首歌的振假名，来回切歌、切模式不重算 */
const FURIGANA_CACHE_SIZE = 30;

/**
 * 「再算一次振假名」。
 *
 * 没有词典时这首歌算失败、什么都没缓存，而 `key` 没变，effect 不会自己重跑——
 * 词典刚下好的那一刻就得有人推它一把。
 */
let retryTick = 0;
const retryListeners = new Set<() => void>();

export function retryFurigana() {
  retryTick += 1;
  for (const listener of retryListeners) listener();
}

function subscribeRetry(listener: () => void) {
  retryListeners.add(listener);
  return () => {
    retryListeners.delete(listener);
  };
}

/** 振假名开着时取这首歌的注音：utteranceId → 注音段。关着、还没取到或出错时是 null */
export function useSongFurigana(
  songId: string | undefined,
  enabled: boolean,
  mode: FuriganaMode,
  onError: (message: string) => void,
): Map<number, Ruby[]> | null {
  const cache = useRef(new Map<string, Map<number, Ruby[]>>());
  const [result, setResult] = useState<{ key: string; map: Map<number, Ruby[]> } | null>(null);
  const key = songId !== undefined && enabled ? `${songId}|${mode}` : null;
  const tick = useSyncExternalStore(subscribeRetry, () => retryTick);

  useEffect(() => {
    if (key === null || songId === undefined) return;
    const cached = cache.current.get(key);
    if (cached !== undefined) {
      setResult({ key, map: cached });
      return;
    }
    let alive = true;
    furiganaApi.song(songId, mode).then(
      (lines) => {
        const map = new Map(lines.map((line) => [line.utteranceId, line.rubies]));
        cache.current.set(key, map);
        if (cache.current.size > FURIGANA_CACHE_SIZE) {
          const oldest = cache.current.keys().next().value;
          if (oldest !== undefined) cache.current.delete(oldest);
        }
        if (alive) setResult({ key, map });
      },
      (err: unknown) => {
        if (alive) onError(err instanceof Error ? err.message : String((err as { message?: string })?.message ?? err));
      },
    );
    return () => {
      alive = false;
    };
    // onError 每次渲染都是新函数也没关系：只在 key 变、或者有人喊「再算一次」时取
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, tick]);

  return key !== null && result?.key === key ? result.map : null;
}
