import { describe, expect, it } from "vitest";

import { isPlayingSpan } from "./listen";

const playing = { songId: "001", playState: "playing" as const, positionSec: 12 };

describe("isPlayingSpan", () => {
  it("同一首、正在放、位置落在区间里才算", () => {
    expect(isPlayingSpan(playing, "001", 10, 14)).toBe(true);
    expect(isPlayingSpan(playing, "002", 10, 14)).toBe(false);
    expect(isPlayingSpan({ ...playing, playState: "paused" }, "001", 10, 14)).toBe(false);
    expect(isPlayingSpan(playing, "001", 13, 20)).toBe(false);
    expect(isPlayingSpan(playing, "001", 5, 12)).toBe(false);
  });

  it("起播有提前量，区间开始前半秒也算在放；没有时间戳的行永远不亮", () => {
    expect(isPlayingSpan(playing, "001", 12.4, 15)).toBe(true);
    expect(isPlayingSpan(playing, "001", null, null)).toBe(false);
  });
});
