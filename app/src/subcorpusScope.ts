/**
 * 「当前子语料」：KWIC、词云、单词例句共用的那一块语料。
 *
 * 统计页有自己的一整套筛选（`ScopeBar`）；检索、词云、例句这些「找」的地方只需要
 * 选一个**存过的子语料**（或者整个库）。选了哪个记在本机（localStorage），三处一起变，
 * 下次打开还在。只用子语料的范围条件，「仅日文」「副歌只算一遍」这些口径各处有自己的开关。
 */

import { useEffect, useState, useSyncExternalStore } from "react";

import type { StatsFilter, Subcorpus } from "./api";
import { qualityApi, type SavedSubcorpus } from "./corpusQuality";

export interface ActiveScope {
  id: number;
  name: string;
  filter: StatsFilter;
}

const KEY = "jpop.activeSubcorpus";
const listeners = new Set<() => void>();

function read(): ActiveScope | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const v = JSON.parse(raw) as ActiveScope;
    return v && typeof v.id === "number" && v.filter ? v : null;
  } catch {
    return null;
  }
}

let current: ActiveScope | null = read();

export function getActiveScope(): ActiveScope | null {
  return current;
}

export function setActiveScope(scope: ActiveScope | null) {
  current = scope;
  try {
    if (scope) localStorage.setItem(KEY, JSON.stringify(scope));
    else localStorage.removeItem(KEY);
  } catch {
    // 存不下（隐私模式）也照样在这次会话里生效
  }
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useActiveScope(): ActiveScope | null {
  return useSyncExternalStore(subscribe, getActiveScope, getActiveScope);
}

/** 存过的子语料列表；当前选的那个被删了（或改了条件）时跟着更新 */
export function useSubcorpora(onError: (message: string) => void): SavedSubcorpus[] {
  const [list, setList] = useState<SavedSubcorpus[]>([]);
  useEffect(() => {
    qualityApi
      .subcorpora()
      .then((all) => {
        setList(all);
        const active = getActiveScope();
        if (!active) return;
        const fresh = all.find((s) => s.id === active.id);
        if (!fresh) setActiveScope(null);
        else if (JSON.stringify(fresh.filter) !== JSON.stringify(active.filter) || fresh.name !== active.name)
          setActiveScope({ id: fresh.id, name: fresh.name, filter: fresh.filter });
      })
      .catch((e) => onError(e instanceof Error ? e.message : String(e)));
  }, [onError]);
  return list;
}

/** 把当前子语料的范围并进一份统计筛选（词云用）。演唱者取两边的并集之外的交集没有意义，以调用方选的为准 */
export function withScope(filter: StatsFilter, scope: ActiveScope | null): StatsFilter {
  if (!scope) return filter;
  const s = scope.filter;
  return {
    ...s,
    performerIds: filter.performerIds.length ? filter.performerIds : s.performerIds,
    jpOnly: filter.jpOnly,
    distinctLines: !!filter.distinctLines,
  };
}

/** 一页自己的条件（歌手、年份……）叠在当前子语料上：这页填了的以这页为准，没填的沿用子语料的 */
export function mergeSubcorpus(local: Subcorpus, scope: ActiveScope | null): Subcorpus {
  if (!scope) return local;
  const s = scope.filter;
  const pick = <T,>(mine: T[] | undefined, theirs: T[] | undefined) => (mine && mine.length ? mine : theirs ?? []);
  return {
    performerIds: pick(local.performerIds, s.performerIds),
    yearFrom: local.yearFrom ?? s.yearFrom ?? null,
    yearTo: local.yearTo ?? s.yearTo ?? null,
    albumIds: pick(local.albumIds, s.albumIds),
    genres: pick(local.genres, s.genres),
    songIds: pick(local.songIds, s.songIds),
    languages: pick(local.languages, s.languages),
    minCompleteness: local.minCompleteness ?? s.minCompleteness ?? null,
  };
}
