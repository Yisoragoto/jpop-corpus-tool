import { describe, expect, it } from "vitest";

import { formatStamp, parseStamp, summarizeEdit } from "./lyricEdit";

describe("stamps", () => {
  it("formats like LRC, to the centisecond", () => {
    expect(formatStamp(3.1)).toBe("0:03.10");
    expect(formatStamp(61.255)).toBe("1:01.26");
    expect(formatStamp(null)).toBe("");
  });

  it("reads the usual spellings and refuses the rest", () => {
    expect(parseStamp("1:02.5")).toBe(62.5);
    expect(parseStamp("[01:02.50]")).toBe(62.5);
    expect(parseStamp("01:02:50")).toBe(62.5);
    expect(parseStamp("62.25")).toBe(62.25);
    expect(parseStamp("  ")).toBeNull();
    expect(parseStamp("1:75")).toBeUndefined();
    expect(parseStamp("abc")).toBeUndefined();
  });

  it("round-trips", () => {
    for (const t of [0, 3.1, 59.99, 61.25, 3599.5]) expect(parseStamp(formatStamp(t))).toBe(t);
  });
});

describe("summarizeEdit", () => {
  const before = [
    { timeSec: 1, text: "夜に駆けるる" },
    { timeSec: 1, text: "夜に駆けるる" },
    { timeSec: 2, text: "さよなら" },
  ];

  it("says nothing changed when nothing did (blank rows ignored)", () => {
    expect(summarizeEdit(before, [...before, { timeSec: null, text: " " }])).toBe("没有改动");
  });

  it("counts rewritten and removed lines", () => {
    const after = [
      { timeSec: 1, text: "夜に駆ける" },
      { timeSec: 2, text: "さよなら" },
    ];
    expect(summarizeEdit(before, after)).toBe("新增或改写 1 行，去掉或改写前 2 行（3 → 2 行）");
  });

  it("notices retimed lines", () => {
    const after = before.map((r, i) => (i === 2 ? { ...r, timeSec: 2.5 } : r));
    expect(summarizeEdit(before, after)).toBe("调了 1 行的时间（3 → 3 行）");
  });
});
