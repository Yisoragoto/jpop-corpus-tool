/**
 * 设置页左边那一列目录。
 *
 * 设置页已经有七组、三十多行，要找「曲库维护」得一路滚下去。左边钉一列组名，
 * 点一下跳过去，滚动时当前这一组高亮——和 Windows 11 设置里左边那一列一个意思。
 *
 * 滚动容器是 `.page` 自己（`overflow-y: auto`），所以这里监听的是它，
 * 不是 window；用 `requestAnimationFrame` 合并一帧内的多次 scroll 事件。
 */

import { useCallback, useEffect, useRef, useState } from "react";

export interface TocItem {
  id: string;
  label: string;
}

/** 组标题顶到容器上沿往下多少像素就算「当前这一组」 */
const ACTIVE_OFFSET = 72;

export function SettingsToc({ items }: { items: TocItem[] }) {
  const [active, setActive] = useState(items[0]?.id ?? "");
  const navRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const page = navRef.current?.closest(".page");
    if (!(page instanceof HTMLElement)) return;

    let frame = 0;
    const measure = () => {
      frame = 0;
      const top = page.getBoundingClientRect().top + ACTIVE_OFFSET;
      let current = items[0]?.id ?? "";
      for (const item of items) {
        const el = document.getElementById(item.id);
        if (el !== null && el.getBoundingClientRect().top <= top) current = item.id;
      }
      // 滚到底时最后一组可能永远够不到那条线，直接认它
      if (page.scrollTop + page.clientHeight >= page.scrollHeight - 2) {
        current = items[items.length - 1]?.id ?? current;
      }
      setActive(current);
    };
    const onScroll = () => {
      if (frame === 0) frame = requestAnimationFrame(measure);
    };

    measure();
    page.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      page.removeEventListener("scroll", onScroll);
      if (frame !== 0) cancelAnimationFrame(frame);
    };
  }, [items]);

  const jump = useCallback((id: string) => {
    const el = document.getElementById(id);
    if (el === null) return;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    el.scrollIntoView({ behavior: reduced ? "auto" : "smooth", block: "start" });
    setActive(id);
  }, []);

  return (
    <nav className="settings-toc" ref={navRef} aria-label="设置目录">
      {items.map((item) => (
        <button
          key={item.id}
          type="button"
          className={item.id === active ? "settings-toc-item active" : "settings-toc-item"}
          onClick={() => jump(item.id)}
          aria-current={item.id === active ? "true" : undefined}
        >
          {item.label}
        </button>
      ))}
    </nav>
  );
}
