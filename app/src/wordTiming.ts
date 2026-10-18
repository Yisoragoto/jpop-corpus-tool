/**
 * 词云、例句里「从这个词开始放」：这个词在这一行里从第几秒开始唱。
 *
 * 三个来源，按可信程度依次取（后端那张表见 `jp-corpus/src/word_timing.rs`）：
 *
 * 1. **手动校准**（manual）：听着按一下存下来的，最准；
 * 2. **逐字歌词**（lrc）：导入的增强型 LRC 自带的逐字时间；
 * 3. **估算**：都没有时按这一行的字数估——这个词前面有多少拍（假名一拍、汉字算将近两拍），
 *    在这一行唱的那段时间里按比例摆。误差大约零点几秒到一秒，所以界面上标「估」、可以校准。
 *
 * 位置一律用**字符下标**（Unicode 标量，`[...text]` 的下标），和后端一致。
 */

import type { KwicHit } from "./api";

export interface WordTiming {
  utteranceId: number;
  charStart: number;
  charEnd: number;
  startSec: number;
  endSec: number | null;
  /** lrc / manual */
  source: string;
}

export type TimingKind = "manual" | "lrc" | "estimate" | "line";

export interface WordStart {
  /** 从这里开始放（已经留了一点提前量） */
  sec: number;
  kind: TimingKind;
}

/** 关键词在这一行里的字符区间 `[start, end)` */
export function keywordSpan(hit: Pick<KwicHit, "text" | "left" | "keyword">): { start: number; end: number } {
  const chars = [...hit.text];
  const kw = [...hit.keyword];
  let start = -1;
  if (hit.text.startsWith(hit.left)) start = [...hit.left].length;
  // 原文对不上（KWIC 退回了拼接法）：在原文里找关键词
  if (start < 0 || chars.slice(start, start + kw.length).join("") !== hit.keyword) {
    const at = hit.text.indexOf(hit.keyword);
    start = at < 0 ? 0 : [...hit.text.slice(0, at)].length;
  }
  return { start, end: Math.min(chars.length, start + Math.max(1, kw.length)) };
}

const SMALL_KANA = /[ぁぃぅぇぉゃゅょゎゕゖァィゥェォャュョヮヵヶ]/u;
const KANA = /[ぁ-ゖァ-ヺー]/u;
const KANJI = /[㐀-鿿豈-﫿々〆]/u;
const LATIN = /[A-Za-zＡ-Ｚａ-ｚ]/u;
const DIGIT = /[0-9０-９]/u;

/** 一个字大约唱多少拍。拗音的小字和前一个字合成一拍；空格、标点算一小口气 */
function beats(ch: string): number {
  if (SMALL_KANA.test(ch)) return 0;
  if (KANA.test(ch)) return 1;
  if (KANJI.test(ch)) return 1.8;
  if (LATIN.test(ch)) return 0.4;
  if (DIGIT.test(ch)) return 0.8;
  if (/\s/u.test(ch)) return 0.6;
  return 0.3;
}

/** 一拍大约多长：J-pop 常见的速度一秒四五拍；只在不知道这一行唱多久时用 */
const SEC_PER_BEAT = 0.24;
/** 估算的提前量：宁可早一点听到前面的尾音，也不要把这个词的开头切掉 */
const ESTIMATE_LEAD = 0.25;
/** 逐字 / 校准的提前量：放音频有一点点启动延迟 */
const EXACT_LEAD = 0.05;

/**
 * 按字数估这个词从第几秒开始唱。`lineStart` 是这一行的时间戳，`lineEnd` 是下一行开始（不知道就是 null）。
 * 这一行唱的时长取「下一行开始之前」和「按字数估的时长」里短的那个——后面常有一段间奏，不能摊进去。
 */
export function estimateWordStart(text: string, charStart: number, lineStart: number, lineEnd: number | null): number {
  const chars = [...text];
  const all = chars.reduce((sum, ch) => sum + beats(ch), 0);
  if (all <= 0 || charStart <= 0) return lineStart;
  const before = chars.slice(0, charStart).reduce((sum, ch) => sum + beats(ch), 0);
  const sung = all * SEC_PER_BEAT + 0.4;
  const span = lineEnd !== null && lineEnd > lineStart ? Math.min(lineEnd - lineStart, sung) : sung;
  return lineStart + (before / all) * span;
}

/**
 * 这一处从第几秒开始放：校准 > 逐字 > 估算。这一行连时间戳都没有就是 null（放不了）。
 * `timings` 是这一行的词级时间（可以混着别的行，按 utteranceId 筛）。
 */
export function wordStart(hit: KwicHit, timings: readonly WordTiming[]): WordStart | null {
  if (hit.timeSec === null) return null;
  const { start } = keywordSpan(hit);
  const mine = timings.filter((t) => t.utteranceId === hit.utteranceId);

  const manual = mine.find((t) => t.source === "manual" && t.charStart === start);
  if (manual) return { sec: Math.max(0, manual.startSec - EXACT_LEAD), kind: "manual" };

  const seg = mine.find((t) => t.source === "lrc" && t.charStart <= start && start < t.charEnd);
  if (seg) {
    // 关键词在一段逐字时间的中间（一段管好几个字时）：在这段里按字数插
    const segEnd = seg.endSec ?? seg.startSec + (seg.charEnd - seg.charStart) * SEC_PER_BEAT;
    const frac = (start - seg.charStart) / Math.max(1, seg.charEnd - seg.charStart);
    return { sec: Math.max(0, seg.startSec + frac * (segEnd - seg.startSec) - EXACT_LEAD), kind: "lrc" };
  }

  if (start === 0) return { sec: hit.timeSec, kind: "line" };
  const est = estimateWordStart(hit.text, start, hit.timeSec, hit.endSec);
  return { sec: Math.max(hit.timeSec, est - ESTIMATE_LEAD), kind: "estimate" };
}

