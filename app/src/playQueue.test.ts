import { describe, expect, it } from "vitest";

import type { Track } from "./api";
import { firstPlayable, neighbor } from "./playQueue";
import { canCue } from "./useLibrary";

const track = (id: string, audio = true): Track => ({
  id, title: id, artist: "", album: "", albumId: null, year: "", genre: "", audioPath: audio ? `/m/${id}.flac` : "", coverPath: "", durationSec: 200, lineCount: 10,
});

describe("播放队列里找下一首", () => {
  const list = [track("a"), track("b", false), track("c"), track("d", false)];

  it("跳过没有音频的", () => {
    expect(neighbor(list, 0, 1)).toBe(2);
    expect(neighbor(list, 2, -1)).toBe(0);
  });

  it("到头不绕回：放完最后一首就停", () => {
    expect(neighbor(list, 2, 1)).toBeNull();
    expect(neighbor(list, 0, -1)).toBeNull();
  });

  it("点的那首没有音频：顺延到后面第一首有的，后面没有再往前找", () => {
    expect(firstPlayable(list, 1)).toBe(2);
    expect(firstPlayable(list, 3)).toBe(2);
    expect(firstPlayable(list, 0)).toBe(0);
  });

  it("整个列表都没有音频：放不了", () => {
    expect(firstPlayable([track("x", false)], 0)).toBeNull();
    expect(firstPlayable([], 0)).toBeNull();
  });
});

describe("只是看一首歌时，能不能把它备进播放条", () => {
  const at = (playState: "empty" | "playing" | "paused" | "ended", songId = "a", positionSec = 0) => ({ playState, songId, positionSec });

  it("播放条空着、放完了：可以", () => {
    expect(canCue(at("empty", ""), null)).toBe(true);
    expect(canCue(at("ended"), null)).toBe(true);
  });

  it("别的歌在放、或者放到一半暂停着：不动它", () => {
    expect(canCue(at("playing", "a", 42), null)).toBe(false);
    expect(canCue(at("paused", "a", 42), null)).toBe(false);
    expect(canCue(at("paused", "a", 42), "a")).toBe(false);
  });

  it("里面是上一首看过没放的歌：可以换掉", () => {
    expect(canCue(at("paused", "a", 0), "a")).toBe(true);
    // 暂停在开头，但那是用户自己放的歌（不是备上的）
    expect(canCue(at("paused", "a", 0), null)).toBe(false);
    expect(canCue(at("paused", "a", 0), "b")).toBe(false);
  });
});
