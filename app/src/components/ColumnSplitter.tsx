/**
 * 三栏版面里可以拖的那两道竖缝。
 *
 * 曲库页和人物页都是「左边一列 + 中间正文 + 右边一列」，宽度以前是写死的
 * （260 / 1fr / 320）。歌词长短、歌手名长短、查词栏里有没有音调图，各人各机器都不一样，
 * 所以让它能拖，并且**记住**——改一次就一直是那样。
 *
 * 记在 localStorage，按版面分开记（曲库的列表视图和网格视图是两套）。
 * 双击那道缝恢复默认。
 *
 * 拖的是 `grid-template-columns` 的第一条和最后一条轨道，中间那条永远是 `1fr`，
 * 所以窗口变宽变窄时多出来少掉的都算在正文上，两边保持用户定的宽度。
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { tr } from "../i18n";

export interface ColumnWidths {
  left: number;
  right: number;
}

export interface ColumnBounds {
  leftMin: number;
  leftMax: number;
  rightMin: number;
  rightMax: number;
  /** 中间正文至少留这么宽，拖到挤不下就不再让 */
  centerMin: number;
}

export const DEFAULT_BOUNDS: ColumnBounds = {
  leftMin: 180,
  leftMax: 560,
  rightMin: 200,
  rightMax: 680,
  centerMin: 360,
};

const PREFIX = "jpop.columns.";

/** 那道缝本身占多宽 */
export const SPLITTER = 6;

/**
 * 三栏：把两边的宽度夹进边界。中间正文不能被挤没，让步的永远是**正在拖的那一侧**
 * ——否则拖左边会把右边也带着动，手感是错的。
 *
 * `total` 是容器宽度；给 0 表示还没量到，这时只按上下限夹。
 */
export function clampColumns(
  bounds: ColumnBounds,
  total: number,
  edge: "left" | "right",
  next: ColumnWidths,
): ColumnWidths {
  const left = Math.min(Math.max(next.left, bounds.leftMin), bounds.leftMax);
  const right = Math.min(Math.max(next.right, bounds.rightMin), bounds.rightMax);
  const spare = total - bounds.centerMin - SPLITTER * 2;
  if (total === 0 || left + right <= spare) return { left, right };
  const shrink = left + right - spare;
  return edge === "left"
    ? { left: Math.max(bounds.leftMin, left - shrink), right }
    : { left, right: Math.max(bounds.rightMin, right - shrink) };
}

/**
 * 显示时用的宽度：窗口变窄、或者恢复的是上次在大窗口里拖出来的宽度时，
 * 两边按各自能让出的余量（当前宽度 − 下限）**按比例**一起收，正文至少留 `centerMin`。
 * 收不动了（两边都到下限）就停在下限，交给 CSS 的窄窗口版面去叠。
 *
 * 只影响显示，不改记下来的宽度——窗口再拉宽时会回到用户定的那个宽度。
 */
export function fitColumns(bounds: ColumnBounds, total: number, widths: ColumnWidths): ColumnWidths {
  const left = Math.min(Math.max(widths.left, bounds.leftMin), bounds.leftMax);
  const right = Math.min(Math.max(widths.right, bounds.rightMin), bounds.rightMax);
  const spare = total - bounds.centerMin - SPLITTER * 2;
  if (total === 0 || left + right <= spare) return { left, right };
  const slackLeft = left - bounds.leftMin;
  const slackRight = right - bounds.rightMin;
  const slack = slackLeft + slackRight;
  if (slack <= 0) return { left, right };
  const shrink = Math.min(left + right - spare, slack);
  return {
    left: Math.round(left - (shrink * slackLeft) / slack),
    right: Math.round(right - (shrink * slackRight) / slack),
  };
}

/** 单侧（全屏歌词的右栏）：同样的规矩，旁边那块至少留 `restMin`。 */
export function clampPane(bounds: PaneBounds, total: number, next: number): number {
  const size = Math.min(Math.max(next, bounds.min), bounds.max);
  if (total === 0) return size;
  return Math.min(size, Math.max(bounds.min, total - bounds.restMin - SPLITTER));
}

