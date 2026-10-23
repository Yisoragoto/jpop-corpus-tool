import { describe, expect, it } from "vitest";
import type { KwicHit, LyricLine } from "./api";
import { charProgress, charTimeline, keywordSpan, lyricEndBound, wordStart, type WordTiming } from "./wordTiming";
import { pickLine, timedLines } from "./usePlayback";

const hit = (text: string, left: string, keyword: string, timeSec: number | null = 10, endSec: number | null = 14): KwicHit => ({
  songId: "001", artist: "A", title: "test", audioPath: "", utteranceId: 7, lineIdx: 0, timeSec, text, left, keyword,
  right: "", repeatCount: 1, corrected: false, endSec, endKind: "nextLine", matchLen: 1,
});
const timing = (charStart: number, charEnd: number, startSec: number, source = "lrc", endSec: number | null = null): WordTiming => ({
  utteranceId: 7, charStart, charEnd, startSec, endSec, source,
});
const complete = () => [timing(0, 2, 10, "lrc", 10.5), timing(2, 4, 11, "lrc", 12), timing(4, 6, 12.4, "lrc", 13)];

describe("lyricEndBound", () => {
  it("uses the tighter next-line/track boundary, matching the Rust song timeline", () => {
    expect(lyricEndBound(999, 14)).toBe(14);
    expect(lyricEndBound(12, 14)).toBe(12);
    expect(lyricEndBound(null, 14)).toBe(14);
    expect(lyricEndBound(14, null)).toBe(14);
    expect(lyricEndBound(NaN, Infinity)).toBeNull();
    expect(charTimeline("test", 10, lyricEndBound(999, 14), [timing(0, 4, 12, "lrc", 15)])).toBeNull();
  });
});

describe("keywordSpan", () => {
  it("uses Unicode scalar offsets, including non-BMP characters", () => {
    expect(keywordSpan(hit("𠮷🎵の夜", "𠮷🎵の", "夜"))).toEqual({ start: 3, end: 4 });
  });
  it("uses the left context to distinguish repeated words", () => {
    expect(keywordSpan(hit("夜に夜", "夜に", "夜"))).toEqual({ start: 2, end: 3 });
  });
  it("falls back to an exact text match when context differs", () => {
    expect(keywordSpan(hit("初めましての色が あることを", "初めましての色が", "ある"))).toEqual({ start: 9, end: 11 });
  });
});

describe("wordStart never estimates", () => {
  const h = hit("雨が降る夜に", "雨が降る", "夜");
  it("uses the exact manual or source start without a synthetic lead", () => {
    const source = timing(4, 5, 11.5, "lrc", 12);
    expect(wordStart(h, [source, timing(4, 5, 11.8, "manual")])).toEqual({ sec: 11.8, kind: "manual" });
    expect(wordStart(h, [source])).toEqual({ sec: 11.5, kind: "lrc" });
  });
  it("seeks to the original unit start even when the keyword is inside a multi-character unit", () => {
    expect(wordStart(h, [timing(2, 6, 11, "lrc", 13)])).toEqual({ sec: 11, kind: "lrc" });
  });
  it("a known source start can be used without inventing its missing end", () => {
    expect(wordStart(h, [timing(4, 6, 12)])).toEqual({ sec: 12, kind: "lrc" });
    expect(wordStart({ ...h, timeSec: null, endSec: null }, [timing(4, 6, 12)])).toEqual({ sec: 12, kind: "lrc" });
  });
  it("falls back to line start for any keyword position, regardless of line length or duration", () => {
    expect(wordStart(h, [])).toEqual({ sec: 10, kind: "line" });
    expect(wordStart({ ...h, endSec: 1000 }, [])).toEqual({ sec: 10, kind: "line" });
    expect(wordStart(hit("夜に駆ける", "", "夜"), [])).toEqual({ sec: 10, kind: "line" });
  });
  it("ignores other utterances, estimated or unknown sources", () => {
    for (const item of [{ ...timing(4, 5, 12), utteranceId: 8 }, timing(4, 5, 12, "estimate"), timing(4, 5, 12, "unknown")]) {
      expect(wordStart(h, [item])).toEqual({ sec: 10, kind: "line" });
    }
  });
  it.each([
    timing(-1, 5, 12), timing(4.5, 5, 12), timing(4, 7, 12), timing(4, 5, NaN),
    timing(4, 5, Infinity), timing(4, 5, -1), timing(4, 5, 9), timing(4, 5, 15),
    timing(4, 5, 12, "lrc", 12), timing(4, 5, 12, "lrc", 11), timing(4, 5, 12, "lrc", Infinity),
  ])("rejects invalid units: %j", (item) => {
    expect(wordStart(h, [item])).toEqual({ sec: 10, kind: "line" });
  });
  it("does not use an estimated listening-window end as a source timestamp", () => {
    expect(wordStart({ ...h, endSec: 10.5, endKind: "estimated" }, [timing(4, 6, 12, "lrc", 13)])).toEqual({ sec: 12, kind: "lrc" });
  });
  it("does not guess between conflicting source units or a missing keyword", () => {
    expect(wordStart(h, [timing(2, 6, 11), timing(4, 6, 12)])).toEqual({ sec: 10, kind: "line" });
    expect(wordStart({ ...h, keyword: "missing" }, [timing(0, 6, 11)])).toEqual({ sec: 10, kind: "line" });
  });
  it("plain text and invalid line timestamps have no default playback position", () => {
    for (const timeSec of [null, NaN, Infinity, -1]) expect(wordStart({ ...h, timeSec }, [])).toBeNull();
  });
});

