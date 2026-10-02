/**
 * 歌词字体：可选字体列表、按文件加载自带 / 导入的字体、拼 CSS 字体栈。
 *
 * 对应 Python 版曲库「显示」对话框的主字体、备用字体和「导入字体」。
 * 本机字体由后端问 DirectWrite（`fonts_catalog`）；**列进下拉框之前在 WebView2 里实际量一次**，
 * 只留真能用上的名字——实测 DirectWrite 给的 348 个名字里 Edge 认 295 个，不认的是图标字体和
 * 「Yu Gothic UI Semilight」这类 Chromium 匹配不上的字重名，选了也不会生效。
 */

import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

import { call } from "./api";

export interface InstalledFont {
  /** CSS 里用的名字（英文名） */
  name: string;
  /** 中文、日文名 */
  aliases: string[];
}

export interface FontFile {
  family: string;
  path: string;
  /** 项目自带（assets/fonts）还是用户导入的 */
  bundled: boolean;
}

export interface FontCatalog {
  installed: InstalledFont[];
  files: FontFile[];
  problems: string[];
}

export interface FontOption {
  /** 存进设置、写进 CSS 的名字 */
  value: string;
  label: string;
  file?: FontFile;
}

/** Python 版字体下拉框排在最前面的几个（装了才列） */
export const PREFERRED_FONTS = [
  "Klee One",
  "LXGW WenKai",
  "Microsoft YaHei UI",
  "Yu Gothic UI",
  "Meiryo UI",
  "Meiryo",
  "Segoe UI",
  "Noto Sans CJK SC",
  "Noto Sans CJK JP",
  "Source Han Sans SC",
  "Source Han Sans JP",
  "BIZ UDPGothic",
];

/** 不选字体时歌词用的字体，就是界面原来的字体 */
export const DEFAULT_FONT_STACK = '"Segoe UI", "Yu Gothic UI", "Microsoft YaHei UI", system-ui, sans-serif';

export function quoteFamily(name: string): string {
  return `"${name.replace(/["\\]/g, "")}"`;
}

/** 主字体、备用字体，再接界面默认字体兜底 */
export function lyricFontStack(primary: string, fallback: string): string {
  const chosen = [primary, fallback].map((n) => n.trim()).filter((n) => n !== "");
  return [...new Set(chosen)].map(quoteFamily).concat(DEFAULT_FONT_STACK).join(", ");
}

/**
 * 量一个字体名在这个 WebView 里有没有对应的字体：分别用等宽和衬线兜底，宽度一样说明两次都用上了它。
 * 在 Edge 里和后端列出的名字逐个对过（见 fonts.rs 的说明）。
 *
 * **样本串里不要放 CJK。** 本机 401 个字体名实测：样本带「あいう漢字」要 2,730ms，
 * 去掉之后 9ms——**判定结果一个不差**（348 个可用，逐名比对 0 处分歧）。
 * 慢的是 CJK 兜底字体的首次解析，每换一个 font stack 都要来一遍；第二遍跑只要 8ms，
 * 正好说明代价在首次加载。这 2.7 秒就卡在主线程上，用户点开「显示」面板时整个界面不动。
 *
 * 判定本身不依赖 CJK：拉丁部分的宽度差已经足够区分「用上了这个字体」和「兜底了」。
 * 真正一个拉丁字形都没有的字体两种样本都会被判成不可用，所以也没有变化。
 *
 * `document.fonts.check()` 看着更对口，但在 WebView2 里对**任何**不存在的字体名都返回 true
 * （实测 "NotAFontXYZ123"、"____" 全是 true），用不了。
 */
export function createFontProbe(): (family: string) => boolean {
  const ctx = document.createElement("canvas").getContext("2d");
  if (ctx === null) return () => true;
  const sample = "abcdefghijklmnopqrstuvwxyz ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789 WMmli";
  const width = (stack: string) => {
    ctx.font = `40px ${stack}`;
    return ctx.measureText(sample).width;
  };
  const mono = width("monospace");
  const serif = width("serif");
  return (family) => {
    const q = quoteFamily(family);
    const a = width(`${q}, monospace`);
    const b = width(`${q}, serif`);
    return a === b && (a !== mono || b !== serif);
  };
}

