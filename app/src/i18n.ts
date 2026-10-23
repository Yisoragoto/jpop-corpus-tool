/**
 * 界面语言：中文 / English。
 *
 * 写法和 gettext 一样：**中文原文就是键**。代码里照旧写中文，外面套一层 `tr()`：
 *
 *     tr("设置")                              → Settings
 *     tr("{count} 首歌", { count: 3 })        → 3 songs（英文写成 "{count} {count|song|songs}"）
 *
 * 英文在 `locales/en*.ts`，键是中文原文（占位符 `{name}` 原样），值是英文字符串；
 * 单复数写成 `{n|song|songs}`（见 `interpolate`），实在不行也可以给一个函数。查不到就退回中文——`i18n.test.ts` 会扫一遍源码，
 * 保证每一处中文字面量都有英文。
 *
 * 语言存在 localStorage（和其它设置一样同步读），**模块一加载就定下来**；所以模块顶层的常量
 * 也可以直接 `tr()`。换语言时整页重新加载（`setLanguage`），不需要每个组件订阅。
 */

import { backendEn } from "./locales/backend";
import { en } from "./locales/en";

export type Lang = "zh" | "en";
export type Vars = Record<string, string | number | boolean | null | undefined>;
export type Entry = string | ((vars: Vars) => string);

export const LANGUAGES: { id: Lang; label: string }[] = [
  { id: "zh", label: "中文" },
  { id: "en", label: "English" },
];

const STORAGE_KEY = "jp.app.lang";
/** 老用户（升级上来、已经有设置）不弹选语言：一直是中文 */
const SETTINGS_KEY = "jp.app.settings";

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** 系统语言是中文就默认中文，其它一律英文 */
export function systemLanguage(): Lang {
  const langs = typeof navigator === "undefined" ? [] : [...(navigator.languages ?? []), navigator.language];
  return langs.some((l) => typeof l === "string" && l.toLowerCase().startsWith("zh")) ? "zh" : "en";
}

function initial(): Lang {
  // 测试（node 里没有 window）：一律中文，现有的断言都按中文写
  if (typeof window === "undefined") return "zh";
  const saved = read(STORAGE_KEY);
  if (saved === "zh" || saved === "en") return saved;
  // 没选过：老用户是中文，新装的先按系统语言，首次启动时再让人选（见 `needsLanguageChoice`）
  return read(SETTINGS_KEY) !== null ? "zh" : systemLanguage();
}

let current: Lang = initial();

export function language(): Lang {
  return current;
}

/** 第一次启动（从没选过语言、也没有旧设置）：要弹选语言 */
export function needsLanguageChoice(): boolean {
  const saved = read(STORAGE_KEY);
  return saved !== "zh" && saved !== "en" && read(SETTINGS_KEY) === null;
}

/** 记下语言。`reload` 时整页重新加载，让模块顶层的文字也换过来 */
export function setLanguage(lang: Lang, reload = true) {
  try {
    localStorage.setItem(STORAGE_KEY, lang);
  } catch {
    // 存不了就只在这次会话里生效
  }
  const changed = lang !== current;
  current = lang;
  document.documentElement.lang = lang === "zh" ? "zh-CN" : "en";
  if (reload && changed) window.location.reload();
}

/**
 * 填占位符。`{name}` 换成值；`{name|one|many}` 按 name 的值是不是 1 挑单复数
 * （英文用：`"{n} {n|song|songs}"`）。值是 "1,234" 这种带千分位的也认。
 */
export function interpolate(text: string, vars?: Vars): string {
  if (!vars) return text;
  return text
    .replace(/\{(\w+)\|([^|{}]*)\|([^|{}]*)\}/g, (m, k: string, one: string, many: string) =>
      k in vars ? (Number(String(vars[k]).replace(/,/g, "")) === 1 ? one : many) : m,
    )
    .replace(/\{(\w+)\}/g, (m, k: string) => (k in vars ? String(vars[k]) : m));
}

// ─────────────────────────── 后端发来的中文 ───────────────────────────

/**
 * Rust 源码里一句中文字面量 → backend.ts 里的键：格式串的占位符（`{}`、`{name}`、`{:.1}`、`{x:?}`）
 * 按出现顺序换成 `{0}`、`{1}`……；`{{` `}}` 是字面的花括号，原样留着。
 */
export function backendKey(rust: string): string {
  let n = 0;
  return rust.replace(/\{\{|\}\}|\{[^{}]*\}/g, (m) => (m === "{{" || m === "}}" ? m : `{${n++}}`));
}

