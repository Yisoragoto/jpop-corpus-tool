/**
 * 我们的歌词行 → AMLL 的歌词行（`@applemusic-like-lyrics/core`，打过补丁的副本见 `app/vendor/amll-core`）。
 *
 * - 时间只用已验证的逐字时间轴（`charTimeline`）。同一个真实单位里的几个字合成一个 AMLL 词，
 *   不把一个单位拆开分时间；
 * - 带振假名的那一段（`Piece.reading`）合成一个词，读音放进 AMLL 的 `ruby`；
 * - 空白单独成词：AMLL 遇到带空格的词会按字数均摊时间，我们不让它估；
 * - 没有逐字时间的行，每个词都在行首瞬间亮起（起止相同），整行一起亮，和 KTV 的规则一样不按字估算；
 * - 每个词带 `charStart`（补丁写到 `data-char-start`），点词时换算回原文位置去查词；
 *   每行带 `lineKey`（我们的行号）：AMLL 会按开始时间重排行，它事件里的行号不一定是我们的。
 */

import type { LyricLine as AmllLine, LyricWord as AmllWord } from "@applemusic-like-lyrics/core";

import type { LyricLine } from "./api";
import { alignTokens, type Segment } from "./lyricSegments";
import type { CharTimeline } from "./wordTiming";
import { validLyricTime } from "./wordTiming";

export type KeyedAmllLine = AmllLine & { lineKey: number };

interface Run {
  /** 在整行里的起始字符下标 */
  at: number;
  text: string;
  reading?: string;
}

/** 一行拆成连续的几段：有振假名的段、没有的段。拆分对不上原文就整行一段 */
export function lineRuns(text: string, segments: Segment[] | null, tokens: LyricLine["tokens"]): Run[] {
  const chars = [...text];
  if (segments === null) return [{ at: 0, text }];
  const runs: Run[] = [];
  let at = 0;
  for (const segment of segments) {
    const pieces = segment.kind === "gap"
      ? [{ text: segment.text }]
      : segment.pieces.length > 0 ? segment.pieces : [{ text: tokens[segment.index]?.surface ?? "" }];
    for (const piece of pieces) {
      const length = [...piece.text].length;
      if (length === 0) continue;
      runs.push(piece.reading === undefined ? { at, text: piece.text } : { at, text: piece.text, reading: piece.reading });
      at += length;
    }
  }
  return at === chars.length && runs.map((r) => r.text).join("") === text ? runs : [{ at: 0, text }];
}

function word(text: string, charStart: number, start: number, end: number, reading?: string): AmllWord {
  const out: AmllWord = { word: text, startTime: start, endTime: end, romanWord: "", obscene: false, charStart };
  if (reading !== undefined) out.ruby = [{ word: reading, startTime: start, endTime: end }];
  return out;
}

const SPACE = /\s/u;

/**
 * 一段文字拆成 AMLL 的词。`clock(i)` 给第 i 个字（整行下标）的真实起止；null 表示这行没有逐字时间，
 * 全部词都在 `lineStart` 瞬间亮起。
 */
function runWords(run: Run, withRuby: boolean, clock: ((i: number) => [number, number]) | null, lineStart: number): AmllWord[] {
  const chars = [...run.text];
  const time = (i: number): [number, number] => (clock === null ? [lineStart, lineStart] : clock(i));
  if (run.reading !== undefined && withRuby && !chars.some((c) => SPACE.test(c))) {
    return [word(run.text, run.at, time(run.at)[0], time(run.at + chars.length - 1)[1], run.reading)];
  }
  const words: AmllWord[] = [];
  let i = 0;
  while (i < chars.length) {
    const at = run.at + i;
    const space = SPACE.test(chars[i]!);
    let j = i + 1;
    // 空白连成一段；字按「同一个真实单位」连成一段（没有逐字时间时整段连着）
    while (j < chars.length && SPACE.test(chars[j]!) === space
      && (space || clock === null || (time(run.at + j)[0] === time(at)[0] && time(run.at + j)[1] === time(at)[1]))) j += 1;
    const [start, end] = time(at);
    const text = chars.slice(i, j).join("");
    // 空白不亮也不占时间：起止都落在前一个字结束的地方
    words.push(space ? word(text, at, words.at(-1)?.endTime ?? start, words.at(-1)?.endTime ?? start) : word(text, at, start, end));
    i = j;
  }
  return words;
}

