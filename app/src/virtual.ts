/**
 * 定高列表虚拟化。
 *
 * KWIC 检索一个常见词能命中上千行，全部渲染成 DOM 会让滚动明显卡顿。
 * 这里只渲染视口附近的那些。
 *
 * **不引第三方库**：行高是固定的（单行、超出省略），这种情况下的窗口
 * 计算就是几行除法。引 `react-window` 或 `@tanstack/react-virtual` 是为了
 * 处理动态高度、水平滚动、粘性表头这些我们用不到的场景。
 *
 * 计算部分是纯函数，所以能测——虚拟化算错的表现是「滚动时内容跳动」
 * 或「底部有一截空白」，全靠肉眼发现的话很容易漏。
 */

export interface VirtualWindow {
  /** 要渲染的第一项下标（含） */
  startIndex: number;
  /** 要渲染的最后一项下标（不含） */
  endIndex: number;
  /** 顶部撑开的高度，让滚动条长度正确 */
  paddingTop: number;
  /** 底部撑开的高度 */
  paddingBottom: number;
  /** 内容总高度 */
  totalHeight: number;
}

export interface VirtualParams {
  /** 总项数 */
  count: number;
  /** 每项高度（px），必须是固定值 */
  itemHeight: number;
  /** 视口高度（px） */
  viewportHeight: number;
  /** 当前滚动位置（px） */
  scrollTop: number;
  /**
   * 视口上下各多渲染几项。
   *
   * 不留余量的话，快速滚动时会先看到空白再看到内容（浏览器的滚动事件
   * 落后于绘制）。3 项在 60fps 下足够遮住这个缝。
   */
  overscan?: number;
}

const DEFAULT_OVERSCAN = 3;

/**
 * 算出当前该渲染哪一段。
 *
 * 所有输入都做了防御：负的滚动位置（macOS 的橡皮筋效果）、
 * 0 或负的行高、超过内容高度的滚动位置，都不该产生越界的下标。
 */
export function computeWindow({
  count,
  itemHeight,
  viewportHeight,
  scrollTop,
  overscan = DEFAULT_OVERSCAN,
}: VirtualParams): VirtualWindow {
  const safeCount = Math.max(0, Math.floor(count));
  const safeHeight = itemHeight > 0 ? itemHeight : 1;
  const totalHeight = safeCount * safeHeight;

  if (safeCount === 0) {
    return { startIndex: 0, endIndex: 0, paddingTop: 0, paddingBottom: 0, totalHeight: 0 };
  }

  // 橡皮筋滚动会给出负值；滚过头会给出超过总高的值
  const clampedScroll = Math.min(Math.max(0, scrollTop), Math.max(0, totalHeight - 1));
  const safeViewport = Math.max(0, viewportHeight);
  const pad = Math.max(0, Math.floor(overscan));

  const firstVisible = Math.floor(clampedScroll / safeHeight);
  // +1 是因为视口顶部通常落在某一项中间，那一项也要渲染
  const visibleCount = Math.ceil(safeViewport / safeHeight) + 1;

  const startIndex = Math.max(0, firstVisible - pad);
  const endIndex = Math.min(safeCount, firstVisible + visibleCount + pad);

  return {
    startIndex,
    endIndex,
    paddingTop: startIndex * safeHeight,
    paddingBottom: Math.max(0, (safeCount - endIndex) * safeHeight),
    totalHeight,
  };
}

/**
 * 让某一项滚进视野所需的滚动位置。
 *
 * 已经在视野里就返回 null——无条件滚动会在用户手动浏览时把位置抢走。
 */
export function scrollOffsetFor(
  index: number,
  { itemHeight, viewportHeight, scrollTop }: Omit<VirtualParams, "count">,
  align: "start" | "center" | "nearest" = "nearest",
): number | null {
  const safeHeight = itemHeight > 0 ? itemHeight : 1;
  const top = index * safeHeight;
  const bottom = top + safeHeight;

  if (align === "start") return top;
  if (align === "center") return Math.max(0, top - (viewportHeight - safeHeight) / 2);

  if (top < scrollTop) return top;
  if (bottom > scrollTop + viewportHeight) return bottom - viewportHeight;
  return null;
}