interface BackendPattern {
  re: RegExp;
  en: string;
}
let patterns: BackendPattern[] | null = null;

function compile(): BackendPattern[] {
  const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return Object.entries(backendEn)
    .filter(([zh]) => /\{\d+\}/.test(zh))
    .map(([zh, en]) => {
      const parts = zh.split(/(\{\d+\})/);
      const body = parts
        .map((p) => (/^\{\d+\}$/.test(p) ? "([\\s\\S]*?)" : escape(p.replace(/\{\{/g, "{").replace(/\}\}/g, "}"))))
        .join("");
      return { re: new RegExp(`^${body}$`), en, literal: zh.replace(/\{\d+\}/g, "").length };
    })
    // 字面部分越长越具体，先试
    .sort((a, b) => b.literal - a.literal)
    .map(({ re, en }) => ({ re, en }));
}

function translateOne(text: string, depth: number): string | null {
  const exact = backendEn[text];
  if (exact !== undefined) return exact;
  patterns ??= compile();
  for (const { re, en } of patterns) {
    const m = re.exec(text);
    if (!m) continue;
    const vars: Vars = {};
    // 占位符里塞的常常是另一句中文（「启动失败：{}」），再翻一层
    m.slice(1).forEach((v, i) => (vars[String(i)] = depth < 3 && HAN.test(v) ? translate(v, depth + 1) : v));
    return interpolate(en, vars);
  }
  return null;
}

function translate(text: string, depth: number): string {
  const whole = translateOne(text, depth);
  if (whole !== null) return whole;
  // anyhow 的 `{:#}` 把一条错误链用 ": " 连起来：一段一段换，认不出的（多半是英文原因）照搬
  if (text.includes(": ")) return text.split(": ").map((part) => translateOne(part, depth) ?? part).join(": ");
  return text;
}

const HAN = /\p{Script=Han}/u;

/** 后端发来的话（报错、命令返回的提示句、进度消息）：英文界面下按 Rust 源码里的模板换成英文 */
export function trBackend(text: string): string {
  if (current !== "en" || !HAN.test(text)) return text;
  return translate(text, 0);
}

/**
 * 命令结果和事件里由后端写的那几个字段（报错、原因、提示、范围说明……）：英文界面下逐个换。
 * 只认这些字段名——歌名、歌词、路径这类数据原样不动。
 */
const BACKEND_TEXT_FIELDS = new Set([
  "message",
  "errorMessage",
  "error",
  "errors",
  "warning",
  "warnings",
  "reason",
  "reasons",
  "tokenizeError",
  "openError",
  "explain",
  "label",
  "note",
  "detail",
  // 分析结果的范围说明（「全部歌曲」「演唱：X」）
  "scope",
]);

function trValue(value: unknown): unknown {
  if (typeof value === "string") return trBackend(value);
  if (Array.isArray(value)) return value.map(trValue);
  return value;
}

/** 后端发来的结构：顶层是字符串就整句换；是对象 / 数组就只换上面那几个字段（往下找，最多 4 层） */
export function trPayload<T>(payload: T, depth = 0): T {
  if (current !== "en" || payload === null || payload === undefined) return payload;
  if (typeof payload === "string") return trBackend(payload) as T;
  if (depth > 4 || typeof payload !== "object") return payload;
  if (Array.isArray(payload)) {
    // 每个对象都看一遍；没改的对象原样返回。元素不是对象（数字、字符串列表）的整个跳过
    const first = payload[0] as unknown;
    if (first === null || typeof first !== "object") return payload;
    return payload.map((item) => trPayload(item, depth + 1)) as T;
  }
  let changed = false;
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(payload as Record<string, unknown>)) {
    const next = BACKEND_TEXT_FIELDS.has(k) ? trValue(v) : v !== null && typeof v === "object" ? trPayload(v, depth + 1) : v;
    if (next !== v) changed = true;
    out[k] = next;
  }
  return changed ? (out as T) : payload;
}

/** 只给测试用：切语言但不碰 localStorage、不重新加载 */
export function setLanguageForTest(lang: Lang) {
  current = lang;
}

/** 翻译。`zh` 是中文原文（也是键） */
export function tr(zh: string, vars?: Vars): string {
  if (current === "en") {
    const e = en[zh];
    if (typeof e === "function") return e(vars ?? {});
    if (typeof e === "string") return interpolate(e, vars);
  }
  return interpolate(zh, vars);
}

