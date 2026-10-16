import { describe, expect, it } from "vitest";

import { DAILY_GOAL_LIMITS, DEFAULT_SETTINGS, parseSettings, STAGE_BLUR_LIMITS } from "./settings";

describe("应用设置", () => {
  it("没存过、存坏了、少字段，都退回默认值", () => {
    expect(parseSettings(null)).toEqual(DEFAULT_SETTINGS);
    expect(parseSettings("这不是 JSON")).toEqual(DEFAULT_SETTINGS);
    expect(parseSettings("{}")).toEqual(DEFAULT_SETTINGS);
    // 旧版本存下的、只有一半字段的，认识的那部分要留住
    expect(parseSettings(JSON.stringify({ autoCheckNewSongs: false }))).toEqual({
      ...DEFAULT_SETTINGS,
      autoCheckNewSongs: false,
    });
  });

  it("类型不对的字段逐个退回默认值，不整份丢掉", () => {
    const got = parseSettings(
      JSON.stringify({ stageCoverBlur: "yes", stageBlurRadius: 40, autoCheckNewSongs: false, spectrum: 0 }),
    );
    expect(got.stageCoverBlur).toBe(DEFAULT_SETTINGS.stageCoverBlur);
    expect(got.stageBlurRadius).toBe(40);
    expect(got.autoCheckNewSongs).toBe(false);
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
  it("每日收听目标夹在可调范围里", () => {
    const [low, high] = DAILY_GOAL_LIMITS;
    expect(parseSettings(JSON.stringify({ dailyGoalMinutes: 0 })).dailyGoalMinutes).toBe(low);
    expect(parseSettings(JSON.stringify({ dailyGoalMinutes: 999 })).dailyGoalMinutes).toBe(high);
    expect(parseSettings(JSON.stringify({ dailyGoalMinutes: "30" })).dailyGoalMinutes).toBe(DEFAULT_SETTINGS.dailyGoalMinutes);
  });
  it("词云字体：存的是字体名，空串表示界面字体，坏值退回默认", () => {
    expect(parseSettings(JSON.stringify({ wordCloudFont: " LXGW WenKai " })).wordCloudFont).toBe("LXGW WenKai");
    expect(parseSettings(JSON.stringify({ wordCloudFont: "" })).wordCloudFont).toBe("");
    expect(parseSettings(JSON.stringify({ wordCloudFont: 3 })).wordCloudFont).toBe(DEFAULT_SETTINGS.wordCloudFont);
    expect(parseSettings(JSON.stringify({ wordCloudFont: "x".repeat(500) })).wordCloudFont).toBe(DEFAULT_SETTINGS.wordCloudFont);
  });
});

describe("词云词数", () => {
  it("夹在可调范围里，坏值用默认", () => {
    expect(parseSettings(JSON.stringify({ wordCloudCount: 5 })).wordCloudCount).toBe(30);
    expect(parseSettings(JSON.stringify({ wordCloudCount: 9999 })).wordCloudCount).toBe(250);
    expect(parseSettings(JSON.stringify({ wordCloudCount: "多" })).wordCloudCount).toBe(DEFAULT_SETTINGS.wordCloudCount);
  });
});
