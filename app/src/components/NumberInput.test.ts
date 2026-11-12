import { describe, expect, it } from "vitest";

import { clampDraft } from "./NumberInput";
import { yearDraftOk } from "./YearInput";

describe("整数输入框的草稿", () => {
  it("范围里的整数原样", () => {
    expect(clampDraft("3", 1, 8)).toBe(3);
    expect(clampDraft(" 8 ", 1, 8)).toBe(8);
  });

  it("超出范围收回边界", () => {
    expect(clampDraft("0", 1, 8)).toBe(1);
    expect(clampDraft("99", 1, 8)).toBe(8);
    expect(clampDraft("-5", 0, 20)).toBe(0);
  });

  it("空的、不是整数的不算数（调用方回到原来的值）", () => {
    expect(clampDraft("", 1, 8)).toBeNull();
    expect(clampDraft("-", 1, 8)).toBeNull();
    expect(clampDraft("1.5", 1, 8)).toBeNull();
    expect(clampDraft("1e3", 1, 8)).toBeNull();
  });
});

describe("年份输入框的草稿", () => {
  it("空（不限）和四位数字能提交", () => {
    expect(yearDraftOk("")).toBe(true);
    expect(yearDraftOk("2010")).toBe(true);
  });

  it("写了一半的不能", () => {
    expect(yearDraftOk("2")).toBe(false);
    expect(yearDraftOk("201")).toBe(false);
  });
});
