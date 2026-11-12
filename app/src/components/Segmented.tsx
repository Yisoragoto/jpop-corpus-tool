/**
 * 分段按钮：几个选项里选一个。全应用共用这一个，样子和键盘行为才一致。
 *
 * 读屏读成单选组；Tab 只停在选中的那一项上，←→（↑↓）/ Home / End 在项之间走并直接选中——
 * 和系统的单选按钮一样。只有用键盘移过去时才这样：鼠标点过之后方向键还是全局的快进快退。
 *
 * 能多选的那种（词云的词性）不是单选组，不走这里：那边是一排 `aria-pressed` 的按钮。
 */

import type { CSSProperties, KeyboardEvent, ReactNode } from "react";

import { keyboardFocused } from "../useHotkeys";

export interface SegmentedOption<T extends string | number> {
  value: T;
  label: ReactNode;
  /** 悬停说明 */
  title?: string;
  /** 只有图标的项要给：读屏读这个 */
  ariaLabel?: string;
  disabled?: boolean;
}

interface Props<T extends string | number> {
  value: T;
  options: readonly SegmentedOption<T>[];
  onChange: (value: T) => void;
  /** 这一组是选什么的。读屏读它；界面上不显示 */
  label: string;
  /** 加在 `.segmented` 后面的类名（`small-seg` 之类） */
  className?: string;
  /** 选中的底板滑过去而不是硬切（左栏的分类、时间轴的方向） */
  slide?: boolean;
  /** 不套 `.segmented` 的外观，只要行为：自己有样式的那几处（档案页的「横条 / 大封面」图标） */
  bare?: boolean;
  disabled?: boolean;
}

/** 从 `from` 往 `step` 方向找下一个能选的项，到头绕回去；一个都没有返回 -1 */
export function nextEnabled(disabled: readonly boolean[], from: number, step: 1 | -1): number {
  const n = disabled.length;
  for (let i = 1; i <= n; i += 1) {
    const at = (((from + step * i) % n) + n) % n;
    if (!disabled[at]) return at;
  }
  return -1;
}

export function Segmented<T extends string | number>({ value, options, onChange, label, className = "", slide = false, bare = false, disabled = false }: Props<T>) {
  const selected = options.findIndex((o) => o.value === value);
  const off = options.map((o) => disabled || o.disabled === true);
  // 没有哪项选中（值不在选项里）时，Tab 停在第一个能选的上
  const tabStop = selected >= 0 && !off[selected] ? selected : off.indexOf(false);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.altKey || e.ctrlKey || e.metaKey) return;
    const buttons = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>('[role="radio"]'));
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    // 鼠标点出来的焦点不算：这时方向键留给全局的快进快退、音量（见 `useHotkeys` 的 `keepsKey`）
    if (at < 0 || !keyboardFocused(buttons[at] as Element)) return;
    let next: number;
    switch (e.key) {
      case "ArrowRight":
      case "ArrowDown": next = nextEnabled(off, at, 1); break;
      case "ArrowLeft":
      case "ArrowUp": next = nextEnabled(off, at, -1); break;
      case "Home": next = nextEnabled(off, -1, 1); break;
      case "End": next = nextEnabled(off, options.length, -1); break;
      default: return;
    }
    e.preventDefault();
    e.stopPropagation();
    const option = options[next];
    if (next < 0 || next === at || option === undefined) return;
    buttons[next]?.focus();
    if (option.value !== value) onChange(option.value);
  };

  const style = slide ? ({ "--seg-i": Math.max(0, selected), "--seg-n": options.length } as CSSProperties) : undefined;
  return (
    <div
      className={`${bare ? "" : "segmented "}${slide ? "seg-slide " : ""}${className}`.trim()}
      role="radiogroup"
      aria-label={label}
      aria-disabled={disabled || undefined}
      style={style}
      onKeyDown={onKeyDown}
    >
      {slide && <span className="seg-thumb" aria-hidden="true" />}
      {options.map((o, i) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={i === selected}
          aria-label={o.ariaLabel}
          tabIndex={i === tabStop ? 0 : -1}
          className={i === selected ? "on" : ""}
          title={o.title}
          disabled={off[i]}
          // 点已经选中的那项也报：有的地方靠它顺手开别的开关（注音方式 → 打开注音）
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
