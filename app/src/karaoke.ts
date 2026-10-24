/**
 * 卡拉 OK 模式与打轴的纯逻辑（不碰 DOM、不碰后端，能单测）。
 *
 * 打轴的数据就是每行若干个「时间单位」：这一行第 `[charStart, charEnd)` 个字（Unicode 字符下标，
 * 和后端一致）从 `startSec` 唱到 `endSec`。存回去时写成增强 LRC，播放器按原来的规则验证。
 *
 * - 已有的真实单位原样拿来当初始值（来源给的逐字时间、带起止的手动校准），不改切分；
 * - 没有时间的字按「拍」切开：一个字一个单位，拗音小字（ゃゅょ…）并进前一个字，
 *   空白和标点跟着前一个字（来源也不给它们时间），连在一起的拉丁字母、数字算一个词；
 * - 打轴只填用户按下去的时间，不按字数均摊、不补造没打的时间。
 */

import type { LyricLine } from "./api";
import type { WordTiming } from "./wordTiming";

export interface EditUnit {
  charStart: number;
  charEnd: number;
  startSec: number | null;
  endSec: number | null;
}

export interface EditLine {
  text: string;
  /** 原来的行首时间；有打过的单位时，存的时候取第一个单位的开始 */
  timeSec: number | null;
  units: EditUnit[];
}

export interface Cursor {
  line: number;
  unit: number;
}

/** 两个单位之间、一个单位的起止之间至少留这么多（秒），免得一下按出零长度 */
export const MIN_UNIT_SEC = 0.03;