describe("charTimeline requires complete real units", () => {
  const text = "雨が降る夜に";
  it("traditional LRC never produces a character timeline, including long instrumental gaps", () => {
    expect(charTimeline(text, 10, 14, [])).toBeNull();
    expect(charTimeline(text, 10, 1000, [])).toBeNull();
    expect(charTimeline(text, 10, null, [])).toBeNull();
  });
  it("plain text does not synthesize either line or character highlighting", () => {
    expect(charTimeline(text, null, null, [])).toBeNull();
    expect(pickLine(timedLines([{ timeSec: null } as LyricLine]), 999)).toBe(-1);
  });
  it("keeps original unit times for every character; no splitting by beats, kana or character count", () => {
    expect(charTimeline(text, 10, 20, complete())).toEqual({
      starts: [10, 10, 11, 11, 12.4, 12.4], ends: [10.5, 10.5, 12, 12, 13, 13], kind: "lrc",
    });
  });
  it("accepts individual native character times without merging small kana", () => {
    expect(charTimeline("きゃ", 0, 2, [timing(0, 1, 0, "lrc", 1), timing(1, 2, 1, "lrc", 2)])?.starts).toEqual([0, 1]);
  });
  it("real units can provide the start when there is no separate LRC line timestamp", () => {
    expect(charTimeline(text, null, null, complete())?.starts[0]).toBe(10);
  });
  it("partial coverage, including unmapped whitespace, falls back for the entire line", () => {
    expect(charTimeline(text, 10, 14, complete().slice(0, 2))).toBeNull();
    expect(charTimeline("a b", 0, 4, [timing(0, 1, 0, "lrc", 1), timing(2, 3, 2, "lrc", 3)])).toBeNull();
    expect(charTimeline(text, 10, 14, [timing(1, 6, 11, "lrc", 13)])).toBeNull();
  });
  it("never fills in the end of the final source unit", () => {
    expect(charTimeline(text, 10, 100, [...complete().slice(0, 2), timing(4, 6, 12.4)])).toBeNull();
  });
  it("manual anchors alone cannot synthesize any remaining characters", () => {
    expect(charTimeline(text, 10, 14, [timing(0, 2, 10, "manual"), timing(2, 6, 11, "manual")])).toBeNull();
    expect(charTimeline(text, 10, 14, [timing(0, 2, 10, "manual", 11)])).toBeNull();
    expect(charTimeline(text, 10, 14, [...complete(), timing(4, 5, 12.8, "manual")])).toEqual(charTimeline(text, 10, 14, complete()));
  });
  it("accepts fully bounded manual units and exact-range manual replacements", () => {
    const manual = complete().map((unit) => ({ ...unit, source: "manual" }));
    expect(charTimeline(text, 10, 14, manual)?.kind).toBe("manual");
    expect(charTimeline(text, 10, 14, [...complete(), timing(2, 4, 11.2, "manual", 12.1)])?.starts).toEqual([10, 10, 11.2, 11.2, 12.4, 12.4]);
  });
  it.each([
    timing(-1, 6, 10, "lrc", 13), timing(0.5, 6, 10, "lrc", 13), timing(0, 7, 10, "lrc", 13),
    timing(0, 6, NaN, "lrc", 13), timing(0, 6, Infinity, "lrc", 13), timing(0, 6, -1, "lrc", 13),
    timing(0, 6, 9, "lrc", 13), timing(0, 6, 10, "lrc", NaN), timing(0, 6, 10, "lrc", Infinity),
    timing(0, 6, 10, "lrc", 10), timing(0, 6, 12, "lrc", 11), timing(0, 6, 10, "lrc", 15),
    timing(0, 6, 10, "estimate", 13), timing(0, 6, 10, "unknown", 13),
  ])("rejects invalid or untrusted segments: %j", (item) => {
    expect(charTimeline(text, 10, 14, [item])).toBeNull();
  });
  it("rejects overlapping characters, overlapping times, backwards units and contradictory duplicates", () => {
    expect(charTimeline(text, 10, 14, [timing(0, 3, 10, "lrc", 11), timing(2, 6, 11, "lrc", 13)])).toBeNull();
    expect(charTimeline(text, 10, 14, [timing(0, 2, 10, "lrc", 12), timing(2, 6, 11, "lrc", 13)])).toBeNull();
    expect(charTimeline(text, 10, 14, [...complete(), timing(2, 4, 11.1, "lrc", 12)])).toBeNull();
    expect(charTimeline(text, 10, 14, [timing(0, 2, 12, "lrc", 13), timing(2, 6, 10, "lrc", 11)])).toBeNull();
  });
  it("rejects invalid line bounds and empty text", () => {
    for (const time of [NaN, Infinity, -1]) expect(charTimeline(text, time, 14, complete())).toBeNull();
    for (const end of [NaN, Infinity, -1, 10]) expect(charTimeline(text, 10, end, complete())).toBeNull();
    expect(charTimeline("", 0, 1, [])).toBeNull();
  });
  it("supports non-BMP characters without turning UTF-16 units into extra characters", () => {
    expect(charTimeline("𠮷🎵きょう𠮷", 0, 6, [timing(0, 2, 0, "lrc", 2), timing(2, 5, 2, "lrc", 5), timing(5, 6, 5, "lrc", 6)]))
      .toEqual({ starts: [0, 0, 2, 2, 2, 5], ends: [2, 2, 5, 5, 5, 6], kind: "lrc" });
  });
  it("mixed songs fall back per line and repeated choruses use their own utterance times", () => {
    const rows = [
      { utteranceId: 7, text, timeSec: 10 }, { utteranceId: 8, text, timeSec: 20 },
      { utteranceId: 9, text, timeSec: 30 }, { utteranceId: 10, text, timeSec: null },
    ];
    const timings = [...complete(), ...complete().map((unit) => ({ ...unit, utteranceId: 9, startSec: unit.startSec + 20, endSec: unit.endSec! + 20 }))];
    const timelines = rows.map((row) => charTimeline(row.text, row.timeSec, null, timings.filter((unit) => unit.utteranceId === row.utteranceId)));
    expect(timelines.map((timeline) => timeline?.kind ?? null)).toEqual(["lrc", null, "lrc", null]);
    expect(timelines[0]?.starts[0]).toBe(10);
    expect(timelines[2]?.starts[0]).toBe(30);
    const lines = timedLines(rows as LyricLine[]);
    expect(pickLine(lines, 21)).toBe(1);
    expect(pickLine(lines, 31)).toBe(2);
  });
});

describe("charProgress shares progress across real units", () => {
  const timeline = charTimeline("雨が降る夜に", 10, 14, complete())!;
  it("advances every character in one source unit together, preserving actual gaps", () => {
    expect(charProgress(timeline, 9)).toEqual({ done: 0, activeEnd: 0, frac: 0 });
    expect(charProgress(timeline, 10.25)).toEqual({ done: 0, activeEnd: 2, frac: 0.5 });
    expect(charProgress(timeline, 10.8)).toEqual({ done: 2, activeEnd: 2, frac: 0 });
    expect(charProgress(timeline, 11.5)).toEqual({ done: 2, activeEnd: 4, frac: 0.5 });
    expect(charProgress(timeline, 100)).toEqual({ done: 6, activeEnd: 6, frac: 0 });
  });
  it("invalid playback position does not create a highlight", () => {
    for (const position of [NaN, Infinity, -1]) expect(charProgress(timeline, position)).toEqual({ done: 0, activeEnd: 0, frac: 0 });
  });
});
