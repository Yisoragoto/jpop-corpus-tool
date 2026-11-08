import { describe, expect, it } from "vitest";

import { formatLoopTime } from "./components/LoopAdjustBar";
import { activeLineLoop, loopKeyChange, type LoopRegion } from "./lineLoop";

const key = (code: string, shiftKey = false, altKey = false) => ({ code, shiftKey, altKey });

describe("循环快捷键", () => {
  it("[ ] 挪起点，Shift 挪终点，Alt 步长 0.5 秒", () => {
    expect(loopKeyChange(key("BracketLeft"))).toEqual({ startMs: -100 });
    expect(loopKeyChange(key("BracketRight"))).toEqual({ startMs: 100 });
    expect(loopKeyChange(key("BracketLeft", true))).toEqual({ endMs: -100 });
    expect(loopKeyChange(key("BracketRight", true, true))).toEqual({ endMs: 500 });
    expect(loopKeyChange(key("KeyL"))).toBeNull();
  });
});

describe("当前循环", () => {
  const region: LoopRegion = {
    songId: "001", firstUtteranceId: 1, lastUtteranceId: 2, startSec: 10, endSec: 14.2,
    baseStartSec: 10, baseEndSec: 14, startDeltaMs: 0, endDeltaMs: 200, endFromWords: true,
  };

  it("播放器循环的就是这一段才算", () => {
    expect(activeLineLoop(region, { startSec: 10, endSec: 14.2 }, "001")).toBe(region);
    // 被别处取消了、换了一段、换了歌
    expect(activeLineLoop(region, null, "001")).toBeNull();
    expect(activeLineLoop(region, { startSec: 20, endSec: 24 }, "001")).toBeNull();
    expect(activeLineLoop(region, { startSec: 10, endSec: 14.2 }, "002")).toBeNull();
  });

  it("时间显示到 0.1 秒", () => {
    expect(formatLoopTime(0)).toBe("0:00.0");
    expect(formatLoopTime(72.34)).toBe("1:12.3");
    expect(formatLoopTime(59.96)).toBe("1:00.0");
  });
});
