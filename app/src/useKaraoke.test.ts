import { describe, expect, it } from "vitest";

import { advanceClock } from "./useKaraoke";

describe("advanceClock", () => {
  it("在播：两次轮询之间按倍速自己往前走", () => {
    const polled = { pos: 10, at: 0 };
    const a = advanceClock(null, polled, 0, true, 1);
    expect(a.sec).toBe(10);
    const b = advanceClock(a, polled, 50, true, 1);
    expect(b.sec).toBeCloseTo(10.05, 9);
    expect(advanceClock(a, polled, 50, true, 0.5).sec).toBeCloseTo(10.025, 9);
  });

  it("轮询位置比显示的略早（抖动）：不往回退，停一下等它追上来", () => {
    const shown = { sec: 10.3, at: 1000 };
    const next = advanceClock(shown, { pos: 10.1, at: 1000 }, 1016, true, 1);
    expect(next.sec).toBe(10.3);
    // 轮询那边接着走，过了 10.3 之后又跟着走
    expect(advanceClock(next, { pos: 10.1, at: 1000 }, 1300, true, 1).sec).toBeCloseTo(10.4, 9);
  });

  it("轮询位置略晚：几帧之内追上去，不是一下跳过去", () => {
    let clock = { sec: 10, at: 0 };
    const polled = { pos: 10.3, at: 0 };
    clock = advanceClock(clock, polled, 16, true, 1);
    expect(clock.sec).toBeLessThan(10.2);
    for (let t = 32; t <= 800; t += 16) clock = advanceClock(clock, polled, t, true, 1);
    // 轮询那边也在按倍速走：800ms 后它在 10.3 + 0.5（推算封顶）附近
    expect(clock.sec).toBeGreaterThan(10.7);
  });

  it("差得多（拖了进度条、往回跳）：直接跳过去", () => {
    const shown = { sec: 60, at: 0 };
    expect(advanceClock(shown, { pos: 30, at: 0 }, 16, true, 1).sec).toBeCloseTo(30.016, 9);
    expect(advanceClock(shown, { pos: 90, at: 0 }, 16, true, 1).sec).toBeCloseTo(90.016, 9);
  });

  it("轮询停了：最多往前推 0.5 秒", () => {
    let clock = advanceClock(null, { pos: 10, at: 0 }, 0, true, 1);
    for (let t = 16; t <= 5000; t += 16) clock = advanceClock(clock, { pos: 10, at: 0 }, t, true, 1);
    expect(clock.sec).toBeLessThan(10.6);
  });

  it("暂停：就是轮询位置", () => {
    expect(advanceClock({ sec: 12, at: 0 }, { pos: 11, at: 0 }, 500, false, 1).sec).toBe(11);
  });
});
