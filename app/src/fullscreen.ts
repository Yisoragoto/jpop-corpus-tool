/**
 * 窗口真正的全屏（盖住任务栏），不是只铺满窗口。
 *
 * 几个地方都可能要全屏：词云的演示、从词云点歌进去的全屏歌词。各自「占用」一下，
 * 都放手了才退出——从演示里点歌进全屏歌词、再退出歌词时，窗口还得是全屏的。
 *
 * 走 Tauri 的窗口接口；在浏览器里（开发用的 mock）退回 Fullscreen API，再不行就算了。
 */

import { getCurrentWindow } from "@tauri-apps/api/window";

const holders = new Set<string>();

async function apply(on: boolean) {
  try {
    await getCurrentWindow().setFullscreen(on);
    return;
  } catch {
    // 不在 Tauri 里
  }
  try {
    if (on && document.fullscreenElement === null) await document.documentElement.requestFullscreen();
    if (!on && document.fullscreenElement !== null) await document.exitFullscreen();
  } catch {
    // 浏览器不让（没有用户手势之类），就保持窗口状态
  }
}

export function holdFullscreen(key: string) {
  const was = holders.size > 0;
  holders.add(key);
  if (!was) void apply(true);
}

export function releaseFullscreen(key: string) {
  if (!holders.delete(key)) return;
  if (holders.size === 0) void apply(false);
}

export function isFullscreenHeld(key: string): boolean {
  return holders.has(key);
}
