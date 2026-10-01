import { describe, expect, it } from "vitest";

import type { LineToken } from "./api";
import { parseLyricsDisplay, DEFAULT_LYRICS_DISPLAY } from "./lyricsDisplay";
import { segmentLine } from "./lyricSegments";

const tok = (surface: string): LineToken => ({ surface, lemma: surface, pos: "NOUN" });

describe("segmentLine", () => {
  it("puts ruby inside the token and keeps okurigana plain", () => {
    const segments = segmentLine("思い出した", [tok("思い出し"), tok("た")], [
      { start: 0, end: 1, reading: "おも" },
      { start: 2, end: 3, reading: "だ" },
    ]);
    expect(segments).toEqual([
      { kind: "token", index: 0, pieces: [{ text: "思", reading: "おも" }, { text: "い" }, { text: "出", reading: "だ" }, { text: "し" }] },
      { kind: "token", index: 1, pieces: [{ text: "た" }] },
    ]);
  });

  it("keeps the spaces between tokens that the tokenizer dropped", () => {
    const segments = segmentLine("春も　消毒も", [tok("春"), tok("も"), tok("消毒"), tok("も")], []);
    expect(segments?.map((s) => (s.kind === "gap" ? `[${s.text}]` : s.pieces.map((p) => p.text).join("")))).toEqual([
      "春",
      "も",
      "[　]",
      "消毒",
      "も",
    ]);
  });

  it("counts characters, not UTF-16 units", () => {
    const segments = segmentLine("🎵夜", [tok("🎵"), tok("夜")], [{ start: 1, end: 2, reading: "よる" }]);
    expect(segments?.[1]).toEqual({ kind: "token", index: 1, pieces: [{ text: "夜", reading: "よる" }] });
  });

  it("gives up instead of guessing when tokens or rubies do not line up", () => {
    expect(segmentLine("夜が明ける", [tok("朝")], [])).toBeNull();
    // 词之间夹了非空白的字
    expect(segmentLine("夜が明ける", [tok("夜"), tok("明ける")], [])).toBeNull();
    // 注音横跨两个词
    expect(segmentLine("日本語", [tok("日本"), tok("語")], [{ start: 0, end: 3, reading: "にほんご" }])).toBeNull();
  });
});

describe("parseLyricsDisplay", () => {
  it("clamps to the Python ranges and falls back per field", () => {
    expect(parseLyricsDisplay(JSON.stringify({ fontSize: 99, rubySize: 2, lineGap: "x", furiganaMode: "romaji", furigana: true }))).toEqual({
      ...DEFAULT_LYRICS_DISPLAY,
      fontSize: 30,
      rubySize: 6,
      furigana: true,
    });
    expect(parseLyricsDisplay("{broken")).toEqual(DEFAULT_LYRICS_DISPLAY);
    expect(parseLyricsDisplay(null)).toEqual(DEFAULT_LYRICS_DISPLAY);
  });
});
