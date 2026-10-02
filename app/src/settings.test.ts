import { describe, expect, it } from "vitest";

import { DEFAULT_SETTINGS, parseSettings, STAGE_BLUR_LIMITS } from "./settings";

describe("应用设置", () => {
  it("没存过、存坏了、少字段，都退回默认值", () => {
    expect(parseSettings(null)).toEqual(DEFAULT_SETTINGS);
    expect(parseSettings("这不是 JSON")).toEqual(DEFAULT_SETTINGS);
    expect(parseSettings("{}")).toEqual(DEFAULT_SETTINGS);
    // 旧版本存下的、只有一半字段的，认识的那部分要留住
    expect(parseSettings(JSON.stringify({ gridByArtist: false }))).toEqual({
      ...DEFAULT_SETTINGS,
      gridByArtist: false,
    });
  });

  it("类型不对的字段逐个退回默认值，不整份丢掉", () => {
    const got = parseSettings(
      JSON.stringify({ stageCoverBlur: "yes", stageBlurRadius: 40, gridByArtist: false, spectrum: 0 }),
    );
    expect(got.stageCoverBlur).toBe(DEFAULT_SETTINGS.stageCoverBlur);
    expect(got.stageBlurRadius).toBe(40);
    expect(got.gridByArtist).toBe(false);
    // 0 不是 false，坏值退回默认（开着）
    expect(got.spectrum).toBe(DEFAULT_SETTINGS.spectrum);
    expect(parseSettings(JSON.stringify({ spectrum: false })).spectrum).toBe(false);
  });

  it("虚化强度夹在可调范围里", () => {
    const [low, high] = STAGE_BLUR_LIMITS;
    expect(parseSettings(JSON.stringify({ stageBlurRadius: -10 })).stageBlurRadius).toBe(low);
    expect(parseSettings(JSON.stringify({ stageBlurRadius: 9999 })).stageBlurRadius).toBe(high);
    // 手改成小数的也认，取整
    expect(parseSettings(JSON.stringify({ stageBlurRadius: 63.6 })).stageBlurRadius).toBe(64);
    // NaN / 字符串这类退回默认值
    expect(parseSettings(JSON.stringify({ stageBlurRadius: "64" })).stageBlurRadius).toBe(DEFAULT_SETTINGS.stageBlurRadius);
  });
});
