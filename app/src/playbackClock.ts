/**
 * 歌词跟随用的播放时钟：播放器位置为准，两次轮询之间按倍速单调插值。
 *
 * 播放位置是 10Hz 轮询来的（`usePlayback.ts`），直接拿它算当前行，换行最多晚 100ms 再加一次 IPC。
 * 这里每帧用 `advanceClock`（和卡拉 OK 逐字点亮同一个时钟）往前推，二分出当前行，
 * **只在行变了时** setState——一首歌几十次，不是每秒 60 次。
 *
 * 几种情况：
 * - 暂停 / 播完：时钟就是轮询位置，不外推。
 * - 拖进度条、点行跳转、单句循环回绕：新的一拍和「上一拍按倍速推过来该在的位置」差得多，
 *   判成跳了，时钟直接落到新位置（往回跳也不会卡在旧行上）。
 * - 换歌：歌词不属于正在放的那一首时没有当前行（见 `pickLine`），时钟也清掉。
 * - 轮询停了（卡顿、切到后台）：最多往前推 `MAX_EXTRAPOLATE`，不会一路滑到下一句。
 * - 倍速：插值按倍速走。
 *
 * 平滑的只是时钟，不造逐字时间：没有真实逐字时间的歌词只按行跟随。
 */

import { useEffect, useRef, useState, type RefObject } from "react";

import { advanceClock, isJump, type ClockState, type PolledSample } from "./useKaraoke";

export { isJump, JUMP_SEC, type PolledSample } from "./useKaraoke";
import { pickLine, type TimedLine } from "./usePlayback";

/**
 * 时钟走一帧：跳了就直接落到新位置，否则交给 `advanceClock` 平滑。
 * 纯函数，方便测；`useSyncedLine` 每帧调一次。
 */
export function stepClock(
  prev: ClockState | null,
  polled: PolledSample,
  now: number,
  playing: boolean,
  rate: number,
  jumped: boolean,
): ClockState {
  return advanceClock(jumped ? null : prev, polled, now, playing, rate);
}

/**
 * 当前歌词行（下标，-1 = 没有）。
 *
 * `belongsToPlaying` 为假（看的不是正在放的那首）时恒为 -1。
 * 暂停时直接用轮询位置，不开动画帧。
 */
export function useSyncedLine(
  timed: TimedLine[],
  positionSec: number,
  playing: boolean,
  rate: number,
  belongsToPlaying: boolean,
): number {
  const [line, setLine] = useState(() => pickLine(timed, belongsToPlaying ? positionSec : null));
  const polled = useRef<PolledSample>({ pos: positionSec, at: performance.now() });
  const jumped = useRef(false);

  // 每拍记下「这一刻在这里」，顺便判断是不是跳了
  useEffect(() => {
    const next = { pos: positionSec, at: performance.now() };
    if (isJump(polled.current, next, playing, rate)) jumped.current = true;
    polled.current = next;
  }, [positionSec, playing, rate]);

  useEffect(() => {
    if (!belongsToPlaying) {
      setLine(-1);
      return;
    }
    if (!playing) {
      setLine(pickLine(timed, positionSec));
      return;
    }
    let frame = 0;
    let clock: ClockState | null = null;
    let shown = Number.NaN;
    const tick = () => {
      clock = stepClock(clock, polled.current, performance.now(), true, rate, jumped.current);
      jumped.current = false;
      const next = pickLine(timed, clock.sec);
      // 行没变就不碰 React 状态
      if (next !== shown) {
        shown = next;
        setLine(next);
      }
      frame = requestAnimationFrame(tick);
    };
    tick();
    return () => cancelAnimationFrame(frame);
    // 暂停时位置变了（拖进度条）要重算一次；在播时每帧都在算，不用跟着位置重启
  }, [timed, playing, rate, belongsToPlaying, playing ? 0 : positionSec]);

  return belongsToPlaying ? line : -1;
}

/**
 * 歌词跟随滚动的「手动接管」：用户自己滚（滚轮、触摸、拖滚动条、方向键 / 翻页键）时暂停跟随，
 * 直到点「回到当前歌词」或换歌。程序自己的 scrollIntoView / scrollTo 不算。
 */
export function useFollowScroll(box: RefObject<HTMLElement | null>, resetKey: unknown) {
  const [paused, setPaused] = useState(false);

  useEffect(() => setPaused(false), [resetKey]);

  useEffect(() => {
    const el = box.current;
    if (el === null) return;
    const take = () => setPaused(true);
    const onKey = (e: KeyboardEvent) => {
      if (["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", " "].includes(e.key)) take();
    };
    // 点在滚动条上：目标是容器自己，并且落在内容区右边
    const onPointer = (e: PointerEvent) => {
      if (e.target === el && e.offsetX > el.clientWidth) take();
    };
    el.addEventListener("wheel", take, { passive: true });
    el.addEventListener("touchmove", take, { passive: true });
    el.addEventListener("keydown", onKey);
    el.addEventListener("pointerdown", onPointer);
    return () => {
      el.removeEventListener("wheel", take);
      el.removeEventListener("touchmove", take);
      el.removeEventListener("keydown", onKey);
      el.removeEventListener("pointerdown", onPointer);
    };
  }, [box, resetKey]);

  return { paused, resume: () => setPaused(false) };
}
