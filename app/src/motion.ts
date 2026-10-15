/**
 * 动效的两件小工具：读「减少动态效果」、用 View Transitions 做共享元素过渡。
 *
 * 共享元素：点歌手墙里的头像，它「飞」到下一屏顶上的位置；点一首歌，封面飞到歌词栏的标题旁。
 * 浏览器负责拍前后两张快照、在两者之间补间，我们只给前后两个元素起同一个 `view-transition-name`。
 * 名字只在过渡期间挂着：同一时刻页面上两个元素同名，整次过渡会被浏览器放弃。
 *
 * WebView2 是 Chromium，支持 `startViewTransition`；不支持的环境、或系统开了减少动态效果，
 * 直接做更新，不过渡。
 */

import { flushSync } from "react-dom";

interface ViewTransition {
  finished: Promise<void>;
}
type TransitionDocument = Document & { startViewTransition?: (update: () => void) => ViewTransition };

export function prefersReducedMotion(): boolean {
  return typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * 把一次 React 状态更新包成共享元素过渡。
 *
 * - `from`：现在屏幕上的那个元素（点的那张头像 / 封面）
 * - `to`：更新之后对应的元素，更新完再找（它可能是刚挂上去的）
 */
export function morph(update: () => void, opts: { name: string; from: Element | null | undefined; to: () => Element | null }) {
  const doc = document as TransitionDocument;
  if (typeof doc.startViewTransition !== "function" || prefersReducedMotion()) {
    update();
    return;
  }
  const from = opts.from instanceof HTMLElement ? opts.from : null;
  if (from) from.style.viewTransitionName = opts.name;
  let target: HTMLElement | null = null;
  try {
    const transition = doc.startViewTransition(() => {
      flushSync(update);
      // 旧元素可能还在（列表里被点的那一项），先摘掉名字，再挂到新元素上
      if (from) from.style.viewTransitionName = "";
      const next = opts.to();
      target = next instanceof HTMLElement ? next : null;
      if (target) target.style.viewTransitionName = opts.name;
    });
    void transition.finished.finally(() => {
      if (target) target.style.viewTransitionName = "";
      if (from) from.style.viewTransitionName = "";
    });
  } catch {
    if (from) from.style.viewTransitionName = "";
    update();
  }
}
