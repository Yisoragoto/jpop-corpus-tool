import { describe, expect, it } from "vitest";

import { dictKind, dropIndex, reorderWithin } from "./dictKinds";

const terms = (id: number) => ({ id, counts: { terms: 10, termMeta: {} } });
const freq = (id: number) => ({ id, counts: { terms: 0, termMeta: { freq: 5 } } });
const pitch = (id: number) => ({ id, counts: { terms: 0, termMeta: { pitch: 5 } } });

describe("dictKind", () => {
  it("有词条就是释义，哪怕也带词频", () => {
    expect(dictKind({ counts: { terms: 3, termMeta: { freq: 9 } } } as never)).toBe("terms");
    expect(dictKind(freq(1) as never)).toBe("freq");
    expect(dictKind(pitch(1) as never)).toBe("pitch");
    expect(dictKind({ counts: { terms: 0, termMeta: { ipa: 2 } } } as never)).toBe("pitch");
    expect(dictKind({ counts: { terms: 0 } } as never)).toBe("other");
  });
});

describe("reorderWithin", () => {
  // 真实库里就是这样交错的：释义中间夹着音调和词频
  const all = [terms(1), terms(2), pitch(3), terms(4), freq(5), terms(6), freq(7)] as never[];
  const ids = (list: { id: number }[]) => list.map((d) => d.id);

  it("只在同类占的位置之间换，别的类原地不动", () => {
    expect(ids(reorderWithin(all, "terms", 3, 0))).toEqual([6, 1, 3, 2, 5, 4, 7]);
    expect(ids(reorderWithin(all, "terms", 0, 2))).toEqual([2, 4, 3, 1, 5, 6, 7]);
    expect(ids(reorderWithin(all, "freq", 1, 0))).toEqual([1, 2, 3, 4, 7, 6, 5]);
  });

  it("没动或越界就原样返回", () => {
    expect(reorderWithin(all, "terms", 1, 1)).toBe(all);
    expect(reorderWithin(all, "pitch", 0, 1)).toBe(all);
    expect(reorderWithin(all, "terms", -1, 0)).toBe(all);
  });
});

describe("dropIndex", () => {
  const rects = [0, 50, 100, 150].map((top) => ({ top, height: 44 }));
  it("中心越过几行中线就落在第几位", () => {
    expect(dropIndex(rects, 0, 0)).toBe(0);
    expect(dropIndex(rects, 0, 60)).toBe(1);
    expect(dropIndex(rects, 0, 160)).toBe(3);
    expect(dropIndex(rects, 3, -60)).toBe(2);
    expect(dropIndex(rects, 3, -400)).toBe(0);
  });
});
