/**
 * 当前行的推导。换歌那一下的边界全在这里——UI 上的表现是
 * 「播完一首切下一首，歌词总是跑到最底下」。
 */

import { describe, expect, it } from "vitest";

import { pickLine, pollDelay, samePlayback, timedLines } from "./usePlayback";
import type { LyricLine, PlaybackState } from "./api";

function lines(...times: (number | null)[]): LyricLine[] {
  return times.map((timeSec, i) => ({
    utteranceId: i + 1,
    lineIdx: i,
    timeSec,
    text: `line ${i}`,
  })) as LyricLine[];
}

describe("pickLine", () => {
  it("找最后一个时间不晚于当前位置的行", () => {
    const timed = timedLines(lines(0, 10, 20, 30));
    expect(pickLine(timed, 0)).toBe(0);
    expect(pickLine(timed, 9.9)).toBe(0);
    expect(pickLine(timed, 10)).toBe(1);
    expect(pickLine(timed, 25)).toBe(2);
    expect(pickLine(timed, 999)).toBe(3);
  });

  it("还没到第一行、或者整首没有时间轴时没有当前行", () => {
    expect(pickLine(timedLines(lines(5, 10)), 2)).toBe(-1);
    expect(pickLine(timedLines(lines(null, null)), 100)).toBe(-1);
    expect(pickLine([], 100)).toBe(-1);
  });

  it("没有时间轴的行不参与，但下标仍然指向原数组", () => {
    // 作词作曲那种没有时间戳的行夹在中间时，下标不能错位
    const timed = timedLines(lines(null, 10, null, 20));
    expect(pickLine(timed, 12)).toBe(1);
    expect(pickLine(timed, 21)).toBe(3);
  });

  /**
   * 换歌的一瞬间：歌词已经是新歌的，播放位置还是上一首的（轮询 10Hz，最多差一拍）。
   * 拿旧位置去二分新歌词，找到的是新歌的**末尾**，跟随滚动于是把歌词整个滚到最下面。
   * 调用方发现两者不是同一首时传 null，这里必须回答「没有当前行」。
   */
  it("位置不属于这份歌词时没有当前行", () => {
    const timed = timedLines(lines(0, 10, 20, 30));
    expect(pickLine(timed, null)).toBe(-1);
    // 对照：同样的位置，若当成属于这份歌词，点亮的是最后一行
    expect(pickLine(timed, 218)).toBe(3);
  });
});

const base: PlaybackState = {
  songId: "159",
  playState: "playing",
  positionSec: 12.3,
  durationSec: 640,
  rate: 1,
  volume: 0.8,
  loopRegion: null,
  pitchSemitones: 0,
  pitchRendering: false,
  pitchError: null,
};

describe("samePlayback：状态没变就不 setState", () => {
  it("每拍都是新对象，但字段一样就算没变——不然暂停时 App 每秒白渲染 10 次", () => {
    expect(samePlayback(base, { ...base })).toBe(true);
    expect(samePlayback(base, { ...base, loopRegion: null })).toBe(true);
    expect(
      samePlayback(
        { ...base, loopRegion: { startSec: 1, endSec: 2 }, pitchError: { id: 3, message: "x" } },
        { ...base, loopRegion: { startSec: 1, endSec: 2 }, pitchError: { id: 3, message: "x" } },
      ),
    ).toBe(true);
  });

  it("任何一个字段变了都要更新", () => {
    const changes: Partial<PlaybackState>[] = [
      { songId: "160" },
      { playState: "paused" },
      { positionSec: 12.4 },
      { durationSec: null },
      { rate: 1.5 },
      { volume: 0 },
      { loopRegion: { startSec: 1, endSec: 2 } },
      { pitchSemitones: -2 },
      { pitchRendering: true },
      { pitchError: { id: 1, message: "失败" } },
    ];
    for (const change of changes) {
      expect(samePlayback(base, { ...base, ...change }), JSON.stringify(change)).toBe(false);
    }
    expect(
      samePlayback({ ...base, pitchError: { id: 1, message: "a" } }, { ...base, pitchError: { id: 2, message: "a" } }),
    ).toBe(false);
  });
});

describe("pollDelay：闲着的时候少问", () => {
  it("在播、或者正在渲染变调时 10Hz", () => {
    expect(pollDelay(base)).toBe(100);
    expect(pollDelay({ ...base, playState: "paused", pitchRendering: true })).toBe(100);
  });

  it("暂停、播完、什么都没加载时 1Hz", () => {
    for (const playState of ["paused", "ended", "empty"] as const) {
      expect(pollDelay({ ...base, playState })).toBe(1000);
    }
  });
});
