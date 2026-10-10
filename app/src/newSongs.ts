/**
 * 启动时查新歌：横幅上说什么、给不给「直接导入」，以及用户说过「别再提」的那些。
 *
 * 去哪些文件夹找是后端推出来的（`jp_import::watch`），这里不管；这里管两件事：
 *
 * 1. **哪些新歌可以不经复核直接导。** 导入页的规矩是「先看计划再写库」，横幅上的「导入」
 *    是给这条规矩开的一个口子，所以只在每一首都说得清自己是谁的时候才开：标签读得出、
 *    歌手是文件自己写的（tag 或「歌手 - 曲名」的文件名），而且不是一下子一大批。
 *    别的情况横幅只给「查看」，进导入页走平常的流程。
 * 2. **「别再提」的清单。** 路径是这台机器上的事，存 localStorage，每次查的时候传给后端。
 *    删了歌没删文件、或者一个文件夹里本来就有不想导的东西，没有这份清单就每次启动都提一遍。
 */

import { useSyncExternalStore } from "react";

import type { ImportReport, NewAudio, PlanItem, ScanResult, WatchedFolder } from "./api";

/** 超过这么多首就不给直接导：一眼看不完的东西不该一键写库 */
export const DIRECT_IMPORT_LIMIT = 20;

/** 计划里会真的写进库的那些 */
export function newItems(scan: ScanResult): PlanItem[] {
  return scan.items.filter((item) => item.action.kind === "new");
}

/** 这批新歌为什么要先看一眼；能直接导返回 null */
export function reviewReason(items: PlanItem[]): string | null {
  if (items.length > DIRECT_IMPORT_LIMIT) return `一次 ${items.length} 首，先过一眼计划`;
  const unreadable = items.filter((item) => item.warning !== null).length;
  if (unreadable > 0) return `${unreadable} 首读不出标签，曲名歌手是猜的`;
  const guessed = items.filter((item) => item.artistSource === "folder").length;
  if (guessed > 0) return `${guessed} 首的歌手是从文件夹名猜的`;
  const anonymous = items.filter((item) => item.artistSource === "none" || item.artist.trim() === "").length;
  if (anonymous > 0) return `${anonymous} 首定不出歌手`;
  return null;
}

/** 「曲名 — 歌手、曲名 — 歌手 等 5 首」。列不完的只说总数，全的在悬停提示和导入页里 */
export function describeNewSongs(items: PlanItem[], max = 3): string {
  const named = items.slice(0, max).map((item) => (item.artist.trim() === "" ? item.title : `${item.title} — ${item.artist}`));
  return items.length > max ? `${named.join("、")} 等 ${items.length} 首` : named.join("、");
}

/** 路径的比较键，和后端 `path_key` 一样：分隔符统一，Windows 的路径不分大小写 */
export function pathKey(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

/** 这个文件是在哪个文件夹里找到的：包含它的里面最贴近的那个 */
export function folderOf(folders: WatchedFolder[], path: string): WatchedFolder | undefined {
  const key = pathKey(path);
  return folders
    .filter((folder) => key.startsWith(`${pathKey(folder.path)}/`))
    .sort((a, b) => pathKey(b.path).length - pathKey(a.path).length)[0];
}

/** 这批新歌分布在哪些文件夹里，按首数从多到少 */
export function foldersOf(found: NewAudio, items: PlanItem[]): string[] {
  const counts = new Map<string, number>();
  for (const item of items) {
    const folder = folderOf(found.folders, item.path)?.path ?? "";
    if (folder !== "") counts.set(folder, (counts.get(folder) ?? 0) + 1);
  }
  return [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([folder]) => folder);
}

export interface ImportOutcomeSummary {
  imported: number;
  /** 导进来的歌分到了哪些歌手名下（按计划里的歌手，去重，保持出现顺序） */
  artists: string[];
  /** 导进来但一行歌词都没有的，songId */
  withoutLyrics: string[];
  /** 没导进去的：曲名（没有就文件路径）和原因 */
  failed: { title: string; error: string }[];
  cancelled: boolean;
}

/** 导完之后横幅上要说的：导了几首、归到谁名下、哪些还缺歌词、哪些没成 */
export function summarizeImport(report: ImportReport, planned: PlanItem[]): ImportOutcomeSummary {
  const artistOf = new Map(planned.map((item) => [pathKey(item.path), item.artist.trim()]));
  const summary: ImportOutcomeSummary = { imported: 0, artists: [], withoutLyrics: [], failed: [], cancelled: report.cancelled };
  for (const track of report.tracks) {
    if (track.outcome.kind === "failed") {
      summary.failed.push({ title: track.title || track.path, error: track.outcome.error });
      continue;
    }
    summary.imported += 1;
    if (track.outcome.lyricLines === 0) summary.withoutLyrics.push(track.songId);
    const artist = artistOf.get(pathKey(track.path)) ?? "";
    if (artist !== "" && !summary.artists.includes(artist)) summary.artists.push(artist);
  }
  return summary;
}

// ────────────────────────────── 「别再提」的清单 ──────────────────────────────

export interface NewSongIgnores {
  /** 别再看的文件夹 */
  folders: string[];
  /** 别再提的文件 */
  files: string[];
}

export const NO_IGNORES: NewSongIgnores = { folders: [], files: [] };

const STORAGE_KEY = "jp.newSongs.ignored";

function strings(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string" && v !== "") : [];
}

/** 读回来的可能被手改过：不是字符串数组的一律当空 */
export function parseIgnores(raw: string | null): NewSongIgnores {
  if (raw === null) return NO_IGNORES;
  try {
    const parsed = JSON.parse(raw) as Partial<Record<keyof NewSongIgnores, unknown>> | null;
    return { folders: strings(parsed?.folders), files: strings(parsed?.files) };
  } catch {
    return NO_IGNORES;
  }
}

function union(current: string[], added: string[]): string[] {
  const seen = new Set(current.map(pathKey));
  const out = [...current];
  for (const path of added) {
    const key = pathKey(path);
    if (!seen.has(key)) {
      seen.add(key);
      out.push(path);
    }
  }
  return out;
}

/** 加进清单。同一个路径换个大小写、换个分隔符再加一次不会多出一条 */
export function withIgnored(current: NewSongIgnores, added: Partial<NewSongIgnores>): NewSongIgnores {
  return { folders: union(current.folders, added.folders ?? []), files: union(current.files, added.files ?? []) };
}

/** 把一个文件夹从清单里拿掉（恢复检查） */
export function withoutIgnoredFolder(current: NewSongIgnores, folder: string): NewSongIgnores {
  const key = pathKey(folder);
  return { ...current, folders: current.folders.filter((f) => pathKey(f) !== key) };
}

function load(): NewSongIgnores {
  try {
    return parseIgnores(localStorage.getItem(STORAGE_KEY));
  } catch {
    return NO_IGNORES;
  }
}

let ignores = load();
const listeners = new Set<() => void>();

export function currentIgnores(): NewSongIgnores {
  return ignores;
}

export function setIgnores(next: NewSongIgnores) {
  ignores = next;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(ignores));
  } catch {
    // 存不了就只在这次会话里生效
  }
  for (const listener of listeners) listener();
}

export function useNewSongIgnores(): NewSongIgnores {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => ignores,
    () => NO_IGNORES,
  );
}