/** 不唱的分隔：空白和标点。和后端 `word_timing::untimed_separator` 同一张表 */
const SEPARATORS = new Set([..."、。！？「」『』（）［］【】〈〉《》〔〕…‥—―〜～・：；，．“”‘’"]);
export function isSeparator(ch: string): boolean {
  return /\s/u.test(ch) || /^[!-/:-@[-`{-~]$/.test(ch) || SEPARATORS.has(ch);
}

/** 拗音、促音以外的小字：跟前一个字合成一拍 */
const SMALL_KANA = new Set([..."ゃゅょぁぃぅぇぉゎゕゖャュョァィゥェォヮヵヶ"]);
const WORDY = /^[\p{Script=Latin}\p{N}'’]$/u;

/** 没有时间的一段文字按拍切开。`offset` 是这段在整行里的起点 */
export function defaultUnits(text: string, offset = 0): EditUnit[] {
  const chars = [...text];
  const units: EditUnit[] = [];
  let pending = 0; // 行首的分隔符：并进第一个真正的字
  for (let i = 0; i < chars.length; i += 1) {
    const ch = chars[i]!;
    const last = units[units.length - 1];
    const prev = chars[i - 1] ?? "";
    const joins = last !== undefined && last.charEnd === offset + i
      && (isSeparator(ch) || SMALL_KANA.has(ch) || (WORDY.test(ch) && WORDY.test(prev)));
    if (joins) {
      last.charEnd += 1;
    } else if (isSeparator(ch) && last === undefined) {
      pending += 1;
    } else {
      units.push({ charStart: offset + i - pending, charEnd: offset + i + 1, startSec: null, endSec: null });
      pending = 0;
    }
  }
  // 整段都是分隔符：作为一个单位留着，免得这几个字没有归属
  if (units.length === 0 && chars.length > 0) units.push({ charStart: offset, charEnd: offset + chars.length, startSec: null, endSec: null });
  return units;
}

/**
 * 一行的初始单位：已有的真实单位（`lrc`，以及带起止、和来源同范围时优先的 `manual`）按位置排好，
 * 和前面重叠的丢掉；没被覆盖的字按拍切开、不带时间。
 */
export function initialUnits(text: string, timings: readonly WordTiming[]): EditUnit[] {
  const length = [...text].length;
  const byRange = new Map<string, WordTiming>();
  for (const t of timings) {
    if (t.source !== "lrc" && !(t.source === "manual" && t.endSec !== null)) continue;
    if (!(Number.isInteger(t.charStart) && Number.isInteger(t.charEnd) && t.charStart >= 0 && t.charStart < t.charEnd && t.charEnd <= length)) continue;
    const key = `${t.charStart}:${t.charEnd}`;
    const previous = byRange.get(key);
    if (previous === undefined || t.source === "manual") byRange.set(key, t);
  }
  const known = [...byRange.values()].sort((a, b) => a.charStart - b.charStart || a.charEnd - b.charEnd);
  const chars = [...text];
  const units: EditUnit[] = [];
  let cursor = 0;
  for (const t of known) {
    if (t.charStart < cursor) continue;
    if (t.charStart > cursor) units.push(...defaultUnits(chars.slice(cursor, t.charStart).join(""), cursor));
    const end = t.endSec !== null && t.endSec > t.startSec ? t.endSec : null;
    units.push({ charStart: t.charStart, charEnd: t.charEnd, startSec: t.startSec, endSec: end });
    cursor = t.charEnd;
  }
  if (cursor < length) units.push(...defaultUnits(chars.slice(cursor).join(""), cursor));
  return units;
}

export function editLinesFrom(lyrics: readonly LyricLine[], timings: readonly WordTiming[]): EditLine[] {
  const byLine = new Map<number, WordTiming[]>();
  for (const t of timings) {
    const list = byLine.get(t.utteranceId) ?? [];
    list.push(t);
    byLine.set(t.utteranceId, list);
  }
  return lyrics.map((line) => ({ text: line.text, timeSec: line.timeSec, units: initialUnits(line.text, byLine.get(line.utteranceId) ?? []) }));
}

export function unitText(line: EditLine, unit: EditUnit): string {
  return [...line.text].slice(unit.charStart, unit.charEnd).join("");
}

function cloneLines(lines: readonly EditLine[], index: number): EditLine[] {
  const copy = lines.slice();
  const line = copy[index]!;
  copy[index] = { ...line, units: line.units.map((u) => ({ ...u })) };
  return copy;
}

/** 下一个单位；行尾跳到下一行第一个。到头了返回 null */
export function nextCursor(lines: readonly EditLine[], at: Cursor): Cursor | null {
  if (at.unit + 1 < (lines[at.line]?.units.length ?? 0)) return { line: at.line, unit: at.unit + 1 };
  for (let line = at.line + 1; line < lines.length; line += 1) {
    if ((lines[line]?.units.length ?? 0) > 0) return { line, unit: 0 };
  }
  return null;
}

export function prevCursor(lines: readonly EditLine[], at: Cursor): Cursor | null {
  if (at.unit > 0) return { line: at.line, unit: at.unit - 1 };
  for (let line = at.line - 1; line >= 0; line -= 1) {
    const count = lines[line]?.units.length ?? 0;
    if (count > 0) return { line, unit: count - 1 };
  }
  return null;
}

export type TapMode = "hold" | "tap";

/**
 * 按下打点键：当前单位从 `sec` 开始。
 * 同一行前一个单位还没有结尾的，结尾就是现在（连着唱）；同一行后面已经打过、却比现在早的单位清掉重打。
 * 点按模式下除了一行最后一个字，按下就跳到下一个；一行最后一个字等松开时记结尾。
 */
export function tapDown(lines: readonly EditLine[], at: Cursor, sec: number, mode: TapMode): { lines: EditLine[]; cursor: Cursor; advanced: boolean } {
  const line = lines[at.line];
  const unit = line?.units[at.unit];
  if (line === undefined || unit === undefined) return { lines: lines.slice(), cursor: at, advanced: false };
  const out = cloneLines(lines, at.line);
  const units = out[at.line]!.units;
  const time = Math.max(0, sec);
  const prev = units[at.unit - 1];
  if (prev !== undefined && prev.startSec !== null && prev.endSec === null && time > prev.startSec) prev.endSec = time;
  if (prev !== undefined && prev.endSec !== null && prev.endSec > time) prev.endSec = Math.max(time, (prev.startSec ?? time) + MIN_UNIT_SEC);
  const current = units[at.unit]!;
  current.startSec = prev?.endSec !== null && prev?.endSec !== undefined ? Math.max(time, prev.endSec) : time;
  current.endSec = null;
  for (const later of units.slice(at.unit + 1)) {
    if (later.startSec !== null && later.startSec < current.startSec) { later.startSec = null; later.endSec = null; }
  }
  const lastOfLine = at.unit === units.length - 1;
  if (mode === "tap" && !lastOfLine) return { lines: out, cursor: { line: at.line, unit: at.unit + 1 }, advanced: true };
  return { lines: out, cursor: at, advanced: false };
}

/** 松开打点键：按住模式记当前单位的结尾并前进；点按模式只给一行最后一个字记结尾。 */
export function tapUp(lines: readonly EditLine[], at: Cursor, sec: number, mode: TapMode): { lines: EditLine[]; cursor: Cursor } {
  const unit = lines[at.line]?.units[at.unit];
  if (unit === undefined || unit.startSec === null || unit.endSec !== null) return { lines: lines.slice(), cursor: at };
  const lastOfLine = at.unit === lines[at.line]!.units.length - 1;
  if (mode === "tap" && !lastOfLine) return { lines: lines.slice(), cursor: at };
  const out = cloneLines(lines, at.line);
  const current = out[at.line]!.units[at.unit]!;
  current.endSec = Math.max(sec, current.startSec! + MIN_UNIT_SEC);
  return { lines: out, cursor: nextCursor(out, at) ?? at };
}

/**
 * 挪一个单位的开始或结尾。和相邻单位贴着的边界一起挪（连唱的两个字之间只有一条线），
 * 但不越过邻居的另一头，也不让单位短于 `MIN_UNIT_SEC`。
 */
export function setEdge(lines: readonly EditLine[], at: Cursor, edge: "start" | "end", sec: number): EditLine[] {
  const unit = lines[at.line]?.units[at.unit];
  if (unit === undefined) return lines.slice();
  const out = cloneLines(lines, at.line);
  const units = out[at.line]!.units;
  const me = units[at.unit]!;
  let time = Math.max(0, sec);
  if (edge === "start") {
    const prev = units[at.unit - 1];
    const linked = prev !== undefined && prev.endSec !== null && me.startSec !== null && Math.abs(prev.endSec - me.startSec) < 1e-6;
    const floor = prev === undefined ? 0 : linked ? (prev.startSec ?? 0) + MIN_UNIT_SEC : prev.endSec ?? prev.startSec ?? 0;
    const ceil = me.endSec !== null ? me.endSec - MIN_UNIT_SEC : Infinity;
    time = Math.min(Math.max(time, floor), ceil);
    me.startSec = time;
    if (linked) prev!.endSec = time;
  } else {
    if (me.startSec === null) return lines.slice();
    const next = units[at.unit + 1];
    const linked = next !== undefined && next.startSec !== null && me.endSec !== null && Math.abs(next.startSec - me.endSec) < 1e-6;
    const floor = me.startSec + MIN_UNIT_SEC;
    const ceil = next === undefined || next.startSec === null ? Infinity : linked ? (next.endSec ?? Infinity) - MIN_UNIT_SEC : next.startSec;
    time = Math.min(Math.max(time, floor), ceil);
    me.endSec = time;
    if (linked) next!.startSec = time;
  }
  return out;
}

export function nudge(lines: readonly EditLine[], at: Cursor, edge: "start" | "end", delta: number): EditLine[] {
  const unit = lines[at.line]?.units[at.unit];
  const base = edge === "start" ? unit?.startSec : unit?.endSec;
  if (unit === undefined || base === null || base === undefined) return lines.slice();
  return setEdge(lines, at, edge, base + delta);
}

/**
 * 一个单位拆成两个（从中间的字拆开）。打过时间的，拆开后前一半保留开始、后一半保留结尾，
 * 中间那条线先放在按字数算的位置——这是给用户接着拖的起点，界面上看得见，不是来源时间。
 */
export function splitUnit(lines: readonly EditLine[], at: Cursor): EditLine[] {
  const unit = lines[at.line]?.units[at.unit];
  if (unit === undefined || unit.charEnd - unit.charStart < 2) return lines.slice();
  const out = cloneLines(lines, at.line);
  const units = out[at.line]!.units;
  const me = units[at.unit]!;
  const mid = me.charStart + Math.floor((me.charEnd - me.charStart) / 2);
  let cut: number | null = null;
  if (me.startSec !== null && me.endSec !== null) {
    cut = me.startSec + ((me.endSec - me.startSec) * (mid - me.charStart)) / (me.charEnd - me.charStart);
  }
  const second: EditUnit = { charStart: mid, charEnd: me.charEnd, startSec: cut, endSec: cut === null ? null : me.endSec };
  me.charEnd = mid;
  me.endSec = cut;
  units.splice(at.unit + 1, 0, second);
  return out;
}

/** 和下一个单位合成一个：开始取前一个的，结尾取后一个的 */
export function mergeNext(lines: readonly EditLine[], at: Cursor): EditLine[] {
  const units0 = lines[at.line]?.units;
  if (units0 === undefined || at.unit + 1 >= units0.length) return lines.slice();
  const out = cloneLines(lines, at.line);
  const units = out[at.line]!.units;
  const me = units[at.unit]!;
  const next = units[at.unit + 1]!;
  me.charEnd = next.charEnd;
  me.startSec = me.startSec ?? next.startSec;
  me.endSec = next.endSec ?? (next.startSec === null ? me.endSec : null);
  units.splice(at.unit + 1, 1);
  return out;
}

/** 清掉一行的全部时间（切分保留） */
export function clearLine(lines: readonly EditLine[], index: number): EditLine[] {
  if (lines[index] === undefined) return lines.slice();
  const out = cloneLines(lines, index);
  for (const unit of out[index]!.units) { unit.startSec = null; unit.endSec = null; }
  return out;
}

/** 一行的起点：第一个打过的单位，没有就是原来的行首 */
export function lineStart(line: EditLine): number | null {
  return line.units.find((u) => u.startSec !== null)?.startSec ?? line.timeSec;
}

/** 一行的结尾：最后一个已知的时间 */
export function lineEnd(line: EditLine): number | null {
  let end: number | null = null;
  for (const u of line.units) end = u.endSec ?? u.startSec ?? end;
  return end;
}

export type LineStatus = "done" | "partial" | "empty" | "error";

/**
 * 这一行打完了没有、能不能逐字播：
 * - `done`：每个单位都有起止，不倒退，不越过下一行的开始和音频结尾——播放器会逐字点亮；
 * - `partial` / `empty`：打了一部分 / 没打。能存，播放时这一行整行高亮；
 * - `error`：时间倒退或重叠，存不了。
 */
export function lineStatus(lines: readonly EditLine[], index: number, durationSec: number | null): { status: LineStatus; reason: string | null } {
  const line = lines[index];
  if (line === undefined) return { status: "empty", reason: null };
  let last = -Infinity;
  let timed = 0;
  for (const u of line.units) {
    if (u.startSec === null) continue;
    timed += 1;
    if (u.startSec < last - 1e-6) return { status: "error", reason: "back" };
    if (u.endSec !== null && u.endSec <= u.startSec) return { status: "error", reason: "back" };
    last = u.endSec ?? u.startSec;
  }
  if (timed === 0) return { status: "empty", reason: null };
  if (timed < line.units.length || line.units.some((u) => u.endSec === null)) return { status: "partial", reason: null };
  const next = lines.slice(index + 1).map(lineStart).find((t) => t !== null) ?? null;
  if (next !== null && last > next + 1e-6) return { status: "partial", reason: "overlapNext" };
  if (durationSec !== null && last > durationSec + 1e-6) return { status: "partial", reason: "pastEnd" };
  return { status: "done", reason: null };
}

/** 存给后端的样子：只带打过的单位；有单位的行，后端会把行首改成第一个单位的开始 */
export function toSavePayload(lines: readonly EditLine[]): { timeSec: number | null; text: string; units: { charStart: number; charEnd: number; startSec: number; endSec: number | null }[] }[] {
  return lines.map((line) => ({
    timeSec: line.timeSec,
    text: line.text,
    units: line.units
      .filter((u): u is EditUnit & { startSec: number } => u.startSec !== null)
      .map((u) => ({ charStart: u.charStart, charEnd: u.charEnd, startSec: u.startSec, endSec: u.endSec })),
  }));
}

// ────────────────────────────── KTV 两行排版 ──────────────────────────────

export interface KtvLine {
  start: number | null;
  end: number | null;
}

export interface KtvLayout {
  /** 上下两个位置各放哪一行（下标），null 是空着 */
  slots: [number | null, number | null];
  /** 正在唱或者马上要唱的那一行 */
  active: number | null;
  /** 入句前倒数还剩几拍（3、2、1），不倒数是 0 */
  countdown: number;
  /** 前奏：第一句还没开始、离开始还早，显示歌名 */
  intro: boolean;
}

/** 间隔超过这么久才倒数（秒）：句子连着唱时不需要 */
export const COUNTDOWN_GAP = 4;
/** 前奏里歌名显示到第一句开始前这么久 */
const INTRO_LEAD = 3;

/**
 * KTV 的上下交替：第 0、2、4… 行在上，第 1、3、5… 行在下。正在唱第 k 行时，另一个位置放第 k+1 行，
 * 唱完第 k 行，它的位置换成第 k+2 行。没有时间的行跳过（KTV 显示不了不知道什么时候唱的行）。
 */
export function ktvLayout(lines: readonly KtvLine[], sec: number): KtvLayout {
  const timed = lines.map((l, i) => ({ ...l, i })).filter((l) => l.start !== null);
  if (timed.length === 0) return { slots: [null, null], active: null, countdown: 0, intro: false };
  const endOf = (k: number) => {
    const line = timed[k]!;
    const next = timed[k + 1]?.start ?? null;
    return line.end ?? next ?? line.start! + 5;
  };
  let k = timed.findIndex((_, idx) => endOf(idx) > sec);
  if (k < 0) return { slots: [null, null], active: null, countdown: 0, intro: false };
  const current = timed[k]!;
  const following = timed[k + 1];
  // 位置按「在有时间的行里排第几」交替，跳过的行不打乱上下
  const slots: [number | null, number | null] = [null, null];
  slots[k % 2] = current.i;
  if (following !== undefined) slots[(k + 1) % 2] = following.i;
  const previousEnd = k > 0 ? endOf(k - 1) : null;
  const wait = current.start! - sec;
  const longGap = previousEnd === null || current.start! - previousEnd >= COUNTDOWN_GAP;
  const countdown = longGap && wait > 0 && wait <= 3 ? Math.ceil(wait) : 0;
  const intro = k === 0 && wait > INTRO_LEAD;
  return { slots, active: current.i, countdown, intro };
}
