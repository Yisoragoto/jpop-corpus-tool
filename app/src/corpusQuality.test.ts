import { describe, expect, it } from "vitest";

import {
  completenessLabel,
  describeFilter,
  isWholeCorpus,
  ISSUE_LABELS,
  metricLabel,
  normalizeFilter,
  sameFilter,
  tokenizerLabel,
} from "./corpusQuality";

const base = { performerIds: [], jpOnly: false };

describe("describeFilter", () => {
  it("says 整个库 when nothing narrows the scope", () => {
    expect(describeFilter(base)).toBe("整个库");
    expect(isWholeCorpus(base)).toBe(true);
  });

  it("joins every condition and keeps method options apart", () => {
    const f = {
      performerIds: [3],
      jpOnly: true,
      yearFrom: 2010,
      yearTo: 2019,
      genres: ["J-Pop"],
      languages: ["ja"],
      minCompleteness: 0.8,
      distinctLines: true,
    };
    expect(describeFilter(f, ["YOASOBI"])).toBe(
      "YOASOBI · 2010–2019 年 · J-Pop · 日文 · 歌词完整度 ≥ 80%（仅日文，副歌只算一遍）",
    );
    expect(isWholeCorpus(f)).toBe(false);
  });

  it("falls back to a count when performer names are unknown, and handles one-sided year ranges", () => {
    expect(describeFilter({ ...base, performerIds: [1, 2] })).toBe("2 位演唱者");
    expect(describeFilter({ ...base, yearFrom: 2020 })).toBe("2020 年起");
    expect(describeFilter({ ...base, yearTo: 1999 })).toBe("1999 年前");
    expect(describeFilter({ ...base, yearFrom: 2001, yearTo: 2001 })).toBe("2001 年");
  });

  it("jpOnly alone is a method, not a narrower scope", () => {
    expect(isWholeCorpus({ ...base, jpOnly: true })).toBe(true);
    expect(describeFilter({ ...base, jpOnly: true })).toBe("整个库（仅日文）");
  });
});

describe("sameFilter", () => {
  it("ignores empty arrays, nulls and performer order", () => {
    expect(sameFilter({ performerIds: [2, 1], jpOnly: false, genres: [], yearFrom: null }, { performerIds: [1, 2], jpOnly: false })).toBe(true);
    expect(sameFilter(base, { ...base, distinctLines: true })).toBe(false);
    expect(normalizeFilter({ ...base, distinctLines: false })).toEqual(base);
  });
});

describe("labels", () => {
  it("covers every issue code the Rust validator emits", () => {
    // 和 jp-corpus/src/quality.rs 的 ISSUE_CODES 一一对应
    const codes = [
      "empty", "replacement_char", "not_japanese", "too_few_lines", "ends_early", "untimed", "mixed_timing",
      "non_monotonic", "beyond_duration", "duplicate_timestamp", "stacked_duplicate", "long_gap",
      "leftover_markup", "very_long_line",
    ];
    expect(Object.keys(ISSUE_LABELS).sort()).toEqual(codes.sort());
  });

  it("grades completeness", () => {
    expect(completenessLabel(null)).toBe("未校验");
    expect(completenessLabel(1)).toBe("完整");
    expect(completenessLabel(0.85)).toBe("基本完整");
    expect(completenessLabel(0.6)).toBe("可能有缺");
    expect(completenessLabel(0.2)).toBe("很可能有问题");
  });

  it("names metrics and tokenizers", () => {
    expect(metricLabel("ttr")).toBe("TTR");
    expect(metricLabel("coverage@500")).toBe("Top 500 覆盖率");
    expect(
      tokenizerLabel({
        engine: "sudachi.rs", engineVersion: "0.6.11", dictDescription: "", dictCreatedAt: "", dictSize: 0,
        splitMode: "C", configHash: "", posScheme: "upos-1",
      }),
    ).toBe("sudachi.rs 0.6.11 · 未知词典 · Mode C");
  });
});
