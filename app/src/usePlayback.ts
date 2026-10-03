/**
 * 播放状态订阅。
 *
 * 前端**不持有播放器**，只订阅状态——这是要求书第十四条要的解耦。
 * 引擎在 Rust 侧，这里轮询它的快照。
 *
 * 为什么是轮询而不是事件推送：Tauri 的事件通道是给「偶发事件」用的，
 * 用它每秒推 10 次状态会在 IPC 上产生大量小消息。轮询一个廉价的
 * command（读几个原子变量）反而更省，而且不用管订阅生命周期。
 */

import { useCallback, useEffect, useMemo, useState } from "react";

import { api, PLAYBACK_KICK, type LyricLine, type PlaybackState } from "./api";

/** 状态轮询间隔。10Hz 足够驱动进度条和歌词跟随，且开销可忽略。 */
const STATE_INTERVAL = 100;

/** 频谱轮询间隔。50ms ≈ 20fps，视觉上已经连续。 */
const SPECTRUM_INTERVAL = 50;

const EMPTY_STATE: PlaybackState = {
  songId: "",
  playState: "empty",
  positionSec: 0,
  durationSec: null,
  rate: 1,
  volume: 1,
  loopRegion: null,
  pitchSemitones: 0,
  pitchRendering: false,
  pitchError: null,
};

/** 闲着（暂停、播完、没加载）时的轮询间隔：状态不会自己变，1Hz 足够 */
const IDLE_INTERVAL = 1000;

/**
 * 两份快照是不是同一个状态。
 *
 * 每拍从 IPC 回来的都是新对象。这份状态挂在 `App` 顶层，`setState` 一个新对象，
 * 整棵组件树就重渲染一遍——暂停时什么都没变，也照样每秒白渲染 10 次。
 * 字段一样时把旧对象原样还给 React，它会直接跳过这次渲染。
 */
export function samePlayback(a: PlaybackState, b: PlaybackState): boolean {
  return (
    a.songId === b.songId &&
    a.playState === b.playState &&
    a.positionSec === b.positionSec &&
    a.durationSec === b.durationSec &&
    a.rate === b.rate &&
    a.volume === b.volume &&
    a.loopRegion?.startSec === b.loopRegion?.startSec &&
    a.loopRegion?.endSec === b.loopRegion?.endSec &&
    a.pitchSemitones === b.pitchSemitones &&
    a.pitchRendering === b.pitchRendering &&
    a.pitchError?.id === b.pitchError?.id &&
    a.pitchError?.message === b.pitchError?.message
  );
}

/**
 * 下一拍隔多久。在播、或者变调正在渲染（渲染完会自己开始播）时 10Hz，其余 1Hz。
 *
 * 闲着时状态只会因为用户操作而变，而播放控制命令完成时会发 `PLAYBACK_KICK`
 * 让这里立刻补拉一次，所以降频不会让按钮显得迟钝。
 */
export function pollDelay(state: PlaybackState): number {
  return state.playState === "playing" || state.pitchRendering ? STATE_INTERVAL : IDLE_INTERVAL;
}

export function usePlaybackState(enabled: boolean): PlaybackState {
  const [state, setState] = useState<PlaybackState>(EMPTY_STATE);

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    // 用 setTimeout 链而不是 setInterval：间隔要跟着状态变（在播 10Hz、闲着 1Hz）
    const tick = async () => {
      clearTimeout(timer);
      let delay = STATE_INTERVAL;
      try {
        // 用 tick 而不是 state：它顺带走收听统计，首页的最近播放靠它
        const next = await api.audioTick();
        if (!alive) return;
        setState((prev) => (samePlayback(prev, next) ? prev : next));
        delay = pollDelay(next);
      } catch {
        // 轮询失败不该刷屏报错——下一拍会再试
      }
      if (alive) timer = setTimeout(() => void tick(), delay);
    };
    const kick = () => void tick();
    window.addEventListener(PLAYBACK_KICK, kick);
    void tick();
    return () => {
      alive = false;
      clearTimeout(timer);
      window.removeEventListener(PLAYBACK_KICK, kick);
    };
  }, [enabled]);

  return state;
}

/**
 * 频谱。**由画频谱的那个组件自己调用**，不要挂到 `App` 上：
 * 它在播放时每秒变 20 次，挂在顶层的话整棵树跟着重渲染 20 次。
 * 组件没挂载（设置里关了频谱）就不拉；页面不可见时也不拉。
 */
