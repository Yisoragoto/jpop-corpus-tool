import { describe, expect, it } from "vitest";

// 和 macOS 的 KaraokeStyleTests 读同一份：两端字段名、预设、校验规则必须一致
import shared from "../../macos/Tests/JPCoreTests/Fixtures/KaraokeStyle.json";
import {
  applyPreset,
  DEFAULT_KARAOKE_STYLE,
  KARAOKE_PRESETS,
  karaokeCssVars,
  matchingPreset,
  normalizeKaraokeStyle,
  type KaraokeStyle,
} from "./karaokeStyle";

describe("karaoke style shared with macOS", () => {
  it("has the same presets and default", () => {
    expect(KARAOKE_PRESETS).toEqual(shared.presets);
    expect(DEFAULT_KARAOKE_STYLE).toEqual(shared.presets[shared.defaultPreset as keyof typeof shared.presets]);
  });

  it("normalizes the same way", () => {
    for (const c of shared.normalize) {
      const expected = typeof c.output === "string" ? shared.presets[c.output as keyof typeof shared.presets] : c.output;
      expect(normalizeKaraokeStyle(c.input)).toEqual(expected);
    }
  });

  it("uses exactly the shared field names", () => {
    expect(Object.keys(DEFAULT_KARAOKE_STYLE).sort()).toEqual(Object.keys(shared.presets.classic).sort());
  });
});

describe("karaoke style", () => {
  it("survives garbage", () => {
    expect(normalizeKaraokeStyle(null)).toEqual(DEFAULT_KARAOKE_STYLE);
    expect(normalizeKaraokeStyle("x")).toEqual(DEFAULT_KARAOKE_STYLE);
  });

  it("keeps font and size when switching presets", () => {
    const mine: KaraokeStyle = { ...DEFAULT_KARAOKE_STYLE, fontFamily: "Meiryo", fontSize: 72 };
    const pink = applyPreset(mine, "pink");
    expect(pink.fontFamily).toBe("Meiryo");
    expect(pink.fontSize).toBe(72);
    expect(pink.sungFill).toBe(KARAOKE_PRESETS.pink.sungFill);
    expect(matchingPreset(pink)).toBe("pink");
    expect(matchingPreset({ ...pink, sungFill: "#000001" })).toBeNull();
  });

  it("scales stroke, glow and shadow with the font", () => {
    const vars = karaokeCssVars({ ...KARAOKE_PRESETS.classic, fontSize: 50, strokeWidth: 5, shadowDepth: 2, glowStrength: 0 });
    expect(vars["--ktv-stroke"]).toBe("0.2000em");
    expect(vars["--ktv-unsung-shadow"]).toBe("0.0200em 0.0200em 0 #000000, 0.0400em 0.0400em 0 #000000");
    expect(karaokeCssVars(KARAOKE_PRESETS.plain)["--ktv-sung-shadow"]).not.toContain("NaN");
    expect(karaokeCssVars({ ...KARAOKE_PRESETS.plain, glowStrength: 0 })["--ktv-sung-shadow"]).toBe("none");
  });
});
