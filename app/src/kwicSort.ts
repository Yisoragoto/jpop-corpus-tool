/**
 * KWIC 排序：按关键词左右第几个词排（AntConc 的 1L / 2L / 1R …）。
 *
 * 左右语境的词由后端给（`leftWords` / `rightWords`，由近到远，不含标点）。
 * 选了「左 2」就先比左边第 2 个词；一样的话接着往外比（左 3、左 4…），再比里面的（左 1），
 * 再比关键词本身，最后比另一侧。都一样就保持原来的顺序（数组排序是稳定的）。
 * 行首、行尾没有词的排在最前。比较用日文的排序规则（假名按五十音）。
 */

import type { KwicHit } from "./api";
import { tr } from "./i18n";

export type KwicSort = "default" | "L3" | "L2" | "L1" | "key" | "R1" | "R2" | "R3";

export const KWIC_SORTS: { value: KwicSort; label: string }[] = [
  { value: "default", label: tr("原顺序（歌手、曲名）") },
  { value: "L3", label: tr("左 3") },
  { value: "L2", label: tr("左 2") },
  { value: "L1", label: tr("左 1") },
  { value: "key", label: tr("关键词") },
  { value: "R1", label: tr("右 1") },
  { value: "R2", label: tr("右 2") },
  { value: "R3", label: tr("右 3") },
];

/** 一侧从第 n 个词（从 1 数）开始：先往外，再往里 */
function fromPosition(words: string[], n: number, depth = 5): string[] {
  const at = (i: number) => words[i] ?? "";
  const out: string[] = [];
  for (let i = n - 1; i < depth; i += 1) out.push(at(i));
  for (let i = n - 2; i >= 0; i -= 1) out.push(at(i));
  return out;
}

/** 排序键：一串依次比较的词 */
export function sortKeys(hit: KwicHit, sort: KwicSort): string[] {
  const left = hit.leftWords ?? [];
  const right = hit.rightWords ?? [];
  switch (sort) {
    case "L1":
    case "L2":
    case "L3":
      return [...fromPosition(left, Number(sort[1])), hit.keyword, ...fromPosition(right, 1)];
    case "R1":
    case "R2":
    case "R3":
      return [...fromPosition(right, Number(sort[1])), hit.keyword, ...fromPosition(left, 1)];
    case "key":
      return [hit.keyword, ...fromPosition(right, 1), ...fromPosition(left, 1)];
    default:
      return [];
  }
}

const collator = new Intl.Collator("ja");

export function sortKwic(hits: KwicHit[], sort: KwicSort): KwicHit[] {
  if (sort === "default") return hits;
  const keyed = hits.map((hit) => ({ hit, keys: sortKeys(hit, sort) }));
  keyed.sort((a, b) => {
    for (let i = 0; i < a.keys.length; i += 1) {
      const c = collator.compare(a.keys[i]!, b.keys[i]!);
      if (c !== 0) return c;
    }
    return 0;
  });
  return keyed.map((k) => k.hit);
}
