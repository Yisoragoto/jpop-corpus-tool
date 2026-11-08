/**
 * 歌词行循环与循环区间的微调（#64）。
 *
 * 区间由 Rust 算（`loop_region`：起点是第一行开始，终点是最后一个字唱完，再加记住的微调），
 * 这里只负责：开始循环、改微调、往前后加一行，然后把算好的起止交给播放器。
 *
 * 微调按行记在库里：起点的记在第一行，终点的记在最后一行。下次再循环这一行还是调过的样子，
 * 试听和制卡截的那一段也跟着用。
 *
 * 当前循环放在模块里而不是某个页面的 state：快捷键是全局的，换页回来调整栏也还在。
 * 播放器那边的循环被别处取消（播放条、命令面板）时，`activeLineLoop` 认出它已经不是这一段了。
 */

import { useSyncExternalStore } from "react";

import { api, command, type LoopRegion as PlayerLoop } from "./api";
import type { LoopRegion } from "./generated/jp-api";

export type { LoopRegion };

/** 一下挪多少（毫秒）：普通 0.1 秒，按着 Alt 0.5 秒 */
export const FINE_STEP_MS = 100;
export const COARSE_STEP_MS = 500;

let current: LoopRegion | null = null;
const listeners = new Set<() => void>();

function publish(next: LoopRegion | null) {
  current = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** 最近一次由这里开始的循环（可能已经被别处取消了，配合 `activeLineLoop` 用） */
export function useLineLoopRegion(): LoopRegion | null {
  return useSyncExternalStore(subscribe, () => current);
}

export function getLineLoopRegion(): LoopRegion | null {
  return current;
}

/** 播放器正在循环的是不是这一段 */
export function activeLineLoop(region: LoopRegion | null, playing: PlayerLoop | null, songId: string): LoopRegion | null {
  if (!region || !playing || region.songId !== songId) return null;
  const same = Math.abs(playing.startSec - region.startSec) < 0.01 && Math.abs(playing.endSec - region.endSec) < 0.01;
  return same ? region : null;
}

// 连着按快捷键时一次接一次地改，后一次基于前一次的结果，不会互相覆盖
let queue: Promise<unknown> = Promise.resolve();
function serial<T>(job: () => Promise<T>): Promise<T> {
  const next = queue.then(job, job);
  queue = next.catch(() => undefined);
  return next;
}

async function apply(region: LoopRegion | null, seekToStart: boolean): Promise<boolean> {
  if (!region) return false;
  if (!(await api.audioSetLoop(region.startSec, region.endSec))) return false;
  publish(region);
  if (seekToStart) await api.audioSeek(region.startSec);
  return true;
}

/** 循环从 `first` 行到 `last` 行（单行就两个一样）。行没有时间戳、太短时返回 false */
export function startLineLoop(first: number, last: number = first, seekToStart = false): Promise<boolean> {
  return serial(async () =>
    apply(await command("loop_region", { firstUtteranceId: first, lastUtteranceId: last }), seekToStart),
  );
}

/** 在当前微调上再挪一点（毫秒，负数是提前）。改了起点就从新起点重放，好马上听出来 */
export function nudgeLineLoop(change: { startMs?: number; endMs?: number }): Promise<boolean> {
  return serial(async () => {
    const region = current;
    if (!region) return false;
    const next = await command("loop_adjust_set", {
      firstUtteranceId: region.firstUtteranceId,
      lastUtteranceId: region.lastUtteranceId,
      startDeltaMs: region.startDeltaMs + (change.startMs ?? 0),
      endDeltaMs: region.endDeltaMs + (change.endMs ?? 0),
    });
    return apply(next, (change.startMs ?? 0) !== 0);
  });
}

/** 这一段的微调清零 */
export function resetLineLoop(): Promise<boolean> {
  return serial(async () => {
    const region = current;
    if (!region) return false;
    const next = await command("loop_adjust_set", {
      firstUtteranceId: region.firstUtteranceId,
      lastUtteranceId: region.lastUtteranceId,
      startDeltaMs: 0,
      endDeltaMs: 0,
    });
    return apply(next, region.startDeltaMs !== 0);
  });
}

/**
 * 往前或往后多循环一行。`timedIds` 是这首歌有时间戳的行，按时间顺序。
 * 往前加时起点变了，从新起点重放
 */
export function extendLineLoop(timedIds: readonly number[], side: "before" | "after"): Promise<boolean> {
  const region = current;
  if (!region) return Promise.resolve(false);
  const a = timedIds.indexOf(region.firstUtteranceId);
  const b = timedIds.indexOf(region.lastUtteranceId);
  if (a < 0 || b < 0) return Promise.resolve(false);
  const first = side === "before" ? timedIds[a - 1] : timedIds[a];
  const last = side === "after" ? timedIds[b + 1] : timedIds[b];
  if (first === undefined || last === undefined) return Promise.resolve(false);
  return startLineLoop(first, last, side === "before");
}

export async function stopLineLoop(): Promise<void> {
  await api.audioSetLoop();
}

/** 正在唱的那一行（Ctrl+L 用）。没有时间戳的歌是 null */
export async function lineAt(songId: string, positionSec: number): Promise<number | null> {
  const spans = await api.lineSpans(songId);
  let hit: number | null = null;
  for (const span of spans) {
    if (span.startSec <= positionSec + 0.05) hit = span.utteranceId;
    else break;
  }
  return hit ?? spans[0]?.utteranceId ?? null;
}

/** 循环快捷键对应的动作。`[` `]` 挪起点，Shift+`[` `]` 挪终点，按着 Alt 步长 0.5 秒 */
export function loopKeyChange(e: Pick<KeyboardEvent, "code" | "shiftKey" | "altKey">): { startMs?: number; endMs?: number } | null {
  const sign = e.code === "BracketLeft" ? -1 : e.code === "BracketRight" ? 1 : 0;
  if (sign === 0) return null;
  const step = sign * (e.altKey ? COARSE_STEP_MS : FINE_STEP_MS);
  return e.shiftKey ? { endMs: step } : { startMs: step };
}
