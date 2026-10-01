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
 * **在输入框里打字时一律不抢键。** 空格是打字、方向键是移动光标，
 * 抢了会让搜索框没法用。判断依据是事件目标，不是焦点状态——
 * 后者在 Tauri 的 webview 里不总是可靠。
 */

import { useEffect } from "react";

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

function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement ||
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

      if (isTyping(e.target) || mod || e.altKey) return;
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
