import { describe, expect, it } from "vitest";

import { backfillNote } from "./backfill";

const none = { songs: 0, lines: 0, tokens: 0, correctionsRestored: 0 };

describe("backfillNote", () => {
  it("没有要补的就不说话", () => {
    expect(backfillNote(none, "")).toBe("");
  });

  it("补了几首要说出来", () => {
    const text = backfillNote({ songs: 16, lines: 600, tokens: 4200, correctionsRestored: 0 }, "");
    expect(text).toContain("16 首");
    expect(text).toContain("振假名");
  });

  it("失败要说原因，并指出去哪儿重试", () => {
    const text = backfillNote(none, "分词失败：…");
    expect(text).toContain("分词失败：…");
    expect(text).toContain("补齐缺失分词");
  });
});
