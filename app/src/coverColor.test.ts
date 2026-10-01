import { describe, expect, it } from "vitest";

import { dominantColor, toBackdrop } from "./coverColor";

/** 造一张纯色图的像素：n 个像素，全是同一个颜色 */
function solid(n: number, r: number, g: number, b: number, a = 255): Uint8ClampedArray {
  const out = new Uint8ClampedArray(n * 4);
  for (let i = 0; i < n; i++) out.set([r, g, b, a], i * 4);
  return out;
}

function concat(...parts: Uint8ClampedArray[]): Uint8ClampedArray {
  const out = new Uint8ClampedArray(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

describe("封面取色", () => {
  it("挑饱和度高的颜色，不被黑边和留白带偏", () => {
    const pixels = concat(solid(300, 0, 0, 0), solid(200, 255, 255, 255), solid(20, 190, 60, 70));
    expect(dominantColor(pixels)).toEqual({ r: 190, g: 60, b: 70 });
  });

  it("整张都是灰的就退回平均色", () => {
    const grey = dominantColor(concat(solid(2, 120, 120, 120), solid(2, 140, 140, 140)));
    expect(grey).toEqual({ r: 130, g: 130, b: 130 });
  });

  it("全透明取不到颜色，交给调用方退回中性深色", () => {
    expect(dominantColor(solid(10, 200, 30, 30, 0))).toBeNull();
  });

  it("压暗后保留色相，亮度够低能当背景", () => {
    expect(toBackdrop({ r: 255, g: 0, b: 0 })).toBe("rgb(66, 0, 0)");
    // 已经很暗的颜色不会被提亮
    expect(toBackdrop({ r: 20, g: 10, b: 0 })).toBe("rgb(20, 10, 0)");
  });
});
