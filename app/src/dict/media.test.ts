import { describe, expect, it } from "vitest";

import { mediaTypeFromPath, toBytes } from "./media";

describe("toBytes", () => {
  const svg = [60, 115, 118, 103, 47, 62]; // "<svg/>"

  it("自定义协议通道给的 ArrayBuffer 原样转", () => {
    expect([...toBytes(new Uint8Array(svg).buffer)]).toEqual(svg);
  });

  it("退回 postMessage 通道时给的是数字数组，也要还原成同样的字节，不能被当成文字", () => {
    const bytes = toBytes(svg);
    expect([...bytes]).toEqual(svg);
    expect(new TextDecoder().decode(bytes)).toBe("<svg/>");
  });
});

describe("mediaTypeFromPath", () => {
  it("按扩展名认类型，路径里的中文和 # 不影响", () => {
    expect(mediaTypeFromPath("gaiji/対義語.svg")).toBe("image/svg+xml");
    expect(mediaTypeFromPath("gaiji/#ws一.SVG")).toBe("image/svg+xml");
    expect(mediaTypeFromPath("images_hitsujun/050df47e74.avif")).toBe("image/avif");
    expect(mediaTypeFromPath("dir.png/noext")).toBeNull();
  });
});
