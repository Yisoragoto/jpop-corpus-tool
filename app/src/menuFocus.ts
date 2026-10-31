import { useEffect, type RefObject } from "react";

/**
 * 下拉菜单的键盘：打开时聚焦第一项，↑↓ / Home / End 在项之间走，Esc 关掉并回到触发按钮，
 * Tab 关掉菜单（焦点照常往后走）。关闭时只有焦点还在菜单里才还给按钮，不抢别处的焦点。
 */
export function useMenuFocus(ref: RefObject<HTMLElement | null>, open: boolean, close: () => void) {
  useEffect(() => {
    const host = ref.current;
    if (!open || !host) return;
    const trigger = host.querySelector<HTMLElement>('[aria-expanded="true"]');
    const menu = host.querySelector<HTMLElement>('[role="menu"]');
    if (!menu) return;
    const items = () => Array.from(menu.querySelectorAll<HTMLElement>('button:not(:disabled), [role="menuitem"]:not([aria-disabled="true"])'));
    items()[0]?.focus();
    const onKey = (event: KeyboardEvent) => {
      const list = items();
      const index = list.indexOf(document.activeElement as HTMLElement);
      let next: number;
      switch (event.key) {
        case "ArrowDown": next = (index + 1) % list.length; break;
        case "ArrowUp": next = (index - 1 + list.length) % list.length; break;
        case "Home": next = 0; break;
        case "End": next = list.length - 1; break;
        case "Escape": event.preventDefault(); trigger?.focus(); close(); return;
        case "Tab": close(); return;
        default: return;
      }
      event.preventDefault();
      list[next]?.focus();
    };
    menu.addEventListener("keydown", onKey);
    return () => {
      menu.removeEventListener("keydown", onKey);
      if (menu.contains(document.activeElement) || document.activeElement === document.body) trigger?.focus();
    };
    // close is an inline state setter wrapper; reopening is the only subscription boundary.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ref, open]);
}
