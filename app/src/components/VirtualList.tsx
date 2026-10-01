/**
 * 定高虚拟列表。
 *
 * 计算在 `virtual.ts`（纯函数，18 个单测），这里只负责测量容器、
 * 监听滚动、渲染那一段。
 *
 * 用法要求：**每一行必须是固定高度**，和 `itemHeight` 一致。
 * 行高不一致会表现为滚动时内容跳动——这是定高虚拟化的固有约束，
 * 换成动态高度的方案要复杂一个数量级，而 KWIC 结果本来就是单行。
 */

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

import { computeWindow, scrollOffsetFor } from "../virtual";

interface Props<T> {
  items: T[];
  itemHeight: number;
  /** 渲染一行。`index` 是在原数组里的下标，不是渲染窗口里的。 */
  renderItem: (item: T, index: number) => ReactNode;
  /** 稳定的 key。用下标当 key 在滚动时会导致 React 复用错行。 */
  itemKey: (item: T, index: number) => string;
  className?: string;
  /** 要滚到视野里的下标。变化时滚一次，之后不再干预用户的滚动。 */
  scrollToIndex?: number | null;
  empty?: ReactNode;
}

export function VirtualList<T>({
  items,
  itemHeight,
  renderItem,
  itemKey,
  className,
  scrollToIndex,
  empty,
}: Props<T>) {
  const ref = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(0);

  // 容器高度要实测。首帧拿不到（还没布局），所以用 ResizeObserver
  // 而不是只在挂载时读一次——窗口缩放、面板展开都会改变它。
  useEffect(() => {
    const node = ref.current;
    if (!node) return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setViewportHeight(entry.contentRect.height);
    });
    observer.observe(node);
    setViewportHeight(node.clientHeight);
    return () => observer.disconnect();
  }, []);

  const onScroll = useCallback(() => {
    // 直接读 DOM 而不是用事件对象：滚动事件可能被合并，
    // 读实时值才不会落后于绘制
    if (ref.current) setScrollTop(ref.current.scrollTop);
  }, []);

  useEffect(() => {
    if (scrollToIndex == null || !ref.current) return;
    const offset = scrollOffsetFor(
      scrollToIndex,
      { itemHeight, viewportHeight, scrollTop: ref.current.scrollTop },
      "center",
    );
    if (offset !== null) ref.current.scrollTop = offset;
  }, [scrollToIndex, itemHeight, viewportHeight]);

  // 列表换了内容（比如重新检索）要回到顶部，否则会停在上一批的滚动位置
  useEffect(() => {
    if (ref.current) {
      ref.current.scrollTop = 0;
      setScrollTop(0);
    }
  }, [items]);

  const window = computeWindow({
    count: items.length,
    itemHeight,
    viewportHeight,
    scrollTop,
  });

  if (items.length === 0 && empty) {
    return (
      <div className={className} ref={ref}>
        {empty}
      </div>
    );
  }

  return (
    <div className={className} ref={ref} onScroll={onScroll}>
      <div style={{ height: window.paddingTop }} />
      {items.slice(window.startIndex, window.endIndex).map((item, offset) => {
        const index = window.startIndex + offset;
        return (
          <div key={itemKey(item, index)} style={{ height: itemHeight }}>
            {renderItem(item, index)}
          </div>
        );
      })}
      <div style={{ height: window.paddingBottom }} />
    </div>
  );
}
