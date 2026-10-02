/**
 * 当前行的推导。换歌那一下的边界全在这里——UI 上的表现是
 * 「播完一首切下一首，歌词总是跑到最底下」。
 */

import { describe, expect, it } from "vitest";

import { pickLine, timedLines } from "./usePlayback";
import type { LyricLine } from "./api";

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