function read(key: string, fallback: ColumnWidths): ColumnWidths {
  try {
    const raw = window.localStorage.getItem(PREFIX + key);
    if (raw === null) return fallback;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return fallback;
    const { left, right } = parsed as Partial<ColumnWidths>;
    if (typeof left !== "number" || typeof right !== "number") return fallback;
    if (!Number.isFinite(left) || !Number.isFinite(right)) return fallback;
    return { left, right };
  } catch {
    // 隐私模式、禁了站点数据：当成没存过，用默认值，别让整页炸掉
    return fallback;
  }
}

function write(key: string, widths: ColumnWidths) {
  try {
    window.localStorage.setItem(PREFIX + key, JSON.stringify(widths));
  } catch {
    // 存不下就算了，这一次的拖动照样生效
  }
}

/**
 * 两边的宽度 + 拖动。`style` 直接摊到 `.columns` 上。
 *
 * `key` 换了就换一套记忆（曲库的列表 / 网格各记各的）。
 */
export function useColumnWidths(
  key: string,
  initial: ColumnWidths,
  bounds: ColumnBounds = DEFAULT_BOUNDS,
) {
  const [widths, setWidths] = useState<ColumnWidths>(() => read(key, initial));
  const total = useContainerWidth();

  // 切视图（换 key）时把那一套读回来
  useEffect(() => {
    setWidths(read(key, initial));
    // initial 是字面量对象，放进依赖会每帧变；key 变才需要重读
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  const clamp = useCallback(
    (edge: "left" | "right", next: ColumnWidths) =>
      clampColumns(bounds, total.node?.clientWidth ?? 0, edge, next),
    [bounds, total.node],
  );

  /** 实际画出来的宽度：按当前容器宽度收过的。拖动从这里起算，手感才不会跳一下 */
  const shown = useMemo(() => fitColumns(bounds, total.width, widths), [bounds, total.width, widths]);

  const drag = useCallback(
    (edge: "left" | "right", event: React.PointerEvent<HTMLDivElement>) => {
      event.preventDefault();
      const handle = event.currentTarget;
      handle.setPointerCapture(event.pointerId);
      const startX = event.clientX;
      const start = edge === "left" ? shown.left : shown.right;
      // 一开始拖就以屏幕上看到的宽度为准，另一侧也定在看到的宽度
      const base = shown;

      const move = (e: PointerEvent) => {
        const delta = e.clientX - startX;
        const size = edge === "left" ? start + delta : start - delta;
        setWidths(clamp(edge, edge === "left" ? { ...base, left: size } : { ...base, right: size }));
      };
      const done = () => {
        handle.releasePointerCapture(event.pointerId);
        handle.removeEventListener("pointermove", move);
        handle.removeEventListener("pointerup", done);
        handle.removeEventListener("pointercancel", done);
        setWidths((prev) => {
          write(key, prev);
          return prev;
        });
        document.body.classList.remove("col-resizing");
      };
      // 挂在 handle 上而不是 window：指针已经 capture 到它，拖出窗口也收得到
      handle.addEventListener("pointermove", move);
      handle.addEventListener("pointerup", done);
      handle.addEventListener("pointercancel", done);
      // 拖的时候整页用同一个光标，并且别顺手选中歌词
      document.body.classList.add("col-resizing");
    },
    [clamp, key, shown],
  );

  /** 键盘也能调：方向键一次 16px，同样从看到的宽度起算 */
  const nudge = useCallback(
    (edge: "left" | "right", step: number) => {
      const next = clamp(
        edge,
        edge === "left" ? { ...shown, left: shown.left + step } : { ...shown, right: shown.right + step },
      );
      setWidths(next);
      write(key, next);
    },
    [clamp, key, shown],
  );

  const reset = useCallback(() => {
    setWidths(initial);
    write(key, initial);
    // initial 同上，是字面量
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  const style = useMemo(
    () => ({ gridTemplateColumns: `${shown.left}px 6px minmax(0, 1fr) 6px ${shown.right}px` }),
    [shown.left, shown.right],
  );

  return { widths, shown, style, drag, nudge, reset, container: total.ref };
}

/**
 * 量容器宽度，窗口缩放、侧栏收起都跟着变。用回调 ref 而不是 useRef：
 * 页面先显示「加载中」、容器晚一步才挂上的时候，useEffect([]) 会错过它。
 */
function useContainerWidth() {
  const [node, setNode] = useState<HTMLDivElement | null>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    if (!node) return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setWidth(entry.contentRect.width);
    });
    observer.observe(node);
    setWidth(node.clientWidth);
    return () => observer.disconnect();
  }, [node]);
  const ref = useCallback((el: HTMLDivElement | null) => setNode(el), []);
  return { node, width, ref };
}

