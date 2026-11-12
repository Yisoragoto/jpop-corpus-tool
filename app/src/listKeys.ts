/**
 * 一长列按钮（曲库左栏的音乐人 / 专辑 / 歌曲）里用 ↑↓ / Home / End 走，不用按几百下 Tab。
 *
 * 只在**用键盘移过去**的时候接管：鼠标点过一项之后焦点也留在上面，这时 ↑↓ 还是全局的音量
 * （和 `useHotkeys` 对按钮的约定一样）。接管时 preventDefault，全局快捷键看到就不再处理。
 */

import type { KeyboardEvent } from "react";

import { keyboardFocused } from "./useHotkeys";

/** 从第 `at` 项按这个键该去第几项；不是这几个键、或者已经在头 / 尾，返回 null */
export function listStep(key: string, at: number, count: number): number | null {
  if (count === 0 || at < 0) return null;
  let next: number;
  switch (key) {
    case "ArrowDown": next = Math.min(count - 1, at + 1); break;
    case "ArrowUp": next = Math.max(0, at - 1); break;
    case "Home": next = 0; break;
    case "End": next = count - 1; break;
    default: return null;
  }
  return next === at ? null : next;
}

/** 挂在列表容器的 onKeyDown 上。`itemSelector` 选出列表里算「一项」的元素 */
export function listArrowKeys(e: KeyboardEvent<HTMLElement>, itemSelector: string): void {
  if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
  const target = e.target instanceof Element ? e.target.closest(itemSelector) : null;
  if (target === null || !e.currentTarget.contains(target) || !keyboardFocused(target)) return;
  const items = Array.from(e.currentTarget.querySelectorAll<HTMLElement>(itemSelector)).filter((el) => !el.hasAttribute("disabled"));
  const next = listStep(e.key, items.indexOf(target as HTMLElement), items.length);
  if (next === null) {
    // 到头了的 ↑↓ 也吃掉：不然在第一项上按 ↑ 会突然变成调音量
    if (e.key === "ArrowDown" || e.key === "ArrowUp") e.preventDefault();
    return;
  }
  e.preventDefault();
  items[next]?.focus();
}
