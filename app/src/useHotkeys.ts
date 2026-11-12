/**
 * 全局键盘交互（要求书第十四条）。
 *
 * | 键 | 行为 |
 * |---|---|
 * | Space | 播放 / 暂停 |
 * | ← → | 后退 / 前进 5 秒 |
 * | ↑ ↓ | 音量 ±5% |
 * | Ctrl/Cmd+K | 全局搜索 |
 * | Ctrl/Cmd+P | 命令面板 |
 * | Ctrl/Cmd+L | 循环正在唱的这一句 / 取消循环 |
 * | [ ] | 循环时起点提前 / 推后 0.1 秒（按着 Alt 0.5 秒） |
 * | Shift+[ ] | 循环时终点提前 / 推后 |
 *
 * **输入框、滑块、用键盘移过去的按钮自己处理按键**（见 `keepsKey`）。判断依据是事件目标，
 * 不是焦点状态——后者在 Tauri 的 webview 里不总是可靠。
 */

import { useEffect } from "react";
import { toggleFullscreen } from "./fullscreen";

import { api, type LoopRegion as PlayerLoop } from "./api";
import { activeLineLoop, getLineLoopRegion, lineAt, loopKeyChange, nudgeLineLoop, startLineLoop, stopLineLoop } from "./lineLoop";

/** 方向键的跳转步长。5 秒是听写场景下「刚才那句没听清」的合适粒度。 */
const SEEK_STEP_SEC = 5;

/** 音量步长。 */
const VOLUME_STEP = 0.05;

export interface HotkeyHandlers {
  onQuickSearch: () => void;
  onCommandPalette: () => void;
  /** 引擎不可用时播放类快捷键要禁用，而不是发出去等着报错 */
  audioReady: boolean;
  positionSec: number;
  volume: number;
  /** 正在放的歌和播放器里的循环：循环快捷键用 */
  songId: string;
  loopRegion: PlayerLoop | null;
}

const TYPING = 'input:not([type="checkbox"]):not([type="radio"]):not([type="button"]):not([type="submit"]),textarea,select';
/** 自己用方向键的控件：滑块、列表、单选组 */
const ARROW_OWNERS = 'input[type="range"],[role="slider"],[role="listbox"],[role="tree"],[role="tablist"],[role="radiogroup"],[role="radio"],[role="menu"]';
/** 空格 / 回车会「按下」的控件 */
const ACTIVATABLE = 'button,a[href],summary,input[type="checkbox"],input[type="radio"],input[type="button"],input[type="submit"],[role="button"],[role="checkbox"],[role="switch"],[role="menuitem"],[role="tab"],[tabindex]';

/**
 * 焦点是不是鼠标点出来的。不能在 keydown 里问 `:focus-visible`：一按键，Chromium 就把当前焦点
 * 标成 focus-visible，问出来永远是「键盘聚焦」。所以自己记：按下鼠标后紧跟着的那次聚焦算鼠标的。
 */
let pointerDownAt = -Infinity;
let pointerFocused: EventTarget | null = null;

export function trackFocusOrigin(): () => void {
  const onPointer = () => {
    pointerDownAt = performance.now();
  };
  // 按了键（Tab 之类）之后的聚焦就不算鼠标的了
  const onKey = () => {
    pointerDownAt = -Infinity;
  };
  const onFocus = (e: FocusEvent) => {
    pointerFocused = performance.now() - pointerDownAt < 300 ? e.target : null;
  };
  window.addEventListener("pointerdown", onPointer, true);
  window.addEventListener("focusin", onFocus, true);
  window.addEventListener("keydown", onKey, true);
  return () => {
    window.removeEventListener("keydown", onKey, true);
    window.removeEventListener("pointerdown", onPointer, true);
    window.removeEventListener("focusin", onFocus, true);
  };
}

export function keyboardFocused(element: Element): boolean {
  return element !== pointerFocused;
}

