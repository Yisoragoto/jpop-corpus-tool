import { describe, expect, it } from "vitest";

import { listStep } from "./listKeys";

describe("列表里的方向键", () => {
  it("↑↓ 走一项，到头不绕回", () => {
    expect(listStep("ArrowDown", 0, 3)).toBe(1);
    expect(listStep("ArrowUp", 2, 3)).toBe(1);
    expect(listStep("ArrowDown", 2, 3)).toBeNull();
    expect(listStep("ArrowUp", 0, 3)).toBeNull();
  });

  it("Home / End 去两头", () => {
    expect(listStep("Home", 2, 3)).toBe(0);
    expect(listStep("End", 0, 3)).toBe(2);
    expect(listStep("Home", 0, 3)).toBeNull();
  });

  it("别的键、空列表、焦点不在列表项上：不管", () => {
    expect(listStep("Enter", 0, 3)).toBeNull();
    expect(listStep("ArrowDown", 0, 0)).toBeNull();
    expect(listStep("ArrowDown", -1, 3)).toBeNull();
  });
});
