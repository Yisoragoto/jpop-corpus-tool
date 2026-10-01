/**
 * 哪些词典默认折叠。点词典名折叠一次，之后所有查词结果里这本都保持折叠（存 localStorage）。
 *
 * 大辞泉、明鏡这类释义很长的词典，常常只想看一眼中日双解；每次都手动收起太烦，所以记住。
 */

import { useSyncExternalStore } from "react";

const STORAGE_KEY = "jp.dict.collapsedDictionaries";

function load(): Set<string> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const parsed: unknown = raw === null ? [] : JSON.parse(raw);
    return new Set(Array.isArray(parsed) ? parsed.filter((x): x is string => typeof x === "string") : []);
  } catch {
    return new Set();
  }
}

let collapsed: Set<string> = load();
const listeners = new Set<() => void>();

function commit(next: Set<string>) {
  collapsed = next;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify([...next]));
  } catch {
    // 存不了就只在这次会话里生效
  }
  for (const listener of listeners) listener();
}

export function setDictionaryCollapsed(title: string, value: boolean) {
  setDictionariesCollapsed([title], value);
}

export function setDictionariesCollapsed(titles: string[], value: boolean) {
  const next = new Set(collapsed);
  for (const title of titles) {
    if (value) next.add(title);
    else next.delete(title);
  }
  commit(next);
}

export function collapsedDictionaries(): Set<string> {
  return collapsed;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useCollapsedDictionaries(): Set<string> {
  return useSyncExternalStore(subscribe, collapsedDictionaries, collapsedDictionaries);
}
