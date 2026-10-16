/**
 * 检索结果和播放器之间的那座桥：点一条结果就能听到那一句。
 *
 * 区间怎么算（行的结束时间、间奏怎么截、提前量）全在 Rust 的 `jp-corpus/src/segments.rs`，
 * 这里只负责「加载那首歌 → 从区间起点放 → 到区间终点停」。精度只到**行**：
 * 库里的时间戳来自 LRC，没有词级时间，所以不假装能跳到词本身。
 */

import { api, type PlaybackState } from "./api";
import type { LibraryActions } from "./useLibrary";

export interface AuditionOptions {
  /** 往前多带几行（试听前文） */
  before?: number;
  /** 往后多带几行 */
  after?: number;
}

/**
 * 试听一行（可带前后文）。返回 false 表示这一行没有时间戳，放不了。
 *
 * 走 `openTrackById` 而不是直接 `audioLoad`：播放条显示的是「当前选中的歌」，
 * 只换音频不换选中，播放条上的曲名就是错的；顺带曲库页回去时也正好停在这一行。
 */
export async function audition(
  actions: LibraryActions,
  utteranceId: number,
  opts: AuditionOptions = {},
): Promise<boolean> {
  const window = await api.listenWindow(utteranceId, opts.before ?? 0, opts.after ?? 0);
  if (!window) return false;
  const opened = await actions.openTrackById(window.songId, {
    scrollTo: utteranceId,
    seekTo: window.startSec,
    autoPlay: true,
  });
  if (!opened) return false;
  await api.audioSetStopAt(window.endSec);
  return true;
}

/** 打开那首歌并从这一行接着往下放（不停）。KWIC 里单击一条就是这个 */
export async function playFrom(
  actions: LibraryActions,
  songId: string,
  utteranceId: number,
  timeSec: number | null,
): Promise<boolean> {
  return actions.openTrackById(songId, {
    scrollTo: utteranceId,
    // 有时间戳就提前一点起播，听得到起音；没有就只定位歌词、不放
    seekTo: timeSec === null ? null : Math.max(0, timeSec - 0.4),
    autoPlay: timeSec !== null,
  });
}

/** 某一段是不是正在放。KWIC 里正在播的那一条要高亮 */
export function isPlayingSpan(
  state: Pick<PlaybackState, "songId" | "playState" | "positionSec">,
  songId: string,
  startSec: number | null,
  endSec: number | null,
): boolean {
  if (state.songId !== songId || state.playState !== "playing" || startSec === null) return false;
  const end = endSec ?? startSec + 5;
  return state.positionSec >= startSec - 0.5 && state.positionSec < end;
}
