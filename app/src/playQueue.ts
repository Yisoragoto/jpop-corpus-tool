/**
 * 播放队列：按了哪个列表的「播放」，就顺着那个列表往下放。
 *
 * 引擎（Rust）一次只管一首歌，放完就停在 `ended`。队列记在前端：看到正在放的那首放完了，
 * 就加载下一首。挂在 App 上而不是曲库页里——切到别的页面，歌也要接着往下放。
 *
 * 不经过队列的播放（检索页试听一句、首页继续听、词的例句）不动队列：那首放完不会接着放别的，
 * 「下一首」仍然回到队列里。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { api, type PlaybackState, type Track } from "./api";
import { messageOf } from "./errors";

export interface PlayQueue {
  tracks: Track[];
  /** 队列里正在放（或者上一次放）的是第几首；空队列是 -1 */
  index: number;
}

export const EMPTY_QUEUE: PlayQueue = { tracks: [], index: -1 };

/** 上一首：放了超过这么多秒，先回到这首的开头（播放器的老规矩） */
export const RESTART_AFTER_SEC = 3;

/** 从 `from` 往 `step` 方向找下一首有音频的；没有返回 null。不绕回开头：放完最后一首就停 */
export function neighbor(tracks: readonly Track[], from: number, step: 1 | -1): number | null {
  for (let i = from + step; i >= 0 && i < tracks.length; i += step) {
    if (tracks[i]?.audioPath) return i;
  }
  return null;
}

/** 从第 `start` 首起，第一首有音频的（点的那首没有音频就顺延）；整个列表都没有返回 null */
export function firstPlayable(tracks: readonly Track[], start: number): number | null {
  const at = Math.max(0, start);
  if (tracks[at]?.audioPath) return at;
  return neighbor(tracks, at, 1) ?? neighbor(tracks, at, -1);
}

export interface PlayQueueControls {
  queue: PlayQueue;
  /** 放这个列表，从第 `start` 首起。列表就成了新的队列 */
  playList: (tracks: Track[], start?: number) => void;
  /** 下一首；队列里没有下一首时是 null（按钮据此禁用） */
  next: (() => void) | null;
  /** 上一首 / 回到这首开头；没有队列时是 null */
  previous: (() => void) | null;
  /** 下一首是谁（按钮的提示里写） */
  upNext: Track | null;
}

export function usePlayQueue(playback: PlaybackState, audioReady: boolean, onError: (message: string) => void): PlayQueueControls {
  const [queue, setQueue] = useState<PlayQueue>(EMPTY_QUEUE);
  const queueRef = useRef(queue);
  queueRef.current = queue;
  const playbackRef = useRef(playback);
  playbackRef.current = playback;

  const playAt = useCallback(
    (tracks: Track[], index: number) => {
      const track = tracks[index];
      if (track === undefined) return;
      setQueue({ tracks, index });
      void api.audioLoad(track.id, { positionSec: null, autoplay: true }).catch((err: unknown) => onError(messageOf(err)));
    },
    [onError],
  );

  const playList = useCallback<PlayQueueControls["playList"]>(
    (tracks, start = 0) => {
      if (!audioReady) return;
      const at = firstPlayable(tracks, start);
      if (at === null) return;
      playAt(tracks, at);
    },
    [audioReady, playAt],
  );

  // 队列里正在放的那首放完了：接着放下一首。每首只接一次——加载下一首要一点时间，
  // 这期间状态还停在「上一首 ended」上
  const advancedFrom = useRef<string | null>(null);
  useEffect(() => {
    if (playback.playState !== "ended") {
      advancedFrom.current = null;
      return;
    }
    const current = queue.tracks[queue.index];
    if (current === undefined || current.id !== playback.songId || advancedFrom.current === current.id) return;
    const at = neighbor(queue.tracks, queue.index, 1);
    if (at === null) return;
    advancedFrom.current = current.id;
    playAt(queue.tracks, at);
  }, [playback.playState, playback.songId, queue, playAt]);

  const nextIndex = useMemo(() => neighbor(queue.tracks, queue.index, 1), [queue]);
  const next = useMemo(
    () => (audioReady && nextIndex !== null ? () => playAt(queueRef.current.tracks, nextIndex) : null),
    [audioReady, nextIndex, playAt],
  );
  const hasQueue = queue.index >= 0;
  const previous = useMemo(
    () =>
      audioReady && hasQueue
        ? () => {
            const q = queueRef.current;
            const now = playbackRef.current;
            const inQueue = q.tracks[q.index]?.id === now.songId;
            const before = neighbor(q.tracks, q.index, -1);
            // 放了一阵、或者前面没有歌了：回到这首开头
            if (inQueue && (now.positionSec > RESTART_AFTER_SEC || before === null)) void api.audioSeek(0).catch(() => undefined);
            else if (before !== null) playAt(q.tracks, inQueue ? before : q.index);
            else playAt(q.tracks, q.index);
          }
        : null,
    [audioReady, hasQueue, playAt],
  );

  return { queue, playList, next, previous, upNext: nextIndex !== null ? (queue.tracks[nextIndex] ?? null) : null };
}