/**
 * 整首歌转成 AMLL 的行（时间单位毫秒）。没有任何时间的歌返回 null：AMLL 只能播有时间轴的歌词。
 */
export function toAmllLines(
  lyrics: readonly LyricLine[],
  timelines: readonly (CharTimeline | null)[],
  segments: readonly (Segment[] | null)[],
  withRuby: boolean,
  durationSec: number | null,
): KeyedAmllLine[] | null {
  const starts = lyrics.map((line, i) => timelines[i]?.starts[0] ?? (validLyricTime(line.timeSec) ? line.timeSec : null));
  if (starts.every((s) => s === null)) return null;
  const ms = (sec: number) => Math.round(sec * 1000);
  const out: KeyedAmllLine[] = [];
  let previousEnd = 0;
  lyrics.forEach((line, index) => {
    const timeline = timelines[index] ?? null;
    const nextStart = starts.slice(index + 1).find((s): s is number => s !== null) ?? null;
    const start = starts[index] ?? previousEnd;
    const fallbackEnd = nextStart ?? (durationSec !== null && durationSec > start ? durationSec : start + 5);
    const end = timeline !== null ? timeline.ends[timeline.ends.length - 1] ?? fallbackEnd : Math.max(start, fallbackEnd);
    const clock = timeline === null ? null : (i: number): [number, number] => [ms(timeline.starts[i] ?? start), ms(timeline.ends[i] ?? end)];
    const runs = lineRuns(line.text, segments[index] ?? null, line.tokens);
    const words = runs.flatMap((run) => runWords(run, withRuby, clock, ms(start)));
    out.push({
      words: words.length > 0 ? words : [word(line.text || " ", 0, ms(start), ms(start))],
      translatedLyric: "",
      romanLyric: "",
      startTime: ms(start),
      endTime: Math.max(ms(start), ms(end)),
      isBG: false,
      isDuet: false,
      lineKey: index,
    });
    previousEnd = end;
  });
  return out;
}

/** 原文第 `charIndex` 个字落在哪个分词里（下标和它在原文里的 [起, 止)）；落在词之间的空白、标点上返回 null */
export function tokenAtChar(text: string, tokens: LyricLine["tokens"], charIndex: number): { index: number; span: [number, number] } | null {
  const spans = alignTokens([...text], tokens);
  if (spans === null) return null;
  const index = spans.findIndex(([a, b]) => a <= charIndex && charIndex < b);
  return index < 0 ? null : { index, span: spans[index]! };
}

/**
 * 点在一个词元素上的哪个字：词元素的 `data-char-start` 加上点击处在这个词正文里的偏移
 * （跳过上面的注音）。`range` 是 `caretRangeFromPoint` 的结果；拿不到就当点在词首。
 */
export function charAtClick(wordElement: Element, range: { startContainer: Node; startOffset: number } | null): number | null {
  const base = Number((wordElement as HTMLElement).dataset?.charStart);
  if (!Number.isInteger(base)) return null;
  if (range === null || !wordElement.contains(range.startContainer)) return base;
  let offset = 0;
  const walker = document.createTreeWalker(wordElement, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    // 注音、音译在 AMLL 里是单独的元素，不算原文的字
    if (node.parentElement?.closest("[class*='rubyWord'],[class*='romanWord']")) continue;
    const text = node.textContent ?? "";
    if (node === range.startContainer) return base + offset + [...text.slice(0, range.startOffset)].length;
    offset += [...text].length;
  }
  return base;
}
