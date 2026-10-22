import { describe, expect, it } from "vitest";

import { advanceClock, paintKaraoke } from "./useKaraoke";
import { charTimeline } from "./wordTiming";

function character(index: string) {
  const classes = new Set<string>();
  const properties = new Map<string, string>();
  const element = {
    dataset: { c: index },
    classList: { toggle: (name: string, enabled: boolean) => { if (enabled) classes.add(name); else classes.delete(name); } },
    style: { setProperty: (name: string, value: string) => { properties.set(name, value); }, removeProperty: (name: string) => { properties.delete(name); } },
  } as unknown as HTMLElement;
  return { element, classes, properties };
}

describe("paintKaraoke", () => {
  const timeline = charTimeline("𠮷野家", 0, 4, [
    { utteranceId: 1, charStart: 0, charEnd: 2, startSec: 0, endSec: 2, source: "lrc" },
    { utteranceId: 1, charStart: 2, charEnd: 3, startSec: 3, endSec: 4, source: "lrc" },
  ])!;
  it("paints the entire multi-character source unit with identical progress", () => {
    const nodes = [character("0"), character("1"), character("2")];
    paintKaraoke(timeline, 1, nodes.map((node) => node.element));
    for (const node of nodes.slice(0, 2)) {
      expect(node.classes.has("kc-now")).toBe(true);
      expect(node.properties.get("--p")).toBe("50.0%");
    }
    expect(nodes[2]!.classes.size).toBe(0);
  });
  it("does not fill gaps between real source units", () => {
    const nodes = [character("0"), character("1"), character("2")];
    paintKaraoke(timeline, 2.5, nodes.map((node) => node.element));
    expect(nodes.map((node) => node.classes.has("kc-on"))).toEqual([true, true, false]);
    expect(nodes.some((node) => node.classes.has("kc-now"))).toBe(false);
  });
  it("line-synced/plain fallback removes previous karaoke styles instead of animating", () => {
    const node = character("0");
    paintKaraoke(timeline, 1, [node.element]);
    paintKaraoke(null, 99, [node.element]);
    expect(node.classes.size).toBe(0);
    expect(node.properties.size).toBe(0);
  });
  it("seeking backwards, including a repeated line, does not retain the old progress", () => {
    const node = character("0");
    paintKaraoke(timeline, 10, [node.element]);
    expect(node.classes.has("kc-on")).toBe(true);
    paintKaraoke(timeline, -1, [node.element]);
    expect(node.classes.size).toBe(0);
    expect(node.properties.size).toBe(0);
  });
  it("ignores malformed or out-of-range DOM character offsets", () => {
    const nodes = [character("NaN"), character("-1"), character("9"), character("0.5")];
    paintKaraoke(timeline, 1, nodes.map((node) => node.element));
    expect(nodes.every((node) => node.classes.size === 0)).toBe(true);
  });
});

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
