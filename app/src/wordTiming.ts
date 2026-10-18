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
