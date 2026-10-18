import { describe, expect, it } from "vitest";

import type { KwicHit } from "./api";
import { estimateWordStart, keywordSpan, wordStart, type WordTiming } from "./wordTiming";

const hit = (text: string, left: string, keyword: string, timeSec: number | null = 10, endSec: number | null = 14): KwicHit => ({
  songId: "001", artist: "A", title: "t", audioPath: "", utteranceId: 7, lineIdx: 0, timeSec, text, left, keyword,
  right: "", repeatCount: 1, corrected: false, endSec, endKind: null, matchLen: 1,
});
const timing = (charStart: number, charEnd: number, startSec: number, source: string, endSec: number | null = null): WordTiming => ({
  utteranceId: 7, charStart, charEnd, startSec, endSec, source,
});

describe("keywordSpan", () => {
  it("按字符算，不是按 UTF-16", () => {
    expect(keywordSpan(hit("本当なんだ　夜みたいで", "本当なんだ　", "夜"))).toEqual({ start: 6, end: 7 });
    expect(keywordSpan(hit("𠮷野家の夜", "𠮷野家の", "夜"))).toEqual({ start: 4, end: 5 });
  });
  it("左边对不上原文就在原文里找", () => {
    expect(keywordSpan(hit("初めましての色が あることを", "初めましての色が", "ある"))).toEqual({ start: 9, end: 11 });
  });
});

describe("wordStart", () => {
  const h = hit("雨が降る夜に", "雨が降る", "夜");

  it("校准过的最优先，逐字其次", () => {
    const ts = [timing(4, 5, 11.5, "lrc"), timing(4, 5, 11.8, "manual")];
    expect(wordStart(h, ts)).toEqual({ sec: 11.75, kind: "manual" });
    expect(wordStart(h, [ts[0]!])).toEqual({ sec: 11.45, kind: "lrc" });
  });

  it("关键词落在一段逐字时间中间就在段里插", () => {
    const r = wordStart(h, [timing(2, 6, 11, "lrc", 13)]);
    expect(r?.kind).toBe("lrc");
    expect(r?.sec).toBeCloseTo(11 + 0.5 * 2 - 0.05);
  });

  it("别的行的时间不算", () => {
    expect(wordStart(h, [{ ...timing(4, 5, 99, "manual"), utteranceId: 8 }])?.kind).toBe("estimate");
  });

  it("行首就是行时间；没有时间戳放不了", () => {
    expect(wordStart(hit("夜に駆ける", "", "夜"), [])).toEqual({ sec: 10, kind: "line" });
    expect(wordStart(hit("夜", "", "夜", null), [])).toBeNull();
  });
});

describe("estimateWordStart", () => {
  it("越靠后越晚，但不超过这一行", () => {
    const a = estimateWordStart("雨が降る夜に", 2, 10, 14);
    const b = estimateWordStart("雨が降る夜に", 4, 10, 14);
    expect(a).toBeGreaterThan(10);
    expect(b).toBeGreaterThan(a);
    expect(b).toBeLessThan(14);
  });
  it("后面有一大段间奏时不摊进去", () => {
    // 下一行在 40 秒后，按字数估这一行只唱两秒多
    expect(estimateWordStart("雨が降る夜に", 4, 10, 50)).toBeLessThan(12);
  });
  it("拗音的小字不单算一拍", () => {
    expect(estimateWordStart("きゃあ", 2, 0, null)).toBeCloseTo(estimateWordStart("きあ", 1, 0, null));
  });
});
