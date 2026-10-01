/**
 * 弹出面板（照 Windows 的 Flyout）：一个按钮，点开是一列选项，选中的那条带对勾。
 *
 * 为什么不用 `<select>`：原生下拉在 WebView2 里是系统列表，和这套半透明控件对不上，
 * 而且播放条在窗口最底下，系统列表会向下弹出屏幕外。这里的面板固定向上弹。
 *
 * 点外面、按 Esc、选完都会收起。位置在挂载时按按钮的位置算一次（播放条不滚动）。
 */

import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";

export interface FlyoutOption<T> {
  value: T;
  label: string;
  /** 右侧的小字说明 */
  hint?: string;
}

interface Props<T> {
  /** 按钮上显示的当前值 */
  label: string;
  /** 按钮前面的小标题，比如「速度」 */
  caption?: string;
  title: string;
  value: T;
  options: FlyoutOption<T>[];
  disabled?: boolean;
  /** 当前值不是默认值时按钮高亮 */
  active?: boolean;
  onPick: (value: T) => void;
  /** 面板顶部的一句说明 */
  note?: ReactNode;
}

export function Flyout<T extends string | number>({
  label,
  caption,
  title,
  value,
  options,
  disabled = false,
  active = false,
  onPick,
  note,
}: Props<T>) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  // 选中的那条滚进视野：12 个半音的列表打开时应当看得见当前调
  useLayoutEffect(() => {
    if (!open) return;
    panel.current?.querySelector<HTMLElement>(".flyout-item.on")?.scrollIntoView({ block: "center" });
  }, [open]);

  return (
    <div className="flyout" ref={root}>
      <button
        className={`flyout-button ${active ? "on" : ""} ${open ? "open" : ""}`}
        onClick={() => setOpen((v) => !v)}
        disabled={disabled}
        title={title}
        aria-haspopup="listbox"
        aria-expanded={open}
      >
        {caption !== undefined && <span className="flyout-caption">{caption}</span>}
        <span className="flyout-value">{label}</span>
        <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="m6 15 6-6 6 6" />
        </svg>
      </button>

      {open && (
        <div className="flyout-panel" ref={panel} role="listbox" aria-label={title}>
          {note !== undefined && <p className="flyout-note">{note}</p>}
          {options.map((option) => (
            <button
              key={String(option.value)}
              className={`flyout-item ${option.value === value ? "on" : ""}`}
              role="option"
              aria-selected={option.value === value}
              onClick={() => {
                setOpen(false);
                if (option.value !== value) onPick(option.value);
              }}
            >
              <span className="flyout-check" aria-hidden="true">
                {option.value === value ? "✓" : ""}
              </span>
              <span className="flyout-item-label">{option.label}</span>
              {option.hint !== undefined && <span className="flyout-hint">{option.hint}</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
