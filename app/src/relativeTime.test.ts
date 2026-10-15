import { describe, expect, it } from "vitest";

import { formatRelative } from "./relativeTime";

describe("相对时间", () => {
  const now = Date.parse("2026-10-15T12:00:00Z");
  it("从刚刚到几个月前", () => {
    expect(formatRelative("2026-10-15T11:59:30Z", now)).toBe("刚刚");
    expect(formatRelative("2026-10-15T11:30:00Z", now)).toBe("30 分钟前");
    expect(formatRelative("2026-10-15T02:00:00Z", now)).toBe("10 小时前");
    expect(formatRelative("2026-10-13T15:44:00Z", now)).toBe("1 天前");
    expect(formatRelative("2026-06-02T16:31:24Z", now)).toBe("4 个月前");
  });
  it("坏的时间串给空串，不给 NaN", () => {
    expect(formatRelative("", now)).toBe("");
    expect(formatRelative("昨天", now)).toBe("");
  });
});
