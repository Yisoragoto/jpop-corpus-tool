import { describe, expect, it } from "vitest";

import type { KwicHit } from "./api";
import { sortKeys, sortKwic } from "./kwicSort";

const hit = (id: number, left: string[], keyword: string, right: string[]): KwicHit => ({
  songId: "001", artist: "A", title: "t", audioPath: "", utteranceId: id, lineIdx: 0, timeSec: null,
  text: "", left: "", keyword, right: "", repeatCount: 1, corrected: false, endSec: null, endKind: null, matchLen: 1,
  leftWords: left, rightWords: right, linePos: 0, songLines: 1,
});

describe("KWIC 排序", () => {
  it("左 2：先比左 2，再往外比左 3，再比里面的左 1，最后比关键词和右边", () => {
    const h = hit(1, ["で", "遠く", "は"], "風", ["が", "鳴る"]);
    expect(sortKeys(h, "L2")).toEqual(["遠く", "は", "", "", "で", "風", "が", "鳴る", "", "", ""]);
    expect(sortKeys(h, "R1")).toEqual(["が", "鳴る", "", "", "", "風", "で", "遠く", "は", "", ""]);
  });

  it("按右 1 排，行尾没有词的排最前，相同时保持原来的顺序", () => {
    const hits = [hit(1, [], "夢", ["を"]), hit(2, [], "夢", []), hit(3, [], "夢", ["が"]), hit(4, ["の"], "夢", ["を"])];
    expect(sortKwic(hits, "R1").map((h) => h.utteranceId)).toEqual([2, 3, 1, 4]);
    expect(sortKwic(hits, "default")).toBe(hits);
  });
});
