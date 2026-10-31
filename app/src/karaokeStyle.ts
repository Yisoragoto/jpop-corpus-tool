/**
 * 卡拉 OK 字幕的样式：字体、字号、粗细，没唱 / 唱过两套「填色 + 描边 + 发光」，立体阴影，两行错开。
 *
 * 字段名、预设和校验规则和 macOS 的 `JPCore/KaraokeStyle.swift` 一字不差——两边的测试读同一份
 * `macos/Tests/JPCoreTests/Fixtures/KaraokeStyle.json`，改一边忘了另一边测试会挂。
 *
 * 和其它偏好一样：模块级一份状态 + `useSyncExternalStore`，改了立刻生效，存 localStorage。
 */

import { useSyncExternalStore } from "react";

export interface KaraokeStyle {
  /** 空串：跟歌词字体走 */
  fontFamily: string;
  /** 窗口够宽时的字号（px）；窗口窄了按比例缩，描边、发光、阴影跟着缩 */
  fontSize: number;
  /** 400–900，100 一档 */
  fontWeight: number;
  unsungFill: string;
  unsungStroke: string;
  unsungGlow: string;
  sungFill: string;
  sungStroke: string;
  sungGlow: string;
  /** 描边粗细（px，按 fontSize 算） */
  strokeWidth: number;
  /** 发光半径（px）；0 不发光 */
  glowStrength: number;
  /** 立体阴影的颜色 */
  shadowColor: string;
  /** 立体阴影往右下叠出去的厚度（px）；0 没有 */
  shadowDepth: number;
  /** 上行靠左、下行靠右；关掉两行都居中 */
  staggered: boolean;
}

export type KaraokePresetId = "classic" | "pink" | "plain";

const base = { fontFamily: "", fontSize: 56, staggered: true } as const;

export const KARAOKE_PRESETS: Record<KaraokePresetId, KaraokeStyle> = {
  // 经典 KTV：白字黑边，唱过变蓝、白边、蓝色发光
  classic: {
    ...base, fontWeight: 900,
    unsungFill: "#ffffff", unsungStroke: "#10121a", unsungGlow: "#1e6fff",
    sungFill: "#2f86ff", sungStroke: "#ffffff", sungGlow: "#3aa2ff",
    strokeWidth: 5, glowStrength: 12, shadowColor: "#000000", shadowDepth: 3,
  },
  // 粉色像素风 MV：白字粉紫边，唱过变粉、白边、粉色发光
  pink: {
    ...base, fontWeight: 800,
    unsungFill: "#ffffff", unsungStroke: "#d96bf0", unsungGlow: "#e070ff",
    sungFill: "#ec4f95", sungStroke: "#ffffff", sungGlow: "#ff5fb0",
    strokeWidth: 4, glowStrength: 16, shadowColor: "#6a2a86", shadowDepth: 3,
  },
  // 原来的样子：没有描边，只有一圈深色投影
  plain: {
    ...base, fontWeight: 700,
    unsungFill: "#ffffff", unsungStroke: "#000000", unsungGlow: "#000000",
    sungFill: "#4cc2ff", sungStroke: "#000000", sungGlow: "#000000",
    strokeWidth: 0, glowStrength: 6, shadowColor: "#000000", shadowDepth: 0,
  },
};

export const DEFAULT_KARAOKE_STYLE: KaraokeStyle = KARAOKE_PRESETS.classic;

export const KARAOKE_LIMITS = {
  fontSize: [28, 96],
  fontWeight: [400, 900],
  strokeWidth: [0, 12],
  glowStrength: [0, 40],
  shadowDepth: [0, 12],
} as const satisfies Record<string, readonly [number, number]>;

export const COLOR_KEYS = ["unsungFill", "unsungStroke", "unsungGlow", "sungFill", "sungStroke", "sungGlow", "shadowColor"] as const;

const HEX = /^#[0-9a-f]{6}$/i;

function num(value: unknown, [low, high]: readonly [number, number], fallback: number, step = 1): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return fallback;
  return Math.min(high, Math.max(low, Math.round(value / step) * step));
}

/** 读回来的可能是旧版本、被手改过的：逐项校验，坏的项用默认值 */
export function normalizeKaraokeStyle(input: unknown): KaraokeStyle {
  const raw = (typeof input === "object" && input !== null ? input : {}) as Partial<Record<keyof KaraokeStyle, unknown>>;
  const d = DEFAULT_KARAOKE_STYLE;
  const out: KaraokeStyle = {
    fontFamily: typeof raw.fontFamily === "string" && raw.fontFamily.length <= 200 ? raw.fontFamily.trim() : d.fontFamily,
    fontSize: num(raw.fontSize, KARAOKE_LIMITS.fontSize, d.fontSize),
    fontWeight: num(raw.fontWeight, KARAOKE_LIMITS.fontWeight, d.fontWeight, 100),
    unsungFill: d.unsungFill, unsungStroke: d.unsungStroke, unsungGlow: d.unsungGlow,
    sungFill: d.sungFill, sungStroke: d.sungStroke, sungGlow: d.sungGlow,
    strokeWidth: num(raw.strokeWidth, KARAOKE_LIMITS.strokeWidth, d.strokeWidth),
    glowStrength: num(raw.glowStrength, KARAOKE_LIMITS.glowStrength, d.glowStrength),
    shadowColor: d.shadowColor,
    shadowDepth: num(raw.shadowDepth, KARAOKE_LIMITS.shadowDepth, d.shadowDepth),
    staggered: typeof raw.staggered === "boolean" ? raw.staggered : d.staggered,
  };
  for (const key of COLOR_KEYS) {
    const value = raw[key];
    if (typeof value === "string" && HEX.test(value)) out[key] = value.toLowerCase();
  }
  return out;
}

