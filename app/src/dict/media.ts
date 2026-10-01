/**
 * 词典图片：按 (词典, 路径) 取字节、建 Blob URL，缓存复用。
 *
 * 类型表照抄 Yomitan `ext/js/media/media-util.js`（和 Rust 侧 `jp-dict/src/media.rs` 同一张表）。
 * SVG 通过 `<img>` 显示，里面的脚本不会执行。
 */

import { dictApi } from "./api";

const MEDIA_TYPES: Record<string, string> = {
  ".apng": "image/apng",
  ".avif": "image/avif",
  ".bmp": "image/bmp",
  ".gif": "image/gif",
  ".ico": "image/x-icon",
  ".cur": "image/x-icon",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".jfif": "image/jpeg",
  ".pjpeg": "image/jpeg",
  ".pjp": "image/jpeg",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".tif": "image/tiff",
  ".tiff": "image/tiff",
  ".webp": "image/webp",
};

export function mediaTypeFromPath(path: string): string | null {
  const match = /\.[^./\\]*$/.exec(path);
  return match !== null ? (MEDIA_TYPES[match[0].toLowerCase()] ?? null) : null;
}

/**
 * `dict_media` 在 Tauri 走自定义协议通道时返回 ArrayBuffer；通道被 CSP 挡掉、退回 postMessage 时
 * 返回的是数字数组。直接把数字数组塞进 Blob 会被当成文字「60,115,…」，图片必然加载失败——
 * 真实应用里就出过这个问题（CSP 漏了 connect-src），所以两种都认。
 */
export function toBytes(data: ArrayBuffer | ArrayLike<number>): Uint8Array<ArrayBuffer> {
  return data instanceof ArrayBuffer ? new Uint8Array(data) : Uint8Array.from(data);
}

const cache = new Map<string, Promise<string>>();

export function loadMediaUrl(dictionary: string, path: string): Promise<string> {
  const key = JSON.stringify([dictionary, path]);
  let url = cache.get(key);
  if (url === undefined) {
    url = dictApi
      .media(dictionary, path)
      .then((data) =>
        URL.createObjectURL(new Blob([toBytes(data)], { type: mediaTypeFromPath(path) ?? "application/octet-stream" })),
      );
    // 失败的不缓存，下次再试
    url.catch(() => cache.delete(key));
    cache.set(key, url);
  }
  return url;
}
