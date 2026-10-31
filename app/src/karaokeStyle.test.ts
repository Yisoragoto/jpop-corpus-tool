import { describe, expect, it } from "vitest";

// 和 macOS 的 KaraokeStyleTests 读同一份：两端字段名、预设、校验规则必须一致
import shared from "../../macos/Tests/JPCoreTests/Fixtures/KaraokeStyle.json";
import {
  applyPreset,
  DEFAULT_KARAOKE_STYLE,
  extrusionSteps,
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
    const vars = karaokeCssVars({ ...KARAOKE_PRESETS.classic, fontSize: 50, strokeWidth: 5, shadowDepth: 3, glowStrength: 0 });
    expect(vars["--ktv-stroke"]).toBe("0.2000em");
    // 侧面：1 + 2 两步叠出 0…3 的整块
    expect(vars["--ktv-unsung-filter"]).toBe("drop-shadow(0.0200em 0.0200em 0 #000000) drop-shadow(0.0400em 0.0400em 0 #000000)");
    expect(karaokeCssVars(KARAOKE_PRESETS.plain)["--ktv-sung-filter"]).not.toContain("NaN");
    expect(karaokeCssVars({ ...KARAOKE_PRESETS.plain, glowStrength: 0 })["--ktv-sung-filter"]).toBe("none");
  });

  it("builds a contiguous extrusion from doubling steps", () => {
    for (let depth = 0; depth <= 12; depth++) {
      let reach = new Set([0]);
      for (const step of extrusionSteps(depth)) reach = new Set([...reach, ...[...reach].map((x) => x + step)]);
      expect([...reach].sort((a, b) => a - b)).toEqual(Array.from({ length: depth + 1 }, (_, i) => i));
    }
    expect(extrusionSteps(12).length).toBe(4);
  });
});
