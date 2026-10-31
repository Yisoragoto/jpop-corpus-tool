/**
 * 弹层（命令面板、更新弹窗、全屏歌词、卡拉 OK）的焦点：
 *
 * - 打开时记下原来的焦点，把焦点移进弹层；
 * - 弹层外面的东西设成 `inert`：Tab 走不出去，读屏也读不到被遮住的页面；
 * - 关闭时撤掉 `inert`，焦点还给打开前的那个控件。
 *
 * 做法是从弹层往上走到 body，把沿途每一层的**兄弟节点**设 inert——弹层本身和它的祖先不动。
 * `allow` 里的元素（比如全屏歌词下面那条播放条）保持可用。
 * 只撤掉自己加的 inert，别的地方本来就 inert 的不碰，弹层叠弹层也不会互相拆台。
 */

import { useEffect, type RefObject } from "react";

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

interface Options {
  /** 打开时先聚焦它；不给就聚焦弹层里第一个能聚焦的，再没有就聚焦弹层本身 */
  initial?: RefObject<HTMLElement | null>;
  /** 弹层外面仍然可以操作的元素，按选择器找 */
  allow?: string[];
  /** false 时什么都不做（比如弹层还没真正打开） */
  active?: boolean;
}

export function useModalFocus(ref: RefObject<HTMLElement | null>, options: Options = {}) {
  const { initial, allow = [], active = true } = options;
  const allowKey = allow.join(",");

  useEffect(() => {
    const box = ref.current;
    if (!active || !box) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;

    const allowed = allowKey ? Array.from(document.querySelectorAll<HTMLElement>(allowKey)) : [];
    const touched: HTMLElement[] = [];
    const mute = (el: HTMLElement) => {
      if (el.inert) return;
      el.inert = true;
      touched.push(el);
    };
    // 弹层外的一个兄弟：整块 inert；里面藏着允许的元素时不能整块盖，往下一层再分
    const muteOutside = (el: HTMLElement) => {
      if (allowed.some((a) => a === el || a.contains(el))) return;
      if (allowed.some((a) => el.contains(a))) {
        for (const child of Array.from(el.children)) if (child instanceof HTMLElement) muteOutside(child);
        return;
      }
      mute(el);
    };

    let node: HTMLElement | null = box;
    while (node && node !== document.body) {
      const parent: HTMLElement | null = node.parentElement;
      if (!parent) break;
      for (const sibling of Array.from(parent.children)) {
        if (sibling !== node && sibling instanceof HTMLElement) muteOutside(sibling);
      }
      node = parent;
    }

    if (!box.contains(document.activeElement)) {
      const target = initial?.current ?? box.querySelector<HTMLElement>(FOCUSABLE) ?? box;
      if (target === box && !box.hasAttribute("tabindex")) box.tabIndex = -1;
      target.focus({ preventScroll: true });
    }

    return () => {
      for (const el of touched) el.inert = false;
      if (previous && previous.isConnected) previous.focus({ preventScroll: true });
    };
    // initial 是 ref，不随渲染变
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ref, active, allowKey]);
}
