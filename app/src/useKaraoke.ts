/**
 * 当前行逐字点亮（卡拉 OK 歌词）的驱动。
 *
 * 播放位置是 10Hz 轮询来的（`usePlayback.ts`），拿它直接点字一卡一卡的。这里用
 * requestAnimationFrame 在两次轮询之间按倍速插值，每帧**直接改 DOM**：给当前行里
 * 已经唱过的字加 `kc-on`、正在唱的那个字加 `kc-now` 并设 `--p`（亮了几成）。
 * 不走 React 状态——每帧 setState 会把整首歌词重渲染一遍。
 *
 * 字是曲库页 `renderLineText` 渲染出来的 `.karaoke-line .kc`（`data-c` 是字符下标）。
 * 普通歌词和全屏歌词同时挂着时两份一起点。React 重渲染时这些 span 的 className
 * 属性没变，React 不会去碰 DOM，这里加的类不会被冲掉。
 */

import { useEffect, useRef } from "react";

import { charProgress, type CharTimeline } from "./wordTiming";

/** 轮询停了（卡顿、切到后台）时最多往前插多久，免得一路亮到行尾 */
const MAX_EXTRAPOLATE = 0.5;
/** 新一拍的位置比插出来的略早时不往回退（轮询抖动）；退得比这多才当作是往回拖了 */
const BACKWARD_TOLERANCE = 0.35;

export function useKaraoke(
  timeline: CharTimeline | null,
  positionSec: number,
  playing: boolean,
  rate: number,
  /** 字的 DOM 换了（开关全屏、振假名）时变一下：暂停着也要把新的字重新点上 */
  repaintKey: unknown,
): void {
  const base = useRef({ pos: positionSec, at: performance.now() });
  // 每拍位置一变就记下「这一刻在这里」
  useEffect(() => {
    base.current = { pos: positionSec, at: performance.now() };
  }, [positionSec]);

  useEffect(() => {
    if (timeline === null) return;
    let frame = 0;
    let shown = -Infinity;
    const paint = () => {
      const { pos, at } = base.current;
      let sec = playing ? pos + Math.min(MAX_EXTRAPOLATE, ((performance.now() - at) / 1000) * rate) : pos;
      if (sec < shown && shown - sec < BACKWARD_TOLERANCE) sec = shown;
      shown = sec;
      const { done, frac } = charProgress(timeline, sec);
      for (const el of document.querySelectorAll<HTMLElement>(".karaoke-line .kc")) {
        const c = Number(el.dataset.c);
        const on = c < done;
        const now = c === done && frac > 0;
        if (el.classList.contains("kc-on") !== on) el.classList.toggle("kc-on", on);
        if (el.classList.contains("kc-now") !== now) el.classList.toggle("kc-now", now);
        if (now) el.style.setProperty("--p", `${(frac * 100).toFixed(1)}%`);
      }
      if (playing) frame = requestAnimationFrame(paint);
    };
    // 先画一帧：暂停时停在这里，刚换行时字也立刻就位
    frame = requestAnimationFrame(paint);
    return () => cancelAnimationFrame(frame);
    // 暂停时拖动进度条：位置变了也要重画一帧；在播时每帧都在画，不用跟着位置重启
  }, [timeline, playing, rate, repaintKey, playing ? 0 : positionSec]);
}
