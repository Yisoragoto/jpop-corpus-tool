/**
 * 前端的扩展名表（文件选择框的筛选、拖放时分文件和文件夹）要和 Rust 的
 * `jp_import::scan::AUDIO_EXTS` / `UNSUPPORTED_AUDIO` 一致。以前是手抄的两份，
 * 后端加了格式、前端忘了改，选择框里就选不到那种文件。
 */

import { describe, expect, it } from "vitest";

import { AUDIO_EXTENSIONS, PLAYABLE_AUDIO_EXTENSIONS, UNSUPPORTED_AUDIO_EXTENSIONS, isAudioPath } from "./dropFiles";
import { AUDIO_EXTENSIONS as RELINK_EXTENSIONS } from "./libraryAdmin";

const SCAN = Object.values(
  import.meta.glob<string>("../../rust/crates/jp-import/src/scan.rs", { query: "?raw", import: "default", eager: true }),
)[0]!;

/** `pub const NAME: &[...] = &[ ... ];` 里的内容 */
function table(name: string): string {
  const start = SCAN.indexOf(`pub const ${name}:`);
  expect(start, name).toBeGreaterThanOrEqual(0);
  const body = SCAN.slice(SCAN.indexOf("= &[", start) + 4);
  return body.slice(0, body.indexOf("];"));
}

describe("audio extensions", () => {
  it("playable ones match AUDIO_EXTS", () => {
    const rust = [...table("AUDIO_EXTS").matchAll(/"([a-z0-9]+)"/g)].map((m) => m[1]);
    expect(PLAYABLE_AUDIO_EXTENSIONS).toEqual(rust);
    expect(RELINK_EXTENSIONS).toEqual(rust);
  });

  it("unsupported ones match UNSUPPORTED_AUDIO", () => {
    const rust = [...table("UNSUPPORTED_AUDIO").matchAll(/\("([a-z0-9]+)",/g)].map((m) => m[1]);
    expect(rust.length).toBeGreaterThan(2);
    expect(UNSUPPORTED_AUDIO_EXTENSIONS).toEqual(rust);
  });

  it("drops of either kind count as audio files", () => {
    expect(AUDIO_EXTENSIONS).toHaveLength(new Set(AUDIO_EXTENSIONS).size);
    for (const path of ["D:/a/b.AIFF", "D:/a/b.ncm", "D:/a/b.m4a", "D:/a/b.mflac"]) expect(isAudioPath(path), path).toBe(true);
    for (const path of ["D:/a/b.jpg", "D:/a/b.lrc", "D:/a/folder"]) expect(isAudioPath(path), path).toBe(false);
  });
});
