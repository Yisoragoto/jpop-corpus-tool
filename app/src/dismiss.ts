/**
 * 轻量弹出层（下拉、浮出面板、「⋯」菜单）怎么关。全应用共用这一个，行为才一致：
 *
 * - 点到外面：关；
 * - 按 Esc：关，焦点回到打开它的那个按钮。这一下 Esc 到此为止，不再传给外层——
 *   不然全屏歌词里收起「显示」面板时会连全屏一起退出；
 * - Tab 走出去（焦点落到外面）：关。原来面板会留在原地开着，挡住下面的东西。
 *
 * `ref` 要同时包住触发按钮和弹出的面板。触发按钮带 `aria-expanded` 的话，Esc 之后焦点回它；
 * 没带就回到打开前的焦点。整页的弹窗（要拦住 Tab、遮住背景的）用 `useModalFocus`，不是这个。
 */

import { useEffect, useRef, type RefObject } from "react";

export function useDismiss(ref: RefObject<HTMLElement | null>, open: boolean, close: () => void) {
  // close 常常是每次渲染新建的箭头函数：记在 ref 里，监听只在开合时挂一次
  const closeRef = useRef(close);
  closeRef.current = close;

  useEffect(() => {
    if (!open) return;
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const inside = (target: EventTarget | null) => target instanceof Node && ref.current !== null && ref.current.contains(target);

    const onPointer = (e: PointerEvent) => {
      if (!inside(e.target)) closeRef.current();
    };
    const onFocus = (e: FocusEvent) => {
      if (!inside(e.target)) closeRef.current();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.isComposing) return;
      e.preventDefault();
      e.stopPropagation();
      const host = ref.current;
      // 焦点在面板里（或者哪儿都不在）才还给按钮；人已经点到别处去了就不抢
      const active = document.activeElement;
      if (host !== null && (active === null || active === document.body || host.contains(active))) {
        const trigger = host.querySelector<HTMLElement>("[aria-expanded]") ?? (before !== null && before.isConnected ? before : null);
        trigger?.focus();
      }
      closeRef.current();
    };
    document.addEventListener("pointerdown", onPointer, true);
    document.addEventListener("focusin", onFocus, true);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("pointerdown", onPointer, true);
      document.removeEventListener("focusin", onFocus, true);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [ref, open]);
}