/** 按键校准时扣掉的反应时间：听到再按，手总是慢一点 */
export const REACTION_SEC = 0.15;

// ────────────────────────────── 逐字点亮 ──────────────────────────────

/** 一行里每个字什么时候开始点亮、什么时候点亮完（秒）。下标是字符下标 */
export interface CharTimeline {
  starts: number[];
  ends: number[];
  /** 用到了哪种来源：有校准就是 manual，有逐字就是 lrc，都没有是按字数估 */
  kind: "manual" | "lrc" | "estimate";
}

/**
 * 当前行每个字的点亮时间（卡拉 OK 歌词）。
 *
 * 先定几个「锚点」（第几个字在第几秒）：行首是这一行的时间戳、行尾是按字数估的唱完时刻
 * （和 `estimateWordStart` 同一个算法：取「下一行开始」和「按字数估的时长」里短的那个）；
 * 逐字歌词的每一段开头（有结束时间的话还有结尾）、手动校准的那个字，都是锚点，校准盖过逐字。
 * 锚点之间按拍数（`beats`）比例摊——所以没有任何逐字时间时，每个字的开始和 `estimateWordStart` 一样。
 * 拗音的小字和前一个字一起亮。
 */
export function charTimeline(
  text: string,
  lineStart: number,
  lineEnd: number | null,
  timings: readonly WordTiming[],
): CharTimeline {
  const chars = [...text];
  const n = chars.length;
  const weight = chars.map(beats);
  const all = weight.reduce((sum, w) => sum + w, 0);
  const sung = all * SEC_PER_BEAT + 0.4;
  const span = lineEnd !== null && lineEnd > lineStart ? Math.min(lineEnd - lineStart, sung) : sung;

  const anchors = new Map<number, number>([[0, lineStart]]);
  const lrc = timings.filter((t) => t.source === "lrc" && t.charStart < t.charEnd && t.charEnd <= n);
  const manual = timings.filter((t) => t.source === "manual" && t.charStart >= 0 && t.charStart < n);
  // 先放结尾、再放开头：前一段的结尾和后一段的开头落在同一个字上时，以开头为准
  for (const t of lrc) if (t.endSec !== null) anchors.set(t.charEnd, t.endSec);
  for (const t of lrc) anchors.set(t.charStart, t.startSec);
  for (const t of manual) anchors.set(t.charStart, t.startSec);

  // 按字的顺序走一遍，时间倒退的锚点丢掉（校准和逐字打架时，先到的那个算数）
  const sorted = [...anchors.entries()].filter(([i]) => i < n).sort((a, b) => a[0] - b[0]);
  const kept: [number, number][] = [];
  for (const [i, t] of sorted) {
    const last = kept[kept.length - 1];
    if (last === undefined || t >= last[1]) kept.push([i, t]);
  }
  // 行尾：有逐字结尾就用，否则按估的时长；落在最后一个锚点之前（逐字比估的晚）就从那里按拍数往后推
  const beatsFrom = (from: number) => weight.slice(from).reduce((sum, w) => sum + w, 0);
  const last = kept[kept.length - 1] ?? [0, lineStart];
  let endAt = anchors.get(n) ?? lineStart + span;
  if (endAt <= last[1]) endAt = last[1] + Math.max(0.2, beatsFrom(last[0]) * SEC_PER_BEAT);
  kept.push([n, endAt]);

  // 逐字的一段唱完、到下一段开始之间有空当：这一段的字在它自己的结束时间就亮完，空当里不动
  const segEnd = new Map(lrc.filter((t) => t.endSec !== null).map((t) => [t.charStart, t] as const));
  const starts = new Array<number>(n).fill(lineStart);
  const ends = new Array<number>(n).fill(lineStart);
  for (let a = 0; a + 1 < kept.length; a += 1) {
    const [i0, t0] = kept[a]!;
    const [i1, next] = kept[a + 1]!;
    const seg = segEnd.get(i0);
    const t1 = seg !== undefined && seg.charEnd === i1 && seg.endSec! > t0 && seg.endSec! < next ? seg.endSec! : next;
    const total = weight.slice(i0, i1).reduce((sum, w) => sum + w, 0);
    let cum = 0;
    for (let k = i0; k < i1; k += 1) {
      // 这一段一拍都没有（全是标点）：按字数平分
      const w = total > 0 ? weight[k]! : 1;
      const denom = total > 0 ? total : i1 - i0;
      starts[k] = t0 + (cum / denom) * (t1 - t0);
      cum += w;
      ends[k] = t0 + (cum / denom) * (t1 - t0);
    }
  }
  for (let k = 1; k < n; k += 1) {
    if (SMALL_KANA.test(chars[k]!)) {
      starts[k] = starts[k - 1]!;
      ends[k] = ends[k - 1]!;
    }
  }
  const kind = manual.length > 0 ? "manual" : lrc.length > 0 ? "lrc" : "estimate";
  return { starts, ends, kind };
}

/**
 * 播到 `sec` 时这一行亮到哪：前 `done` 个字已经亮完，第 `done` 个字亮了 `frac`（0~1）。
 * 拗音的小字和前一个字同起同止，一起算亮完。
 */
export function charProgress(timeline: CharTimeline, sec: number): { done: number; frac: number } {
  const { starts, ends } = timeline;
  let done = 0;
  while (done < ends.length && ends[done]! <= sec) done += 1;
  if (done >= starts.length) return { done, frac: 0 };
  const s = starts[done]!;
  const e = ends[done]!;
  const frac = sec <= s || e <= s ? 0 : Math.min(1, (sec - s) / (e - s));
  return { done, frac };
}
