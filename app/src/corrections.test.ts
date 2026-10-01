import { describe, expect, it } from "vitest";

import type { TokenEdit } from "./api";
import {
  charCount,
  cleanTokens,
  isContiguous,
  matchesLine,
  mergeTokens,
  sameTokens,
  splitToken,
} from "./corrections";

const t = (surface: string, lemma = surface, pos = "NOUN"): TokenEdit => ({ surface, lemma, pos });

const yoru = [t("夜"), t("が", "が", "ADP"), t("明ける", "明ける", "VERB")];

describe("isContiguous", () => {
  it("needs at least two adjacent indices", () => {
    expect(isContiguous([1])).toBe(false);
    expect(isContiguous([0, 1])).toBe(true);
    expect(isContiguous([2, 0, 1])).toBe(true);
    expect(isContiguous([0, 2])).toBe(false);
    expect(isContiguous([1, 1])).toBe(false);
  });
});

describe("mergeTokens", () => {
  it("joins surfaces and keeps the first token's lemma and pos", () => {
    expect(mergeTokens(yoru, [0, 1])).toEqual([t("夜が", "夜", "NOUN"), yoru[2]]);
  });

  it("leaves tokens alone when the selection has a gap", () => {
    expect(mergeTokens(yoru, [0, 2])).toBe(yoru);
  });
});

describe("splitToken", () => {
  it("splits by character and reuses each half as its lemma", () => {
    expect(splitToken([t("明ける", "明ける", "VERB")], 0, 1)).toEqual([
      t("明", "明", "VERB"),
      t("ける", "ける", "VERB"),
    ]);
  });

  it("counts code points, not UTF-16 units", () => {
    // 𠮷 是扩展区汉字，在 JS 字符串里占两个码元
    expect(charCount("𠮷野家")).toBe(3);
    expect(splitToken([t("𠮷野家")], 0, 1)).toEqual([t("𠮷"), t("野家")]);
  });

  it("refuses to produce an empty half", () => {
    const one = [t("夜空")];
    expect(splitToken(one, 0, 0)).toBe(one);
    expect(splitToken(one, 0, 2)).toBe(one);
    expect(splitToken(one, 5, 1)).toBe(one);
  });
});

describe("cleanTokens", () => {
  it("trims, drops blanks and fills defaults like the Python editor", () => {
    expect(cleanTokens([t(" 夜 ", "", ""), t("   ", "x"), t("が明ける", "", "VERB")])).toEqual([
      t("夜", "夜", "NOUN"),
      t("が明ける", "が明ける", "VERB"),
    ]);
  });
});

describe("sameTokens / matchesLine", () => {
  it("compares every field", () => {
    expect(sameTokens(yoru, [...yoru])).toBe(true);
    expect(sameTokens(yoru, [t("夜"), t("が", "が", "PART"), yoru[2]!])).toBe(false);
  });

  it("ignores the spaces lyrics use as phrasing", () => {
    expect(matchesLine([t("初めて"), t("の"), t("色")], "初めて の色")).toBe(true);
    expect(matchesLine([t("初めて")], "初めての色")).toBe(false);
  });
});
