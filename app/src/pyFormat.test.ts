import { describe, expect, it } from "vitest";

import fixtures from "./testdata/python_formats.json";
import { kwicCsv, pyFixed, pyFloatRepr, pyGrouped } from "./pyFormat";

describe("和 Python 的格式化逐条对", () => {
  it("定点小数：正好一半时取偶", () => {
    const wrong = (fixtures.fixed as [string, number, string][])
      .map(([repr, digits, python]) => ({ repr, digits, python, ours: pyFixed(Number(repr), digits) }))
      .filter((c) => c.ours !== c.python);
    expect(wrong).toEqual([]);
    expect(fixtures.fixed.length).toBeGreaterThan(2000);
    // 例子：JS 的 toFixed 在这几处和 Python 不同
    expect([pyFixed(0.125, 2), pyFixed(2.5, 0), pyFixed(3.5, 0), pyFixed(24.25, 1)]).toEqual(["0.12", "2", "4", "24.2"]);
  });

  it("千分位", () => {
    for (const [n, python] of fixtures.grouped as [number, string][]) {
      expect(pyGrouped(n)).toBe(python);
    }
  });

  it("浮点数的 str()", () => {
    expect([pyFloatRepr(60), pyFloatRepr(0), pyFloatRepr(12.34), pyFloatRepr(5e-5), pyFloatRepr(0.1 + 0.2)]).toEqual([
      "60.0",
      "0.0",
      "12.34",
      "5e-05",
      "0.30000000000000004",
    ]);
  });

  it("检索结果 CSV 和 csv.writer 写出的逐字符相同", () => {
    expect(kwicCsv(fixtures.kwicRows)).toBe(fixtures.kwicCsv);
  });
});