/** 当前样式正好是哪套预设（改过任何一项就不是了） */
export function matchingPreset(style: KaraokeStyle): KaraokePresetId | null {
  for (const id of Object.keys(KARAOKE_PRESETS) as KaraokePresetId[]) {
    const preset = KARAOKE_PRESETS[id];
    // 字体、字号不算在预设里：换预设时保留自己挑的字体和大小
    const same = (Object.keys(preset) as (keyof KaraokeStyle)[])
      .every((k) => k === "fontFamily" || k === "fontSize" || preset[k] === style[k]);
    if (same) return id;
  }
  return null;
}

/** 套用预设：颜色、描边、发光、阴影、粗细换掉，字体和字号留着 */
export function applyPreset(style: KaraokeStyle, id: KaraokePresetId): KaraokeStyle {
  return { ...KARAOKE_PRESETS[id], fontFamily: style.fontFamily, fontSize: style.fontSize };
}

/** 立体阴影的几步位移：1、2、4… 翻倍叠，最后补零头。每一步都把之前叠好的整块再挪一次，
 *  所以 k 步就能连成 0…厚度 的一整块侧面（厚度 12 只要 4 步，不用 12 个滤镜） */
export function extrusionSteps(depth: number): number[] {
  const steps: number[] = [];
  let covered = 0;
  for (let next = 1; covered + next <= depth; next *= 2) { steps.push(next); covered += next; }
  if (covered < depth) steps.push(depth - covered);
  return steps;
}

/**
 * 给 `.ktv` 的 CSS 变量。长度都换成 em（按 fontSize 折算），窗口窄了字缩小时描边、发光、阴影一起缩。
 *
 * 立体阴影和发光用 `filter: drop-shadow`，不用 text-shadow：Chromium 画描边字时 text-shadow 描边一份、
 * 填色一份，填色那份会盖在描边上面，阴影就错位地夹在描边和字之间。drop-shadow 投的是描好边的整个字。
 * 先叠侧面（实心、往右下），再在外面发光。
 */
export function karaokeCssVars(style: KaraokeStyle): Record<string, string> {
  const em = (px: number) => `${(px / style.fontSize).toFixed(4)}em`;
  const extrusion = extrusionSteps(style.shadowDepth).map((n) => `drop-shadow(${em(n)} ${em(n)} 0 ${style.shadowColor})`);
  const glow = (color: string) => style.glowStrength === 0
    ? []
    : [`drop-shadow(0 0 ${em(style.glowStrength / 4)} ${color})`, `drop-shadow(0 0 ${em(style.glowStrength / 2)} ${color})`];
  const filter = (color: string) => [...extrusion, ...glow(color)].join(" ") || "none";
  return {
    "--ktv-size": `${style.fontSize}px`,
    "--ktv-size-vw": `${(style.fontSize * 0.075).toFixed(3)}vw`,
    "--ktv-weight": String(style.fontWeight),
    "--ktv-unsung-fill": style.unsungFill,
    "--ktv-unsung-stroke": style.unsungStroke,
    "--ktv-sung-fill": style.sungFill,
    "--ktv-sung-stroke": style.sungStroke,
    // 描边居中画在字形轮廓上、填色盖住里面一半（paint-order），所以要画两倍宽，露在外面的才是设定的粗细
    "--ktv-stroke": em(style.strokeWidth * 2),
    "--ktv-unsung-filter": filter(style.unsungGlow),
    "--ktv-sung-filter": filter(style.sungGlow),
  };
}

const STORAGE_KEY = "jp.app.karaokeStyle";

function load(): KaraokeStyle {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw === null ? DEFAULT_KARAOKE_STYLE : normalizeKaraokeStyle(JSON.parse(raw));
  } catch {
    return DEFAULT_KARAOKE_STYLE;
  }
}

let current = load();
const listeners = new Set<() => void>();

export function updateKaraokeStyle(patch: Partial<KaraokeStyle> | KaraokeStyle) {
  current = normalizeKaraokeStyle({ ...current, ...patch });
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(current));
  } catch {
    // 存不了就只在这次会话里生效
  }
  for (const listener of listeners) listener();
}

export function useKaraokeStyle(): KaraokeStyle {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => current,
    () => DEFAULT_KARAOKE_STYLE,
  );
}