/** 下拉框选项：字体文件在前，然后是 Python 版常用的几个，再然后是全部本机字体 */
export function buildFontOptions(catalog: FontCatalog, usable: (family: string) => boolean): FontOption[] {
  const options: FontOption[] = [];
  const seen = new Set<string>();
  // 一个文件常带两种名字（「Klee One」和「Klee One SemiBold」），只列第一个
  const seenFiles = new Set<string>();
  for (const file of catalog.files) {
    if (seen.has(file.family) || seenFiles.has(file.path)) continue;
    seen.add(file.family);
    seenFiles.add(file.path);
    options.push({ value: file.family, label: `${file.family}（${file.bundled ? "自带" : "导入"}）`, file });
  }
  const installed: FontOption[] = [];
  for (const font of catalog.installed) {
    const names = [font.name, ...font.aliases];
    const value = names.find(usable);
    if (value === undefined || seen.has(value)) continue;
    seen.add(value);
    const others = names.filter((n) => n !== value && usable(n));
    installed.push({ value, label: others.length > 0 ? `${value}（${others.join(" / ")}）` : value });
  }
  const preferred = PREFERRED_FONTS.flatMap((name) => installed.filter((o) => o.value === name));
  return [...options, ...preferred, ...installed.filter((o) => !preferred.includes(o))];
}

const IMPORTED_KEY = "jp.lyrics.importedFonts";

export function loadImportedFonts(): string[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(IMPORTED_KEY) ?? "[]");
    return Array.isArray(parsed) ? parsed.filter((p): p is string => typeof p === "string") : [];
  } catch {
    return [];
  }
}

export function saveImportedFonts(paths: string[]) {
  try {
    localStorage.setItem(IMPORTED_KEY, JSON.stringify([...new Set(paths)]));
  } catch {
    // 存不了就只在这次会话里生效
  }
}

let catalogPromise: { key: string; promise: Promise<FontCatalog> } | null = null;

/* ── 探测结果的两级缓存 ──────────────────────────────────────────
 *
 * **探测很贵，而且贵在「这个字体第一次被用到」。** 本机 401 个字体名实测：
 * 冷着跑 1,779ms（125 个名字单个超过 5ms，最慢 41.8ms），跑完再来一遍只要 4ms。
 * 代价是字体引擎第一次解析每个字族，省不掉——只能别让它卡在主线程上，别每次都重来。
 *
 * 所以：① 结果按字体清单存进 localStorage，装了新字体才会重算；
 * ② 没有缓存时分片跑，每片之间把主线程还回去；
 * ③ 启动后空闲时就先跑，等用户点开「显示」或设置页时通常已经好了。
 */
const PROBE_KEY = "jp.lyrics.fontProbe";

/** 建好的选项表，这次会话里复用 */
let optionsCache: { key: string; options: FontOption[] } | null = null;
/** 正在跑的那一次，避免设置页和「显示」面板同时点开各跑一遍 */
let optionsPending: { key: string; promise: Promise<FontOption[]> } | null = null;

function allNames(catalog: FontCatalog): string[] {
  const names: string[] = [];
  for (const font of catalog.installed) names.push(font.name, ...font.aliases);
  return names;
}

/** 缓存键：字体清单本身。装了 / 卸了字体就会变，结果跟着重算。 */
function probeKey(catalog: FontCatalog, imported: string[]): string {
  return `${imported.join("|")}##${allNames(catalog).join("|")}`;
}

function readProbeCache(key: string): Set<string> | null {
  try {
    const raw = localStorage.getItem(PROBE_KEY);
    if (raw === null) return null;
    const parsed = JSON.parse(raw) as { key?: unknown; usable?: unknown };
    if (parsed.key !== key || !Array.isArray(parsed.usable)) return null;
    return new Set(parsed.usable.filter((v): v is string => typeof v === "string"));
  } catch {
    return null;
  }
}

function writeProbeCache(key: string, usable: Set<string>) {
  try {
    localStorage.setItem(PROBE_KEY, JSON.stringify({ key, usable: [...usable] }));
  } catch {
    // 存不了就只在这次会话里有效
  }
}

