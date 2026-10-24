// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";

import { charAtClick, lineRuns, tokenAtChar, toAmllLines } from "./amllLines";
import type { LyricLine, LineToken } from "./api";
import type { Segment } from "./lyricSegments";
import type { CharTimeline } from "./wordTiming";

const token = (surface: string): LineToken => ({ surface, lemma: surface, pos: "NOUN" } as LineToken);
const line = (text: string, timeSec: number | null, tokens: LineToken[] = []): LyricLine =>
  ({ utteranceId: 1, lineIdx: 0, timeSec, text, tokens });

describe("拆段", () => {
  it("振假名一段，其余按原文顺序；对不上原文就整行一段", () => {
    const segs: Segment[] = [
      { kind: "token", index: 0, pieces: [{ text: "夜", reading: "よる" }] },
      { kind: "token", index: 1, pieces: [] },
      { kind: "gap", text: " " },
      { kind: "token", index: 2, pieces: [{ text: "駆", reading: "か" }, { text: "ける" }] },
    ];
    const tokens = [token("夜"), token("に"), token("駆ける")];
    expect(lineRuns("夜に 駆ける", segs, tokens)).toEqual([
      { at: 0, text: "夜", reading: "よる" },
      { at: 1, text: "に" },
      { at: 2, text: " " },
      { at: 3, text: "駆", reading: "か" },
      { at: 4, text: "ける" },
    ]);
    expect(lineRuns("別の行", segs, tokens)).toEqual([{ at: 0, text: "別の行" }]);
    expect(lineRuns("夜", null, [])).toEqual([{ at: 0, text: "夜" }]);
  });
});

describe("转成 AMLL 的行", () => {
  // 「夜に 駆ける」：夜 / に / 空格 / 駆け（一个单位两个字）/ る
  const timeline: CharTimeline = {
    starts: [10, 10.5, 11, 11, 11, 11.6],
    ends: [10.5, 11, 11.6, 11.6, 11.6, 12],
    kind: "lrc",
  };

  it("同一个真实单位的字合成一个词，空白单独成词且不占时间，每个词带原文位置", () => {
    const lines = toAmllLines([line("夜に 駆ける", 10)], [timeline], [null], true, null)!;
    expect(lines[0]!.words.map((w) => [w.word, w.startTime, w.endTime, w.charStart])).toEqual([
      ["夜", 10000, 10500, 0],
      ["に", 10500, 11000, 1],
      [" ", 11000, 11000, 2],
      ["駆け", 11000, 11600, 3],
      ["る", 11600, 12000, 5],
    ]);
    expect(lines[0]).toMatchObject({ startTime: 10000, endTime: 12000, lineKey: 0 });
  });

  it("有振假名的段合成一个词带 ruby；关掉振假名就按单位拆", () => {
    const segs: Segment[] = [
      { kind: "token", index: 0, pieces: [{ text: "夜", reading: "よる" }] },
      { kind: "token", index: 1, pieces: [] },
      { kind: "gap", text: " " },
      { kind: "token", index: 2, pieces: [{ text: "駆", reading: "か" }, { text: "ける" }] },
    ];
    const lyric = line("夜に 駆ける", 10, [token("夜"), token("に"), token("駆ける")]);
    const on = toAmllLines([lyric], [timeline], [segs], true, null)!;
    expect(on[0]!.words[0]).toMatchObject({ word: "夜", ruby: [{ word: "よる", startTime: 10000, endTime: 10500 }] });
    expect(on[0]!.words.find((w) => w.word === "駆")).toMatchObject({ charStart: 3, startTime: 11000, endTime: 11600 });
    const off = toAmllLines([lyric], [timeline], [segs], false, null)!;
    expect(off[0]!.words.every((w) => w.ruby === undefined)).toBe(true);
  });

  it("没有逐字时间的行在行首整行亮起；没有时间的行接在上一行后面；整首都没时间返回 null", () => {
    const lines = toAmllLines([line("あい う", 5), line("（間奏）", null), line("え", 9)], [null, null, null], [null, null, null], true, 30)!;
    expect(lines[0]!.words.map((w) => [w.word, w.startTime, w.endTime])).toEqual([["あい", 5000, 5000], [" ", 5000, 5000], ["う", 5000, 5000]]);
    expect(lines[0]).toMatchObject({ startTime: 5000, endTime: 9000 });
    expect(lines[1]).toMatchObject({ startTime: 9000, lineKey: 1 });
    expect(lines[2]).toMatchObject({ startTime: 9000, endTime: 30000 });
    expect(toAmllLines([line("あ", null)], [null], [null], true, null)).toBeNull();
  });
});

describe("点词", () => {
  it("原文位置换算成分词；点在词之间返回 null", () => {
    const tokens = [token("夜"), token("に"), token("駆ける")];
    expect(tokenAtChar("夜に 駆ける", tokens, 4)).toEqual({ index: 2, span: [3, 6] });
    expect(tokenAtChar("夜に 駆ける", tokens, 2)).toBeNull();
  });

  it("点击处在词正文里的偏移，跳过上面的注音", () => {
    const el = document.createElement("span");
    el.dataset.charStart = "3";
    el.innerHTML = '<span class="FmKaba_rubyWord"><span>かけ</span></span><span><span>駆</span><span>け</span></span>';
    const second = [...el.querySelectorAll("span")].find((s) => s.textContent === "け")!.firstChild!;
    expect(charAtClick(el, { startContainer: second, startOffset: 0 })).toBe(4);
    const rubyText = el.querySelector(".FmKaba_rubyWord span")!.firstChild!;
    expect(charAtClick(el, { startContainer: rubyText, startOffset: 1 })).toBe(3);
    expect(charAtClick(el, null)).toBe(3);
  });
});
