import { describe, expect, it } from "vitest";

import { isJump, stepClock, type PolledSample } from "./playbackClock";
import type { ClockState } from "./useKaraoke";
import { pickLine, type TimedLine } from "./usePlayback";

/** 每两秒一行，从 0 秒开始 */
const LINES: TimedLine[] = Array.from({ length: 20 }, (_, idx) => ({ idx, time: idx * 2 }));

/**
 * 模拟一段播放：播放器真实位置由 `truth(ms)` 给，10Hz 轮询（带抖动和 IPC 延迟），60fps 画帧。
 * 返回每帧的（时刻，时钟秒数，当前行）。
 */
function simulate(
  truth: (ms: number) => number,
  opts: { from: number; to: number; rate?: number; playing?: (ms: number) => boolean; pollGap?: (ms: number) => boolean },
) {
  const rate = opts.rate ?? 1;
  const playing = opts.playing ?? (() => true);
  let polled: PolledSample | null = null;
  let jumped = false;
  let nextPoll = opts.from;
  let clock: ClockState | null = null;
  const frames: { at: number; sec: number; line: number }[] = [];
  for (let at = opts.from; at <= opts.to; at += 16) {
    while (nextPoll <= at) {
      if (!opts.pollGap?.(nextPoll)) {
        // 轮询拿到的是 30ms 前的位置（IPC 往返），再加一点抖动
        const jitter = ((nextPoll * 7919) % 23) - 11;
        const sample = { pos: Math.max(0, truth(nextPoll - 30 + jitter)), at: nextPoll };
        if (isJump(polled, sample, playing(nextPoll), rate)) jumped = true;
        polled = sample;
      }
      nextPoll += 100;
    }
    if (polled === null) continue;
    clock = stepClock(clock, polled, at, playing(at), rate, jumped);
    jumped = false;
    frames.push({ at, sec: clock.sec, line: pickLine(LINES, clock.sec) });
  }
  return frames;
}

describe("isJump", () => {
  it("ordinary playback and polling jitter are not jumps", () => {
    expect(isJump({ pos: 10, at: 0 }, { pos: 10.1, at: 100 }, true, 1)).toBe(false);
    expect(isJump({ pos: 10, at: 0 }, { pos: 10.15, at: 100 }, true, 1)).toBe(false);
    expect(isJump({ pos: 10, at: 0 }, { pos: 10.2, at: 100 }, true, 2)).toBe(false);
  });
  it("seeking forward or back is a jump", () => {
    expect(isJump({ pos: 10, at: 0 }, { pos: 30, at: 100 }, true, 1)).toBe(true);
    expect(isJump({ pos: 10, at: 0 }, { pos: 9.5, at: 100 }, true, 1)).toBe(true);
  });
  it("while paused any movement beyond jitter is a jump", () => {
    expect(isJump({ pos: 10, at: 0 }, { pos: 10, at: 1000 }, false, 1)).toBe(false);
    expect(isJump({ pos: 10, at: 0 }, { pos: 12, at: 1000 }, false, 1)).toBe(true);
  });
});

describe("stepClock", () => {
  it("line changes land within a frame or two of the real boundary, never going back", () => {
    const frames = simulate((ms) => ms / 1000, { from: 0, to: 12_000 });
    let previous = -1;
    for (const frame of frames) {
      expect(frame.line).toBeGreaterThanOrEqual(previous);
      previous = frame.line;
    }
    for (const boundary of [2, 4, 6, 8, 10]) {
      const first = frames.find((f) => f.line === boundary / 2);
      expect(first).toBeDefined();
      // 真实时刻 boundary 秒；轮询晚 30ms + 10Hz，插值后应在 ±80ms 内
      expect(Math.abs(first!.at - boundary * 1000)).toBeLessThan(80);
    }
  });

  it("the clock is monotonic between polls during normal playback", () => {
    const frames = simulate((ms) => ms / 1000, { from: 0, to: 5_000 });
    for (let i = 1; i < frames.length; i++) expect(frames[i]!.sec).toBeGreaterThanOrEqual(frames[i - 1]!.sec);
  });

  it("follows playback rate", () => {
    const frames = simulate((ms) => (ms / 1000) * 2, { from: 0, to: 6_000, rate: 2 });
    const first = frames.find((f) => f.line === 5)!; // 10 秒处，真实时刻 5000ms
    expect(Math.abs(first.at - 5_000)).toBeLessThan(80);
  });

  it("pausing holds the line at the player position, resuming continues from there", () => {
    // 0–3s 播，3–6s 暂停在 3s，6s 后接着播
    const truth = (ms: number) => (ms < 3_000 ? ms / 1000 : ms < 6_000 ? 3 : 3 + (ms - 6_000) / 1000);
    const playing = (ms: number) => ms < 3_000 || ms >= 6_000;
    const frames = simulate(truth, { from: 0, to: 9_000, playing });
    const paused = frames.filter((f) => f.at > 3_300 && f.at < 6_000);
    expect(paused.every((f) => f.line === 1)).toBe(true);
    expect(paused.every((f) => f.sec <= 3.05)).toBe(true);
    const resumed = frames.find((f) => f.line === 2)!; // 4 秒处 = 真实 7000ms
    expect(Math.abs(resumed.at - 7_000)).toBeLessThan(120);
  });

  it("seeking back lands on the earlier line immediately instead of waiting", () => {
    // 跑到 9 秒时往回跳到 1 秒
    const truth = (ms: number) => (ms < 9_000 ? ms / 1000 : 1 + (ms - 9_000) / 1000);
    const frames = simulate(truth, { from: 0, to: 10_000 });
    const after = frames.filter((f) => f.at >= 9_200 && f.at < 9_500);
    expect(after.length).toBeGreaterThan(0);
    expect(after.every((f) => f.line === 0)).toBe(true);
  });

  it("a short step back across a line boundary is not swallowed by the forward-only smoothing", () => {
    // 4.3 秒处往回退半秒到 3.8 秒（比如单句循环回绕、← 键）：应立刻回到第 1 行
    const truth = (ms: number) => (ms < 4_300 ? ms / 1000 : 3.8 + (ms - 4_300) / 1000);
    const frames = simulate(truth, { from: 0, to: 4_700 });
    // 下一拍（4400ms）就该回到第 1 行，直到 3.8 秒接着播到 4 秒（约 4500ms）
    const after = frames.filter((f) => f.at >= 4_400 && f.at < 4_480);
    expect(after.length).toBeGreaterThan(0);
    expect(after.every((f) => f.line === 1)).toBe(true);
  });

  it("seeking forward jumps straight to the target line", () => {
    const truth = (ms: number) => (ms < 2_500 ? ms / 1000 : 30 + (ms - 2_500) / 1000);
    const frames = simulate(truth, { from: 0, to: 3_000 });
    const after = frames.filter((f) => f.at >= 2_700);
    expect(after.every((f) => f.line === 15)).toBe(true);
  });

  it("when polling stalls the clock stops after a short extrapolation", () => {
    // 1.9 秒后轮询全停（后台、卡住）：不能一路滑进下一行太多
    const frames = simulate((ms) => ms / 1000, { from: 0, to: 5_000, pollGap: (ms) => ms > 1_900 });
    const last = frames.at(-1)!;
    expect(last.sec).toBeLessThan(1.9 + 0.6);
  });
});
