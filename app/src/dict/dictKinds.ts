/**
 * 词典管理的纯逻辑：按内容分类、在一类里面挪顺序。
 *
 * 分类只看导入时数出来的内容（`counts`），不猜书名：有词条的是释义词典，
 * 只有词频的是词频表，只有音调 / IPA 的是发音词典。一本词典同时带词条和词频时算释义。
 *
 * 顺序在库里是一条（`priority`），查词时每一类各自按它排。所以在某一类里拖动，
 * 只在这一类占的那几个位置之间换，**别的类的相对位置一点不动**。
 */

import type { DictionaryInfo } from "./api";
import { tr } from "../i18n";

export type DictKind = "terms" | "freq" | "pitch" | "other";

export const DICT_KINDS: { kind: DictKind; label: string; hint: string }[] = [
  { kind: "terms", label: tr("释义"), hint: tr("查词时释义按这个顺序往下排") },
  { kind: "freq", label: tr("词频"), hint: tr("词条旁边的词频标签按这个顺序排") },
  { kind: "pitch", label: tr("发音"), hint: tr("音调、IPA 按这个顺序显示") },
  { kind: "other", label: tr("其他"), hint: tr("没有词条、词频、音调的（只有标签之类）") },
];

export function dictKind(d: Pick<DictionaryInfo, "counts">): DictKind {
  if ((d.counts.terms ?? 0) > 0) return "terms";
  const meta = d.counts.termMeta ?? {};
  if ((meta.freq ?? 0) > 0) return "freq";
  if ((meta.pitch ?? 0) > 0 || (meta.ipa ?? 0) > 0) return "pitch";
  return "other";
}

/**
 * 把 `kind` 这一类里第 `from` 本挪到第 `to` 本（都是类内下标），返回新的整条顺序。
 * 其他类的书原地不动；下标越界或没动时原样返回同一个数组。
 */
export function reorderWithin<T extends Pick<DictionaryInfo, "counts">>(
  all: T[],
  kind: DictKind,
  from: number,
  to: number,
): T[] {
  const slots: number[] = [];
  all.forEach((d, i) => {
    if (dictKind(d) === kind) slots.push(i);
  });
  if (from === to || from < 0 || to < 0 || from >= slots.length || to >= slots.length) return all;
  const members = slots.map((i) => all[i] as T);
  const [moved] = members.splice(from, 1);
  members.splice(to, 0, moved as T);
  const next = [...all];
  slots.forEach((slot, k) => {
    next[slot] = members[k] as T;
  });
  return next;
}

/**
 * 拖动时落点：被拖那一行的中心越过了几行的中线，就落在第几个位置。
 * `rects` 是开始拖时量的每一行（上沿、高度），`dy` 是到现在挪了多少。
 */
export function dropIndex(rects: { top: number; height: number }[], from: number, dy: number): number {
  const self = rects[from];
  if (self === undefined) return from;
  const center = self.top + self.height / 2 + dy;
  let index = 0;
  rects.forEach((r, i) => {
    if (i !== from && r.top + r.height / 2 < center) index += 1;
  });
  return index;
}