/**
 * 这个键该不该留给焦点所在的元素，而不是当全局快捷键。
 *
 * - 输入框里打字：所有键都留给它（空格是打字、方向键是移动光标）；
 * - 滑块、列表这类自己用方向键的控件：方向键留给它（分段按钮例外：只有用键盘移过去时才留）；
 * - 按钮这类会被空格「按下」的：只有**用键盘移过去**时才把空格留给它——
 *   BUG-0001 是用 Tab 移到「导入」上按空格没反应。用鼠标点过的按钮焦点也停在上面，
 *   这时空格还是播放 / 暂停，方向键还是快进快退、音量，否则点一下播放键快捷键就全失灵了。
 */
export function keepsKey(target: EventTarget | null, code: string, byKeyboard: (element: Element) => boolean = keyboardFocused): boolean {
  if (typeof Element === "undefined" || !(target instanceof Element)) return false;
  if (target instanceof HTMLElement && target.isContentEditable) return true;
  if (target.closest(TYPING) !== null) return true;
  if (code.startsWith("Arrow")) {
    const owner = target.closest(ARROW_OWNERS);
    if (owner === null) return false;
    // 分段按钮（单选组）和普通按钮一样：鼠标点过之后焦点留在上面，方向键还是快进快退、音量；
    // 用键盘移过去的才拿方向键在选项之间走
    return owner.matches('[role="radio"]') ? byKeyboard(owner) : true;
  }
  if (code === "Space" || code === "Enter") {
    const control = target.closest(ACTIVATABLE);
    return control !== null && byKeyboard(control);
  }
  return false;
}

export function useHotkeys({
  onQuickSearch,
  onCommandPalette,
  audioReady,
  positionSec,
  volume,
  songId,
  loopRegion,
}: HotkeyHandlers): void {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;

      // F11：整个软件全屏，再按一次退出（和浏览器、资源管理器一样）
      if (e.key === "F11") {
        e.preventDefault();
        toggleFullscreen();
        return;
      }

      // 面板快捷键在输入框里也要能用——正在搜索时想切命令模式是合理的
      if (mod && (e.key === "k" || e.key === "K")) {
        e.preventDefault();
        onQuickSearch();
        return;
      }
      if (mod && (e.key === "p" || e.key === "P")) {
        e.preventDefault();
        onCommandPalette();
        return;
      }

      if (e.defaultPrevented || keepsKey(e.target, e.code)) return;
      if (!audioReady) return;

      if (mod && !e.altKey && !e.shiftKey && e.code === "KeyL") {
        e.preventDefault();
        if (loopRegion) void stopLineLoop();
        else if (songId) {
          void lineAt(songId, positionSec).then((id) => (id === null ? false : startLineLoop(id)));
        }
        return;
      }
      // 循环区间微调：只在循环着一段歌词时生效，别的时候 [ ] 不占
      const change = mod ? null : loopKeyChange(e);
      if (change && activeLineLoop(getLineLoopRegion(), loopRegion, songId)) {
        e.preventDefault();
        void nudgeLineLoop(change);
        return;
      }

      if (mod || e.altKey) return;

      switch (e.code) {
        case "Space":
          e.preventDefault();
          void api.audioToggle();
          break;
        case "ArrowLeft":
          e.preventDefault();
          void api.audioSeek(Math.max(0, positionSec - SEEK_STEP_SEC));
          break;
        case "ArrowRight":
          e.preventDefault();
          void api.audioSeek(positionSec + SEEK_STEP_SEC);
          break;
        case "ArrowUp":
          e.preventDefault();
          void api.audioSetVolume(Math.min(1, volume + VOLUME_STEP));
          break;
        case "ArrowDown":
          e.preventDefault();
          void api.audioSetVolume(Math.max(0, volume - VOLUME_STEP));
          break;
        default:
          break;
      }
    };

    window.addEventListener("keydown", onKey);
    const untrack = trackFocusOrigin();
    return () => {
      window.removeEventListener("keydown", onKey);
      untrack();
    };
  }, [onQuickSearch, onCommandPalette, audioReady, positionSec, volume, songId, loopRegion]);
}
