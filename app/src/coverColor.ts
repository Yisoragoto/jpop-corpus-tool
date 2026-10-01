/**
 * 从封面里取一个主色，给全屏歌词做底色（Spotify 的全屏就是这么干的）。
 *
 * 只在前端算：把封面缩到 24×24 画进 canvas，取饱和度高的那些像素的平均色，
 * 再压暗到能当背景的亮度。**不引取色库**——一张 24×24 的图就 576 个像素，
 * 自己算比引一个依赖划算（要求书第十五条第 3 点）。
 *
 * 取不到（没有封面、读不了、canvas 被跨域污染）就返回 null，调用方退回中性深色，
 * 不猜一个颜色出来。
 */

import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

/** 缩到这么小再取色：够稳，又快 */
const SIZE = 24;

/** 同一张封面只算一次 */
const cache = new Map<string, Promise<string | null>>();

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

/** 饱和度高、不太暗也不太亮的像素占主导；全是灰的图就退回平均色 */
export function dominantColor(pixels: Uint8ClampedArray): Rgb | null {
  let best: { weight: number; r: number; g: number; b: number } = { weight: 0, r: 0, g: 0, b: 0 };
  let plain = { n: 0, r: 0, g: 0, b: 0 };
  for (let i = 0; i + 3 < pixels.length; i += 4) {
    const r = pixels[i] ?? 0;
    const g = pixels[i + 1] ?? 0;
    const b = pixels[i + 2] ?? 0;
    const alpha = pixels[i + 3] ?? 0;
    if (alpha < 128) continue;
    plain = { n: plain.n + 1, r: plain.r + r, g: plain.g + g, b: plain.b + b };
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    // 纯黑纯白不参与：封面边框和留白会把颜色拖成灰
    if (max < 30 || min > 225) continue;
    const saturation = max === 0 ? 0 : (max - min) / max;
    const weight = saturation * saturation * (max / 255);
    if (weight > best.weight) best = { weight, r, g, b };
  }
  if (best.weight > 0.02) return { r: best.r, g: best.g, b: best.b };
  if (plain.n === 0) return null;
  return { r: Math.round(plain.r / plain.n), g: Math.round(plain.g / plain.n), b: Math.round(plain.b / plain.n) };
}

/** 压到能当背景的亮度：保留色相，亮度压到 `target` 附近 */
export function toBackdrop(color: Rgb, target = 0.26): string {
  const max = Math.max(color.r, color.g, color.b) / 255;
  const scale = max > 0 ? Math.min(1, target / max) : 0;
  const mix = (v: number) => Math.round(v * scale);
  return `rgb(${mix(color.r)}, ${mix(color.g)}, ${mix(color.b)})`;
}

async function read(path: string): Promise<string | null> {
  try {
    // fetch 而不是 <img>：asset 协议的响应带 CORS 头，这样 canvas 不会被污染
    const response = await fetch(convertFileSrc(path));
    if (!response.ok) return null;
    const bitmap = await createImageBitmap(await response.blob());
    const canvas = document.createElement("canvas");
    canvas.width = SIZE;
    canvas.height = SIZE;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (ctx === null) return null;
    ctx.drawImage(bitmap, 0, 0, SIZE, SIZE);
    bitmap.close();
    const color = dominantColor(ctx.getImageData(0, 0, SIZE, SIZE).data);
    return color === null ? null : toBackdrop(color);
  } catch {
    return null;
  }
}

/** 封面的主色（已压暗）。没有就是 null */
export function useCoverColor(path: string | null | undefined): string | null {
  const [color, setColor] = useState<string | null>(null);
  useEffect(() => {
    setColor(null);
    if (!path) return;
    let alive = true;
    const pending = cache.get(path) ?? read(path);
    cache.set(path, pending);
    void pending.then((value) => {
      if (alive) setColor(value);
    });
    return () => {
      alive = false;
    };
  }, [path]);
  return color;
}
