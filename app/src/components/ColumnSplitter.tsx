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

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

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
  const container = useRef<HTMLDivElement | null>(null);

  // 切视图（换 key）时把那一套读回来
  useEffect(() => {
    setWidths(read(key, initial));
    // initial 是字面量对象，放进依赖会每帧变；key 变才需要重读
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  const clamp = useCallback(
    (edge: "left" | "right", next: ColumnWidths) =>
      clampColumns(bounds, container.current?.clientWidth ?? 0, edge, next),
    [bounds],
  );

  const drag = useCallback(
    (edge: "left" | "right", event: React.PointerEvent<HTMLDivElement>) => {
      event.preventDefault();
      const handle = event.currentTarget;
      handle.setPointerCapture(event.pointerId);
      const startX = event.clientX;
      const start = edge === "left" ? widths.left : widths.right;

      const move = (e: PointerEvent) => {
        const delta = e.clientX - startX;
        const size = edge === "left" ? start + delta : start - delta;
        setWidths((prev) => clamp(edge, edge === "left" ? { ...prev, left: size } : { ...prev, right: size }));
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
    [clamp, key, widths.left, widths.right],
  );

  /** 键盘也能调：方向键一次 16px */
  const nudge = useCallback(
    (edge: "left" | "right", step: number) => {
      setWidths((prev) => {
        const next = clamp(
          edge,
          edge === "left" ? { ...prev, left: prev.left + step } : { ...prev, right: prev.right + step },
        );
        write(key, next);
        return next;
      });
    },
    [clamp, key],
  );

  const reset = useCallback(() => {
    setWidths(initial);
    write(key, initial);
    // initial 同上，是字面量
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  const style = useMemo(
    () => ({ gridTemplateColumns: `${widths.left}px 6px minmax(0, 1fr) 6px ${widths.right}px` }),
    [widths.left, widths.right],
  );

  return { widths, style, drag, nudge, reset, container };
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
  const container = useRef<HTMLDivElement | null>(null);

  const clamp = useCallback(
    (next: number) => clampPane(bounds, container.current?.clientWidth ?? 0, next),
    [bounds],
  );

  const drag = useCallback(
    (_edge: "left" | "right", event: React.PointerEvent<HTMLDivElement>) => {
      event.preventDefault();
      const handle = event.currentTarget;
      handle.setPointerCapture(event.pointerId);
      const startX = event.clientX;
      const start = width;

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
    [clamp, key, width],
  );

  const nudge = useCallback(
    (_edge: "left" | "right", step: number) => {
      setWidth((prev) => {
        const next = clamp(prev + step);
        write(key, { left: 0, right: next });
        return next;
      });
    },
    [clamp, key],
  );

  const reset = useCallback(() => {
    setWidth(initial);
    write(key, { left: 0, right: initial });
  }, [key, initial]);

  return { width, style: { flexBasis: `${width}px` }, drag, nudge, reset, container };
}

interface Props {
  edge: "left" | "right";
  onDrag: (edge: "left" | "right", event: React.PointerEvent<HTMLDivElement>) => void;
  onNudge: (edge: "left" | "right", step: number) => void;
  onReset: () => void;
  label: string;
}

/** 那道缝本身。6px 宽，鼠标到上面才显出一道高亮线。 */
export function ColumnSplitter({ edge, onDrag, onNudge, onReset, label }: Props) {
  return (
    <div
      className="col-splitter"
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
      title={`${label}（拖动调整，双击恢复默认）`}
    />
  );
}
