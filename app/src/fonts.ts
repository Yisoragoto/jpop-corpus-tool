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
 */
export function createFontProbe(): (family: string) => boolean {
  const ctx = document.createElement("canvas").getContext("2d");
  if (ctx === null) return () => true;
  const sample = "abcdefghijklmnopqrstuvwxyz ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789 WMmli あいう漢字";
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
      (catalog) => {
        if (!alive) return;
        setState({ options: buildFontOptions(catalog, createFontProbe()), problems: catalog.problems });
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
