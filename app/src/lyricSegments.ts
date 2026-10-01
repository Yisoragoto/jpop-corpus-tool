/**
 * 把一行歌词拆成「词之间的原文」和「词」，词里再按振假名拆成带注音 / 不带注音的几段。
 *
 * 词来自库里的分词（点了能查词），振假名来自现场分词；两者都按原文的字符位置对齐。
 * 全部 8443 行实测：分词都能对齐回原文，注音段没有一段横跨两个词。万一对不上（比如以后改过歌词没重新分词），
 * 返回 null，调用方退回只显示词、不注音，不猜位置。
 *
 * 位置按 Unicode 字符（码点）算，和后端一致；JS 字符串下标是 UTF-16，所以先 `Array.from`。
 */

import type { LineToken } from "./api";
import type { Ruby } from "./lyricsDisplay";

export interface Piece {
  text: string;
  /** 没有就是不用注音 */
  reading?: string;
}

export type Segment = { kind: "gap"; text: string } | { kind: "token"; index: number; pieces: Piece[] };

/** 每个词在原文里的 [起, 止)；词与词之间只允许夹空白 */
export function alignTokens(chars: string[], tokens: LineToken[]): [number, number][] | null {
  const spans: [number, number][] = [];
  let pos = 0;
  for (const token of tokens) {
    const surface = Array.from(token.surface);
    if (surface.length === 0) {
      spans.push([pos, pos]);
      continue;
    }
    let found = -1;
    for (let p = pos; p + surface.length <= chars.length; p += 1) {
      if (surface.every((ch, i) => chars[p + i] === ch)) {
        found = p;
        break;
      }
      if (!/\s/u.test(chars[p] ?? "")) return null;
    }
    if (found < 0) return null;
    spans.push([found, found + surface.length]);
    pos = found + surface.length;
  }
  return spans;
}

export function segmentLine(text: string, tokens: LineToken[], rubies: Ruby[]): Segment[] | null {
  const chars = Array.from(text);
  const spans = alignTokens(chars, tokens);
  if (spans === null) return null;
  if (rubies.some((r) => !spans.some(([a, b]) => a <= r.start && r.end <= b && r.start < r.end))) return null;

  const segments: Segment[] = [];
  let cursor = 0;
  spans.forEach(([start, end], index) => {
    if (start > cursor) segments.push({ kind: "gap", text: chars.slice(cursor, start).join("") });
    const pieces: Piece[] = [];
    let at = start;
    for (const ruby of rubies.filter((r) => start <= r.start && r.end <= end).sort((a, b) => a.start - b.start)) {
      if (ruby.start > at) pieces.push({ text: chars.slice(at, ruby.start).join("") });
      pieces.push({ text: chars.slice(ruby.start, ruby.end).join(""), reading: ruby.reading });
      at = ruby.end;
    }
    if (end > at) pieces.push({ text: chars.slice(at, end).join("") });
    segments.push({ kind: "token", index, pieces });
    cursor = Math.max(cursor, end);
  });
  if (cursor < chars.length) segments.push({ kind: "gap", text: chars.slice(cursor).join("") });
  return segments;
}
