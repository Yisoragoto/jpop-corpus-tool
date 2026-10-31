/**
 * 当前行逐字点亮（卡拉 OK 歌词）的驱动。
 *
 * 播放位置是 10Hz 轮询来的（`usePlayback.ts`），拿它直接点字一卡一卡的。这里用
 * requestAnimationFrame 自己走一个时钟（`advanceClock`），每帧**直接改 DOM**：给当前行里
 * 已经唱过的单位加 `kc-on`、正在唱的单位加 `kc-now` 并设 `--p`（亮了几成）。
 * 一个真实单位内的几个字按顺序扫过去（只是画法），不拆出虚构的字符起止时间。
 * 不走 React 状态——每帧 setState 会把整首歌词重渲染一遍。
 *
 * 字是曲库页 `renderLineText` 渲染出来的 `.karaoke-line .kc`（`data-c` 是字符下标）。
 * 平滑的是播放器时钟，不是歌词时间轴；没有完整真实时间轴时不启动动画。
 * 普通歌词和全屏歌词同时挂着时两份一起点。React 重渲染时这些 span 的 className
 * 属性没变，React 不会去碰 DOM，这里加的类不会被冲掉。
 */

import { useEffect, useRef } from "react";

import { charProgress, type CharTimeline } from "./wordTiming";

/** 轮询停了（卡顿、切到后台）时，从最后一拍往前最多推多久，免得一路亮到行尾 */
const MAX_EXTRAPOLATE = 0.5;
/** 和轮询差得比这多，就是拖了进度条 / 换了一句：直接跳过去 */
const SNAP_SEC = 0.6;
/** 差得不多时每帧追回这么多比例：几帧之内悄悄对齐，看不出抖 */
const PULL = 0.12;

/** 新的一拍和预期差这么多（秒）就算跳了：轮询抖动通常在几十毫秒内 */
export const JUMP_SEC = 0.35;

export interface PolledSample {
  /** 轮询回来的位置（秒） */
  pos: number;
  /** 收到它的时刻（performance.now()，毫秒） */
  at: number;
}

/**
 * 新的一拍是不是一次跳转（拖进度条、点行、循环回绕）。
 * 按上一拍的位置、经过的时间和倍速推一个预期位置，差得多就是跳了。
 */
export function isJump(prev: PolledSample | null, next: PolledSample, playing: boolean, rate: number): boolean {
  if (prev === null) return false;
  const expected = prev.pos + (playing ? Math.max(0, (next.at - prev.at) / 1000) * rate : 0);
  return Math.abs(next.pos - expected) > JUMP_SEC;
}

export interface ClockState {
  /** 上一帧显示到第几秒 */
  sec: number;
  /** 上一帧是什么时候（performance.now()，毫秒） */
  at: number;
}

/**
 * 显示用的播放时钟走一帧。
 *
 * 轮询回来的位置有抖动（IPC 往返、引擎按缓冲块报位置），直接用会让字忽前忽后地闪。
 * 这里自己按倍速往前走：落后于轮询位置就一点点追上去，超前就停一下等它；**只进不退**，
 * 除非差得太多（拖进度条、往回跳），那时直接跳到轮询位置。暂停时就是轮询位置。
 */
export function advanceClock(
  prev: ClockState | null,
  polled: { pos: number; at: number },
  now: number,
  playing: boolean,
  rate: number,
): ClockState {
  if (!playing) return { sec: polled.pos, at: now };
  const target = polled.pos + Math.min(MAX_EXTRAPOLATE, Math.max(0, (now - polled.at) / 1000) * rate);
  if (prev === null) return { sec: target, at: now };
  const predicted = prev.sec + Math.max(0, (now - prev.at) / 1000) * rate;
  const err = target - predicted;
  if (Math.abs(err) > SNAP_SEC) return { sec: target, at: now };
  // 落后：几帧之内追上去；超前：停在原地等轮询追上来（不往回退，轮询停了也不会一直往前跑）
  if (err >= 0) return { sec: predicted + err * PULL, at: now };
  return { sec: Math.max(prev.sec, target), at: now };
}

/** 只消费已验证的来源时间轴；null 会清除上一帧的点亮状态。 */
export function paintKaraoke(timeline: CharTimeline | null, sec: number, elements: Iterable<HTMLElement>): void {
  const { done, activeEnd, frac } = timeline === null ? { done: 0, activeEnd: 0, frac: 0 } : charProgress(timeline, sec);
  for (const element of elements) {
    const index = Number(element.dataset.c);
    const valid = timeline !== null && Number.isInteger(index) && index >= 0 && index < timeline.starts.length;
    const on = valid && index < done;
    // 一个单位里的几个字连成一道从左到右扫过去：这只是画法，单位的起止时间仍是来源给的那一对
    const width = activeEnd - done;
    const local = width > 0 ? Math.min(1, Math.max(0, frac * width - (index - done))) : 0;
    const now = valid && index >= done && index < activeEnd && local > 0;
    if (now) element.style.setProperty("--p", `${(local * 100).toFixed(1)}%`);
    else element.style.removeProperty("--p");
    element.classList.toggle("kc-on", on);
    element.classList.toggle("kc-now", now);
  }
}

export function useKaraoke(
  timeline: CharTimeline | null,
  positionSec: number,
  playing: boolean,
  rate: number,
  /** 字的 DOM 换了（开关全屏、振假名）时变一下：暂停着也要把新的字重新点上 */
  repaintKey: unknown,
): void {
  const polled = useRef<PolledSample>({ pos: positionSec, at: performance.now() });
  const jumped = useRef(false);
  // 每拍位置一变就记下「这一刻在这里」；拖了进度条、点行跳转时下一帧直接落过去（往回跳也不等）
  useEffect(() => {
    const next = { pos: positionSec, at: performance.now() };
    if (isJump(polled.current, next, playing, rate)) jumped.current = true;
    polled.current = next;
  }, [positionSec, playing, rate]);

  useEffect(() => {
    const elements = () => document.querySelectorAll<HTMLElement>(".karaoke-line .kc");
    if (timeline === null) { paintKaraoke(null, 0, elements()); return; }
    let frame = 0;
    let clock: ClockState | null = null;
    const paint = () => {
      clock = advanceClock(jumped.current ? null : clock, polled.current, performance.now(), playing, rate);
      jumped.current = false;
      paintKaraoke(timeline, clock.sec, elements());
      if (playing) frame = requestAnimationFrame(paint);
    };
    // 先画一帧：暂停时停在这里，刚换行时字也立刻就位
    frame = requestAnimationFrame(paint);
    return () => { cancelAnimationFrame(frame); paintKaraoke(null, 0, elements()); };
    // 暂停时拖动进度条：位置变了也要重画一帧；在播时每帧都在画，不用跟着位置重启
  }, [timeline, playing, rate, repaintKey, playing ? 0 : positionSec]);
}
