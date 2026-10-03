import { describe, expect, it } from "vitest";

import { ffmpegNote } from "./ffmpeg";

describe("ffmpegNote：设置页上 ffmpeg 那一行", () => {
  it("没找到时说清少了什么、放哪儿能被找到", () => {
    const note = ffmpegNote({ ffmpegPath: null, ffmpegExpected: "C:\\lib\\ffmpeg.exe" });
    expect(note.found).toBe(false);
    expect(note.status).toContain("没找到");
    expect(note.detail).toContain("变调");
    expect(note.detail).toContain("Anki 音频片段");
    expect(note.detail).toContain("PATH");
    expect(note.detail).toContain("C:\\lib\\ffmpeg.exe");
  });

  it("找到时给出用的是哪一个", () => {
    const note = ffmpegNote({ ffmpegPath: "D:\\tools\\ffmpeg.exe", ffmpegExpected: "C:\\lib\\ffmpeg.exe" });
    expect(note.found).toBe(true);
    expect(note.detail).toContain("D:\\tools\\ffmpeg.exe");
    expect(note.detail).not.toContain("不可用");
  });
});
