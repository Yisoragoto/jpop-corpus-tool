import { describe, expect, it } from "vitest";

import { computeWindow, scrollOffsetFor } from "./virtual";

const base = { count: 1000, itemHeight: 24, viewportHeight: 480, scrollTop: 0, overscan: 3 };

describe("computeWindow", () => {
  it("从顶部开始时只渲染视口那一段", () => {
    const w = computeWindow(base);
    expect(w.startIndex).toBe(0);
    // 480/24 = 20 项 + 1（顶部半截）+ 3（余量）
    expect(w.endIndex).toBe(24);
    expect(w.paddingTop).toBe(0);
  });

  it("撑开的高度加上渲染的行数正好是总高", () => {
    const w = computeWindow({ ...base, scrollTop: 5000 });
    const rendered = (w.endIndex - w.startIndex) * base.itemHeight;
    expect(w.paddingTop + rendered + w.paddingBottom).toBe(w.totalHeight);
  });

  it("滚动后窗口跟着移动", () => {
    const w = computeWindow({ ...base, scrollTop: 2400 }); // 第 100 项
    expect(w.startIndex).toBe(97); // 100 - 3 余量
    expect(w.paddingTop).toBe(97 * 24);
  });

  it("留了上下余量，避免快速滚动时露白", () => {
    const w = computeWindow({ ...base, scrollTop: 2400, overscan: 5 });
    expect(w.startIndex).toBe(95);
  });

  it("空列表不产生越界下标", () => {
    const w = computeWindow({ ...base, count: 0 });
    expect(w).toEqual({
      startIndex: 0,
      endIndex: 0,
      paddingTop: 0,
      paddingBottom: 0,
      totalHeight: 0,
    });
  });

  it("列表短于视口时全部渲染", () => {
    const w = computeWindow({ ...base, count: 5 });
    expect(w.startIndex).toBe(0);
    expect(w.endIndex).toBe(5);
    expect(w.paddingBottom).toBe(0);
  });

  it("负的滚动位置（橡皮筋效果）不会越界", () => {
    const w = computeWindow({ ...base, scrollTop: -200 });
    expect(w.startIndex).toBe(0);
    expect(w.paddingTop).toBe(0);
  });

  it("滚过头也不会越界", () => {
    const w = computeWindow({ ...base, scrollTop: 999_999 });
    expect(w.endIndex).toBeLessThanOrEqual(base.count);
    expect(w.startIndex).toBeLessThanOrEqual(w.endIndex);
    expect(w.paddingBottom).toBeGreaterThanOrEqual(0);
  });

  it("滚到底时最后一项在渲染范围内", () => {
    const total = base.count * base.itemHeight;
    const w = computeWindow({ ...base, scrollTop: total - base.viewportHeight });
    expect(w.endIndex).toBe(base.count);
  });

  it("行高为 0 或负数时不会除以零", () => {
    for (const itemHeight of [0, -10]) {
      const w = computeWindow({ ...base, itemHeight });
      expect(Number.isFinite(w.totalHeight)).toBe(true);
      expect(w.startIndex).toBeGreaterThanOrEqual(0);
    }
  });

  it("视口高度为 0 时至少渲染一项", () => {
    // 首帧拿不到容器高度是常态，这时不该渲染成空白
    const w = computeWindow({ ...base, viewportHeight: 0 });
    expect(w.endIndex).toBeGreaterThan(w.startIndex);
  });

  it("窗口始终是有效区间", () => {
    for (const scrollTop of [0, 1, 999, 12_000, 23_999, 24_000]) {
      const w = computeWindow({ ...base, scrollTop });
      expect(w.startIndex).toBeGreaterThanOrEqual(0);
      expect(w.endIndex).toBeLessThanOrEqual(base.count);
      expect(w.startIndex).toBeLessThanOrEqual(w.endIndex);
    }
  });
});

describe("scrollOffsetFor", () => {
  const view = { itemHeight: 24, viewportHeight: 480, scrollTop: 0 };

  it("已经在视野里就不滚动", () => {
    // 无条件滚动会在用户手动浏览时把位置抢走
    expect(scrollOffsetFor(5, view)).toBeNull();
  });

  it("目标在上方时滚到它的顶部", () => {
    expect(scrollOffsetFor(2, { ...view, scrollTop: 1000 })).toBe(48);
  });

  it("目标在下方时只滚到刚好露出它", () => {
    // 不是滚到顶部——那会让视野跳一大截
    expect(scrollOffsetFor(30, view)).toBe(31 * 24 - 480);
  });

  it("居中对齐", () => {
    expect(scrollOffsetFor(50, view, "center")).toBe(50 * 24 - (480 - 24) / 2);
  });

  it("居中对齐在列表开头不会给出负值", () => {
    expect(scrollOffsetFor(0, view, "center")).toBe(0);
  });

  it("顶部对齐总是给出确定值", () => {
    expect(scrollOffsetFor(10, view, "start")).toBe(240);
  });
});
