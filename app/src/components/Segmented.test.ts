import { describe, expect, it } from "vitest";

import { nextEnabled } from "./Segmented";

describe("分段按钮的方向键", () => {
  it("往后走，到头绕回第一个", () => {
    expect(nextEnabled([false, false, false], 0, 1)).toBe(1);
    expect(nextEnabled([false, false, false], 2, 1)).toBe(0);
  });

  it("往前走，到头绕回最后一个", () => {
    expect(nextEnabled([false, false, false], 0, -1)).toBe(2);
  });

  it("跳过禁用的项", () => {
    expect(nextEnabled([false, true, false], 0, 1)).toBe(2);
    expect(nextEnabled([false, true, true], 0, 1)).toBe(0);
  });

  it("Home / End：从两头外面起步，落在第一个 / 最后一个能选的上", () => {
    expect(nextEnabled([true, false, false], -1, 1)).toBe(1);
    expect(nextEnabled([false, false, true], 3, -1)).toBe(1);
  });

  it("全都禁用：没有可去的", () => {
    expect(nextEnabled([true, true], 0, 1)).toBe(-1);
  });
});
