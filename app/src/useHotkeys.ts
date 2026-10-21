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
 *
 * **可交互元素自己处理按键。** BUG-0001：按钮上的空格被全局播放抢走，导入不能激活。
 * 输入框的空格和方向键也必须留给编辑。判断依据是事件目标，不是焦点状态——
 * 后者在 Tauri 的 webview 里不总是可靠。
 */

import { useEffect } from "react";
import { toggleFullscreen } from "./fullscreen";

import { api } from "./api";

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
}

function isInteractive(target: EventTarget | null): boolean {
  return target instanceof Element && (
    target.closest('button,a[href],input,textarea,select,[role="button"],[role="slider"],[role="radio"],[tabindex]') !== null ||
    (target instanceof HTMLElement && target.isContentEditable)
  );
}

export function useHotkeys({
  onQuickSearch,
  onCommandPalette,
  audioReady,
  positionSec,
  volume,
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

      if (e.defaultPrevented || isInteractive(e.target) || mod || e.altKey) return;
      if (!audioReady) return;

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
    return () => window.removeEventListener("keydown", onKey);
  }, [onQuickSearch, onCommandPalette, audioReady, positionSec, volume]);
}
