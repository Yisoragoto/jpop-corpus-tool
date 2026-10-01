/**
 * 分词校正编辑器里的纯逻辑：合并、拆分、清理。
 *
 * 规则和 Python 的分词校正对话框（dialogs/token_correction.py）一致：
 * - 合并：表层形拼起来，词元和词性取第一个词的
 * - 拆分：按字符位置切成两段，两段的词元就是各自的表层形，词性沿用原来的
 * - 清理：去首尾空白，丢掉空词，词元缺省用表层形，词性缺省 NOUN
 *
 * 拆分按 Unicode 码点数，不按 UTF-16 码元——Python 的 `surface[:pos]` 是码点，
 * 「𠮷野家」这种扩展区汉字在 JS 里占两个码元，按 `.length` 切会切出半个字。
 */

import type { TokenEdit } from "./api";

/** 选中的下标是否连成一段（合并只允许相邻的词）。 */
export function isContiguous(indices: number[]): boolean {
  if (indices.length < 2) return false;
  const sorted = [...indices].sort((a, b) => a - b);
  const first = sorted[0] ?? 0;
  const last = sorted[sorted.length - 1] ?? 0;
  return new Set(sorted).size === sorted.length && last - first + 1 === sorted.length;
}

/** 合并选中的相邻词。选中的不连续时原样返回。 */
export function mergeTokens(tokens: TokenEdit[], indices: number[]): TokenEdit[] {
  if (!isContiguous(indices)) return tokens;
  const sorted = [...indices].sort((a, b) => a - b);
  const start = sorted[0] ?? 0;
  const end = sorted[sorted.length - 1] ?? 0;
  const head = tokens[start];
  if (!head || end >= tokens.length) return tokens;
  const merged: TokenEdit = {
    surface: tokens.slice(start, end + 1).map((t) => t.surface).join(""),
    lemma: head.lemma,
    pos: head.pos,
  };
  return [...tokens.slice(0, start), merged, ...tokens.slice(end + 1)];
}

/** 字符（码点）数。 */
export function charCount(text: string): number {
  return Array.from(text).length;
}

/** 在第 `at` 个字符之后把一个词切成两个。位置越界时原样返回。 */
export function splitToken(tokens: TokenEdit[], index: number, at: number): TokenEdit[] {
  const token = tokens[index];
  if (!token) return tokens;
  const chars = Array.from(token.surface);
  if (at < 1 || at > chars.length - 1) return tokens;
  const left = chars.slice(0, at).join("");
  const right = chars.slice(at).join("");
  return [
    ...tokens.slice(0, index),
    { surface: left, lemma: left, pos: token.pos },
    { surface: right, lemma: right, pos: token.pos },
    ...tokens.slice(index + 1),
  ];
}

/** 保存前的清理。和后端 `corrections::normalize` 同一套规则。 */
export function cleanTokens(tokens: TokenEdit[]): TokenEdit[] {
  const out: TokenEdit[] = [];
  for (const t of tokens) {
    const surface = t.surface.trim();
    if (!surface) continue;
    out.push({
      surface,
      lemma: t.lemma.trim() || surface,
      pos: t.pos.trim() || "NOUN",
    });
  }
  return out;
}

export function sameTokens(a: TokenEdit[], b: TokenEdit[]): boolean {
  return (
    a.length === b.length &&
    a.every((t, i) => {
      const u = b[i];
      return !!u && t.surface === u.surface && t.lemma === u.lemma && t.pos === u.pos;
    })
  );
}

/**
 * 词拼回去和原句（去掉空白）对不对得上。对不上不拦着保存——
 * Python 版也允许直接改表层形——但要告诉用户：KWIC 的左右语境会退回按词拼接。
 */
export function matchesLine(tokens: TokenEdit[], text: string): boolean {
  return tokens.map((t) => t.surface).join("") === text.replace(/\s+/g, "");
}
