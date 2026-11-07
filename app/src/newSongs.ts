/**
 * 启动时查新歌：哪些算新歌，以及用户说过「别再提」的那些。
 *
 * 去哪些文件夹找是后端推出来的（`jp_import::watch`），这里不管。
 * 「别再提」的清单：路径是这台机器上的事，存 localStorage，每次查的时候传给后端。
 * 删了歌没删文件、或者一个文件夹里本来就有不想导的东西，没有这份清单就每次启动都提一遍。
 */

import { useSyncExternalStore } from "react";

import type { PlanItem, ScanResult } from "./api";

/** 计划里会真的写进库的那些 */
export function newItems(scan: ScanResult): PlanItem[] {
  return scan.items.filter((item) => item.action.kind === "new");
}

/** 路径的比较键，和后端 `path_key` 一样：分隔符统一，Windows 的路径不分大小写 */
export function pathKey(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
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
