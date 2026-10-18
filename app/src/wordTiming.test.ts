import { describe, expect, it } from "vitest";

import type { KwicHit } from "./api";
import { charProgress, charTimeline, estimateWordStart, keywordSpan, wordStart, type WordTiming } from "./wordTiming";

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

describe("charTimeline", () => {
  const text = "雨が降る夜に";

  it("没有逐字时间：每个字的开始和 estimateWordStart 一致，首尾相接", () => {
    const tl = charTimeline(text, 10, 14, []);
    expect(tl.kind).toBe("estimate");
    expect(tl.starts).toHaveLength(6);
    [...text].forEach((_, k) => expect(tl.starts[k]).toBeCloseTo(estimateWordStart(text, k, 10, 14), 9));
    for (let k = 0; k + 1 < 6; k += 1) expect(tl.ends[k]).toBeCloseTo(tl.starts[k + 1]!, 9);
    // 汉字 1.8 拍、假名 1 拍：雨比が占得长
    expect(tl.ends[0]! - tl.starts[0]!).toBeGreaterThan(tl.ends[1]! - tl.starts[1]!);
    // 6 个字 = 8.4 拍 ≈ 2.4 秒，比到下一行的 4 秒短：后面是间奏，不摊进去
    expect(tl.ends[5]).toBeCloseTo(10 + 8.4 * 0.24 + 0.4, 9);
  });

  it("下一行来得早：整行压进到下一行之前", () => {
    const tl = charTimeline(text, 10, 11, []);
    expect(tl.ends[5]).toBeCloseTo(11, 9);
  });

  it("拗音的小字和前一个字一起亮", () => {
    const tl = charTimeline("きょう", 0, null, []);
    expect(tl.starts[1]).toBe(tl.starts[0]);
    expect(tl.ends[1]).toBe(tl.ends[0]);
    expect(charProgress(tl, tl.ends[0]!)).toEqual({ done: 2, frac: 0 });
  });

  it("逐字歌词：每段开头用真实时间，段内按拍数摊", () => {
    const ts = [
      timing(0, 2, 10, "lrc", 10.5), // 雨が
      timing(2, 4, 11, "lrc", 12), // 降る
      timing(4, 6, 12.4, "lrc", 13), // 夜に
    ];
    const tl = charTimeline(text, 10, 20, ts);
    expect(tl.kind).toBe("lrc");
    expect(tl.starts[0]).toBe(10);
    expect(tl.starts[2]).toBe(11);
    expect(tl.starts[4]).toBe(12.4);
    expect(tl.ends[5]).toBe(13);
    // 段与段之间的空当（10.5~11、12~12.4）：前一段在自己的结束时间就亮完
    expect(tl.ends[1]).toBe(10.5);
    expect(tl.ends[3]).toBe(12);
    // 降(1.8) る(1)：段内按拍数
    expect(tl.starts[3]).toBeCloseTo(11 + 1.8 / 2.8, 9);
    expect(charProgress(tl, 10.8)).toEqual({ done: 2, frac: 0 });
  });

  it("手动校准盖过逐字，倒退的锚点丢掉", () => {
    const ts = [timing(4, 6, 12.4, "lrc"), timing(4, 5, 12.9, "manual"), timing(2, 3, 9, "manual")];
    const tl = charTimeline(text, 10, 20, ts);
    expect(tl.kind).toBe("manual");
    expect(tl.starts[4]).toBe(12.9);
    // 降 校准到 9 秒，比行首还早：丢掉，按拍数摊
    expect(tl.starts[2]).toBeGreaterThan(10);
    for (let k = 1; k < 6; k += 1) expect(tl.starts[k]).toBeGreaterThanOrEqual(tl.starts[k - 1]!);
  });

  it("逐字的最后一段没有结束时间：行尾从它开始按拍数往后推", () => {
    const tl = charTimeline(text, 10, 11, [timing(4, 6, 12, "lrc")]);
    expect(tl.starts[4]).toBe(12);
    expect(tl.ends[5]).toBeCloseTo(12 + 2.8 * 0.24, 9);
  });

  it("charProgress：亮到第几个字、这个字亮了多少", () => {
    const tl = charTimeline(text, 10, 14, []);
    expect(charProgress(tl, 9)).toEqual({ done: 0, frac: 0 });
    const mid = (tl.starts[2]! + tl.ends[2]!) / 2;
    const p = charProgress(tl, mid);
    expect(p.done).toBe(2);
    expect(p.frac).toBeCloseTo(0.5, 9);
    expect(charProgress(tl, 100)).toEqual({ done: 6, frac: 0 });
  });

  it("按字符算：补充平面的字也只占一格", () => {
    expect(charTimeline("𠮷野家", 0, null, []).starts).toHaveLength(3);
  });
});
