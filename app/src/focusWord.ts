/**
 * 「正在看的那个词」：最近一次在「词的去处」里打开的词。
 *
 * 底部的「正在唱」字幕靠它把正在唱的那句里的这个词标亮——人从分析页点了「風」、
 * 再点一首歌开始放，听到的那一句里「風」就是亮的。和词云下面的字幕同一个意思。
 *
 * 记在模块里：几个页面各自打开面板，字幕在 App 那一层，谁也不用把它往上传。
 */

import { useSyncExternalStore } from "react";

let variants: readonly string[] = [];
const listeners = new Set<() => void>();

/** 打开一个词时调用；传空数组清掉 */
export function setFocusWord(next: readonly string[]) {
  if (next.length === variants.length && next.every((v, i) => v === variants[i])) return;
  variants = next;
  for (const fn of listeners) fn();
}

export function useFocusWord(): readonly string[] {
  return useSyncExternalStore(
    (fn) => {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    () => variants,
  );
}