/** 分片探测。每片之后把主线程还回去——一次跑完会卡住近两秒。 */
async function probeInChunks(names: string[]): Promise<Set<string>> {
  // 单个名字平均 4.4ms，6 个一片约 26ms：掉一帧，不会像整段 1.8 秒那样「点了没反应」
  const CHUNK = 6;
  const probe = createFontProbe();
  const usable = new Set<string>();
  for (let i = 0; i < names.length; i += CHUNK) {
    for (const name of names.slice(i, i + CHUNK)) {
      if (probe(name)) usable.add(name);
    }
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  return usable;
}

/** 选项表。有缓存就直接给，没有就分片探测一遍再给。 */
export function fontOptions(catalog: FontCatalog, imported: string[]): Promise<FontOption[]> {
  const key = probeKey(catalog, imported);
  if (optionsCache?.key === key) return Promise.resolve(optionsCache.options);
  if (optionsPending?.key === key) return optionsPending.promise;

  const promise = (async () => {
    const cached = readProbeCache(key);
    const usable = cached ?? (await probeInChunks(allNames(catalog)));
    if (cached === null) writeProbeCache(key, usable);
    const options = buildFontOptions(catalog, (family) => usable.has(family));
    optionsCache = { key, options };
    return options;
  })();
  optionsPending = { key, promise };
  promise.finally(() => {
    if (optionsPending?.promise === promise) optionsPending = null;
  });
  return promise;
}

/**
 * 启动后空闲时先把选项表备好，省得用户点开「显示」面板时等。
 * 第一次装好的机器要跑将近两秒（分片，不卡界面），之后走 localStorage 缓存。
 */
export function warmFontOptions() {
  const run = () => {
    fetchFontCatalog(loadImportedFonts())
      .then((catalog) => fontOptions(catalog, loadImportedFonts()))
      .catch(() => undefined); // 预热失败无所谓，真用到的时候还会再试一次
  };
  const idle = (window as { requestIdleCallback?: (cb: () => void, opts?: { timeout: number }) => number })
    .requestIdleCallback;
  if (typeof idle === "function") idle(run, { timeout: 4000 });
  else setTimeout(run, 2500);
}

/** 字体目录。同一组导入文件只问一次后端 */
export function fetchFontCatalog(imported: string[]): Promise<FontCatalog> {
  const key = JSON.stringify(imported);
  if (catalogPromise?.key !== key) {
    const promise = call<FontCatalog>("fonts_catalog", { imported });
    catalogPromise = { key, promise };
    // 失败了下次重问
    promise.catch(() => {
      if (catalogPromise?.promise === promise) catalogPromise = null;
    });
  }
  return catalogPromise.promise;
}

const loadedFiles = new Map<string, Promise<void>>();

/** 按文件加载一个字体（同一个文件只加载一次） */
export function ensureFontFile(file: FontFile): Promise<void> {
  const key = `${file.family}\u0000${file.path}`;
  let loading = loadedFiles.get(key);
  if (loading === undefined) {
    const face = new FontFace(file.family, `url("${convertFileSrc(file.path)}")`);
    loading = face.load().then((loaded) => {
      document.fonts.add(loaded);
    });
    loading.catch(() => loadedFiles.delete(key));
    loadedFiles.set(key, loading);
  }
  return loading;
}

/** 选中的字体里有按文件加载的，就加载上 */
export function useLyricFontFiles(families: string[], onError: (message: string) => void) {
  const key = families.filter((f) => f !== "").join("\u0000");
  useEffect(() => {
    if (key === "") return;
    let alive = true;
    const wanted = key.split("\u0000");
    fetchFontCatalog(loadImportedFonts())
      .then((catalog) => Promise.all(catalog.files.filter((f) => wanted.includes(f.family)).map(ensureFontFile)))
      .catch((err: unknown) => {
        if (alive) onError(`字体加载失败：${err instanceof Error ? err.message : String((err as { message?: string })?.message ?? err)}`);
      });
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
}

/** 「显示」面板打开时取可选字体 */
export function useFontOptions(enabled: boolean, onError: (message: string) => void) {
  const [imported, setImported] = useState(loadImportedFonts);
  const [state, setState] = useState<{ options: FontOption[]; problems: string[] } | null>(null);

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    fetchFontCatalog(imported).then(
      async (catalog) => {
        const options = await fontOptions(catalog, imported);
        if (!alive) return;
        setState({ options, problems: catalog.problems });
      },
      (err: unknown) => {
        if (alive) onError(`列不出字体：${err instanceof Error ? err.message : String((err as { message?: string })?.message ?? err)}`);
      },
    );
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, imported]);

  const addImported = (paths: string[]) => {
    const next = [...new Set([...imported, ...paths])];
    saveImportedFonts(next);
    setImported(next);
  };
  const removeImported = (path: string) => {
    const next = imported.filter((p) => p !== path);
    saveImportedFonts(next);
    setImported(next);
  };
  return { options: state?.options ?? null, problems: state?.problems ?? [], imported, addImported, removeImported };
}
