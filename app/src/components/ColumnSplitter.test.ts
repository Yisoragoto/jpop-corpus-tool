import { describe, expect, it } from "vitest";

import { clampColumns, clampPane, DEFAULT_BOUNDS, type PaneBounds } from "./ColumnSplitter";

const WIDE = 1920;

describe("三栏的宽度", () => {
  it("正常范围内原样放行", () => {
    expect(clampColumns(DEFAULT_BOUNDS, WIDE, "left", { left: 300, right: 360 })).toEqual({
      left: 300,
      right: 360,
    });
  });

  it("拖过上下限就停在限上", () => {
    expect(clampColumns(DEFAULT_BOUNDS, WIDE, "left", { left: 20, right: 320 }).left).toBe(
      DEFAULT_BOUNDS.leftMin,
    );
    expect(clampColumns(DEFAULT_BOUNDS, WIDE, "left", { left: 9999, right: 320 }).left).toBe(
      DEFAULT_BOUNDS.leftMax,
    );
  });

  it("窗口窄到两边挤掉正文时，让步的是正在拖的那一侧", () => {
    const narrow = 900; // 180 + 200 + 360 + 12 = 752，还有余量；拖大就得让
    const left = clampColumns(DEFAULT_BOUNDS, narrow, "left", { left: 560, right: 300 });
    expect(left.right).toBe(300); // 没动的那一侧不动
    expect(left.left).toBeLessThan(560);
    expect(left.left + left.right + 12).toBeLessThanOrEqual(narrow - DEFAULT_BOUNDS.centerMin);

    const right = clampColumns(DEFAULT_BOUNDS, narrow, "right", { left: 300, right: 680 });
    expect(right.left).toBe(300);
    expect(right.right).toBeLessThan(680);
  });

  it("挤到没地方时也不会把那一侧缩到下限以下", () => {
    const tiny = 400;
    const got = clampColumns(DEFAULT_BOUNDS, tiny, "left", { left: 560, right: 320 });
    expect(got.left).toBe(DEFAULT_BOUNDS.leftMin);
  });

  it("还没量到容器宽度时只按上下限夹", () => {
    expect(clampColumns(DEFAULT_BOUNDS, 0, "left", { left: 9999, right: 9999 })).toEqual({
      left: DEFAULT_BOUNDS.leftMax,
      right: DEFAULT_BOUNDS.rightMax,
    });
  });
});

describe("全屏歌词右栏的宽度", () => {
  const bounds: PaneBounds = { min: 260, max: 720, restMin: 420 };

  it("正常范围内原样放行", () => {
    expect(clampPane(bounds, WIDE, 500)).toBe(500);
  });

  it("上下限都收得住", () => {
    expect(clampPane(bounds, WIDE, 10)).toBe(260);
    expect(clampPane(bounds, WIDE, 5000)).toBe(720);
  });

  it("歌词那边要留够 restMin", () => {
    // 1000 - 420 - 6 = 574
    expect(clampPane(bounds, 1000, 720)).toBe(574);
  });

  it("窗口再窄也不低于下限（宁可挤歌词，也不要一条看不懂的缝）", () => {
    expect(clampPane(bounds, 500, 400)).toBe(260);
  });
});
