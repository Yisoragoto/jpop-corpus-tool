import { describe, expect, it } from "vitest";

import { PITCH_CACHE_LIMITS_MB, PITCH_CACHE_LIMIT_RANGE, limitLabel, sizeLabel, usageLabel } from "./pitchCache";
import { DEFAULT_SETTINGS, parseSettings } from "./settings";

describe("变调缓存的上限", () => {
  it("可选值写成人看的单位", () => {
    expect(PITCH_CACHE_LIMITS_MB.map(limitLabel)).toEqual([
      "512 MB",
      "1 GB",
      "1.5 GB",
      "2 GB",
      "3 GB",
      "5 GB",
      "10 GB",
      "20 GB",
    ]);
  });

  it("默认 1.5 GB，而且默认值就在可选值里", () => {
    expect(DEFAULT_SETTINGS.pitchCacheLimitMb).toBe(1536);
    expect(PITCH_CACHE_LIMITS_MB).toContain(DEFAULT_SETTINGS.pitchCacheLimitMb);
  });

  it("每个可选值后端都肯收", () => {
    for (const mb of PITCH_CACHE_LIMITS_MB) {
      expect(mb).toBeGreaterThanOrEqual(PITCH_CACHE_LIMIT_RANGE[0]);
      expect(mb).toBeLessThanOrEqual(PITCH_CACHE_LIMIT_RANGE[1]);
    }
  });

  it("存坏了的上限回到默认，越界的夹回范围里", () => {
    expect(parseSettings(JSON.stringify({ pitchCacheLimitMb: "很多" })).pitchCacheLimitMb).toBe(1536);
    expect(parseSettings(JSON.stringify({ pitchCacheLimitMb: 1 })).pitchCacheLimitMb).toBe(256);
    expect(parseSettings(JSON.stringify({ pitchCacheLimitMb: 9e9 })).pitchCacheLimitMb).toBe(102400);
    expect(parseSettings(JSON.stringify({ pitchCacheLimitMb: 5120 })).pitchCacheLimitMb).toBe(5120);
    // 旧版本存下来的设置里没有这一项
    expect(parseSettings(JSON.stringify({ spectrum: false })).pitchCacheLimitMb).toBe(1536);
  });
});

describe("变调缓存的占用", () => {
  it("空的时候说还没有", () => {
    expect(usageLabel({ files: 0, bytes: 0 })).toBe("还没有缓存");
  });

  it("不到 1 GB 用 MB，往上用 GB", () => {
    expect(sizeLabel(45 * 1024 * 1024)).toBe("45 MB");
    expect(sizeLabel(1536 * 1024 * 1024)).toBe("1.5 GB");
    expect(usageLabel({ files: 7, bytes: 312 * 1024 * 1024 })).toBe("已用 312 MB · 7 个");
  });
});