export interface PaneBounds {
  min: number;
  max: number;
  /** 旁边那块至少留这么宽 */
  restMin: number;
}

/**
 * 只有一侧可调的版面（全屏歌词：左边歌词 + 右边查词栏）。
 *
 * 和 `useColumnWidths` 同一套记忆和边界，只是这里是 flex 不是 grid，
 * 给的是 `flexBasis`。
 */
export function usePaneWidth(key: string, initial: number, bounds: PaneBounds) {
  const [width, setWidth] = useState<number>(() => read(key, { left: 0, right: initial }).right);
  const total = useContainerWidth();

  const clamp = useCallback(
    (next: number) => clampPane(bounds, total.node?.clientWidth ?? 0, next),
    [bounds, total.node],
  );
  /** 窗口变窄时按当前宽度收；记下来的值不变 */
  const shown = clampPane(bounds, total.width, width);

  const drag = useCallback(
    (_edge: "left" | "right", event: React.PointerEvent<HTMLDivElement>) => {
      event.preventDefault();
      const handle = event.currentTarget;
      handle.setPointerCapture(event.pointerId);
      const startX = event.clientX;
      const start = shown;

      const move = (e: PointerEvent) => setWidth(clamp(start - (e.clientX - startX)));
      const done = () => {
        handle.releasePointerCapture(event.pointerId);
        handle.removeEventListener("pointermove", move);
        handle.removeEventListener("pointerup", done);
        handle.removeEventListener("pointercancel", done);
        setWidth((prev) => {
          write(key, { left: 0, right: prev });
          return prev;
        });
        document.body.classList.remove("col-resizing");
      };
      handle.addEventListener("pointermove", move);
      handle.addEventListener("pointerup", done);
      handle.addEventListener("pointercancel", done);
      document.body.classList.add("col-resizing");
    },
    [clamp, key, shown],
  );

  const nudge = useCallback(
    (_edge: "left" | "right", step: number) => {
      const next = clamp(shown + step);
      setWidth(next);
      write(key, { left: 0, right: next });
    },
    [clamp, key, shown],
  );

  const reset = useCallback(() => {
    setWidth(initial);
    write(key, { left: 0, right: initial });
  }, [key, initial]);

  return { width: shown, style: { flexBasis: `${shown}px` }, drag, nudge, reset, container: total.ref };
}

interface Props {
  edge: "left" | "right";
  onDrag: (edge: "left" | "right", event: React.PointerEvent<HTMLDivElement>) => void;
  onNudge: (edge: "left" | "right", step: number) => void;
  onReset: () => void;
  label: string;
  className?: string;
}

/** 那道缝本身。6px 宽，鼠标到上面才显出一道高亮线。 */
export function ColumnSplitter({ edge, onDrag, onNudge, onReset, label, className }: Props) {
  return (
    <div
      className={`col-splitter ${className ?? ""}`}
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      tabIndex={0}
      onPointerDown={(event) => onDrag(edge, event)}
      onDoubleClick={onReset}
      onKeyDown={(event) => {
        if (event.key === "ArrowLeft") {
          event.preventDefault();
          onNudge(edge, -16);
        } else if (event.key === "ArrowRight") {
          event.preventDefault();
          onNudge(edge, 16);
        } else if (event.key === "Home") {
          event.preventDefault();
          onReset();
        }
      }}
      title={tr("{label}（拖动调整，双击恢复默认）", { label })}
    />
  );
}