export function useSpectrum(enabled: boolean, active: boolean): number[] {
  const [bands, setBands] = useState<number[]>([]);
  const [visible, setVisible] = useState(() => document.visibilityState === "visible");

  useEffect(() => {
    const onChange = () => setVisible(document.visibilityState === "visible");
    document.addEventListener("visibilitychange", onChange);
    return () => document.removeEventListener("visibilitychange", onChange);
  }, []);

  useEffect(() => {
    if (!enabled || !active || !visible) {
      // 停播时衰减到静默，而不是把最后一帧永远留在屏幕上
      setBands((prev) => (prev.length ? prev.map(() => 0) : prev));
      return;
    }
    let alive = true;
    const tick = async () => {
      try {
        const next = await api.audioSpectrum();
        if (alive) setBands(next);
      } catch {
        /* 同上 */
      }
    };
    void tick();
    const timer = setInterval(() => void tick(), SPECTRUM_INTERVAL);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [enabled, active, visible]);

  return bands;
}

/** 一行有时间轴的歌词：它在 `lines` 里的下标，和它的开始秒数。 */
export interface TimedLine {
  idx: number;
  time: number;
}

/** 挑出有时间轴的行。按 `lines` 记忆化，不要每拍重建。 */
export function timedLines(lines: LyricLine[]): TimedLine[] {
  return lines
    .map((line, idx) => ({ idx, time: line.timeSec ?? Number.NaN }))
    .filter((entry) => Number.isFinite(entry.time));
}

/**
 * 位置落在哪一行。返回 -1 表示没有当前行。
 *
 * `positionSec` 传 **null** 表示「这个位置不属于这份歌词」——换歌的那一瞬间，
 * 歌词已经是新歌的，而播放位置还是上一首的（轮询 10Hz，最多差一拍）。
 * 拿旧位置去二分新歌词，找到的是**最后一个 time <= 旧位置**的行，
 * 也就是新歌的末尾；跟随滚动于是把歌词整个滚到最下面。这正是
 * 「播完一首切下一首，歌词总是跑到最底下」的来源。
 */
export function pickLine(timed: TimedLine[], positionSec: number | null): number {
  if (positionSec === null || timed.length === 0) return -1;

  // 二分找最后一个 time <= position。歌词行数上千时线性扫描会在
  // 10Hz 轮询下变成可观的开销。
  let lo = 0;
  let hi = timed.length - 1;
  let found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    const entry = timed[mid];
    if (entry === undefined) break;
    if (entry.time <= positionSec) {
      found = entry.idx;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return found;
}

/**
 * 从播放位置推导当前歌词行。
 *
 * **这个推导刻意放在前端而不是音频引擎里。** 引擎只知道位置，
 * 歌词时间轴属于歌词层；把 current_line 塞进引擎会让它依赖歌词，
 * 正是要求书要拆开的耦合。
 *
 * `belongsToPlaying`：这份歌词是不是正在放的那一首。为假时没有当前行——
 * 见 [`pickLine`]。调用方（曲库页、全屏歌词）拿 `playback.songId` 和
 * 选中曲目的 id 比。
 *
 * 用 `useMemo` 而不是 ref + effect：effect 在**渲染之后**才跑，
 * 换歌那一帧读到的还是上一首的时间轴，照样会点亮错的行。
 */
export function useCurrentLine(
  lines: LyricLine[],
  positionSec: number,
  belongsToPlaying = true,
): number {
  const timed = useMemo(() => timedLines(lines), [lines]);
  return useMemo(
    () => pickLine(timed, belongsToPlaying ? positionSec : null),
    [timed, positionSec, belongsToPlaying],
  );
}

/**
 * 单句循环：把循环区间设成某一行的起止。
 *
 * 行的结束时间 = 下一行的开始时间。最后一行没有下一行，
 * 用曲目时长兜底；再没有就退 5 秒——总比不能循环好。
 */
export function useLineLoop(lines: LyricLine[], durationSec: number | null) {
  return useCallback(
    async (lineIdx: number): Promise<boolean> => {
      const line = lines[lineIdx];
      if (!line || line.timeSec === null) return false;
      const start = line.timeSec;

      let end: number | null = null;
      for (let i = lineIdx + 1; i < lines.length; i += 1) {
        const next = lines[i];
        if (next?.timeSec != null && next.timeSec > start) {
          end = next.timeSec;
          break;
        }
      }
      if (end === null) end = durationSec ?? start + 5;

      return api.audioSetLoop(start, end);
    },
    [lines, durationSec],
  );
}
