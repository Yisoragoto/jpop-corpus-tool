import { describe, expect, it } from "vitest";

import { buildFontOptions, DEFAULT_FONT_STACK, lyricFontStack, quoteFamily } from "./fonts";

describe("lyricFontStack", () => {
  it("quotes chosen fonts, drops empty ones and keeps the UI font as the last resort", () => {
    expect(lyricFontStack("Meiryo UI", "LXGW WenKai")).toBe(`"Meiryo UI", "LXGW WenKai", ${DEFAULT_FONT_STACK}`);
    expect(lyricFontStack("", "")).toBe(DEFAULT_FONT_STACK);
    expect(lyricFontStack("Meiryo", "Meiryo")).toBe(`"Meiryo", ${DEFAULT_FONT_STACK}`);
  });

  it("cannot be broken out of with quotes", () => {
    expect(quoteFamily('Evil", monospace; x: "')).toBe('"Evil, monospace; x: "');
  });
});

describe("buildFontOptions", () => {
  it("lists font files first, then the Python favourites, then everything else that actually renders", () => {
    const usable = (name: string) => !["Yu Gothic UI Semilight", "Marlett", "メイリオ"].includes(name);
    const options = buildFontOptions(
      {
        installed: [
          { name: "Arial", aliases: [] },
          { name: "Marlett", aliases: [] },
          { name: "Meiryo", aliases: ["メイリオ"] },
          { name: "Microsoft YaHei UI", aliases: ["微软雅黑 UI"] },
          { name: "Yu Gothic UI Semilight", aliases: [] },
        ],
        files: [
          { family: "Klee One", path: "D:/a/KleeOne.ttf", bundled: true },
          { family: "Klee One SemiBold", path: "D:/a/KleeOne.ttf", bundled: true },
          { family: "My Font", path: "C:/fonts/my.ttf", bundled: false },
        ],
        problems: [],
      },
      usable,
    );
    expect(options.map((o) => o.label)).toEqual([
      "Klee One（自带）",
      "My Font（导入）",
      "Microsoft YaHei UI（微软雅黑 UI）",
      "Meiryo",
      "Arial",
    ]);
    expect(options[0]?.file?.path).toBe("D:/a/KleeOne.ttf");
  });
});
