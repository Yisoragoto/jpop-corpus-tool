/**
 * 整数输入框。直接把 `Number(输入)` 写回状态的那种框有两个毛病：
 *
 * - 删不空：删掉 2 想输 3，空串当场被换成默认值 1，再敲 3 就成了 13；
 * - 不管范围：`min` / `max` 只管上下箭头，手打的 0 或 99 照样报上去。
 *
 * 这里自己留一份草稿：打字时随便改（可以是空的）；草稿是范围里的整数就立刻生效，
 * 否则等回车 / 离开输入框时收回范围里——空的回到原来的值。
 */

import { useEffect, useState, type CSSProperties } from "react";

interface Props {
  value: number;
  min: number;
  max: number;
  onCommit: (value: number) => void;
  ariaLabel?: string;
  className?: string;
  style?: CSSProperties;
  disabled?: boolean;
  step?: number;
}

/** 草稿 → 范围里的整数；不是数字（空的、只有负号）返回 null */
export function clampDraft(draft: string, min: number, max: number): number | null {
  const text = draft.trim();
  if (!/^-?\d+$/.test(text)) return null;
  return Math.min(max, Math.max(min, Number(text)));
}

export function NumberInput({ value, min, max, onCommit, ariaLabel, className, style, disabled, step }: Props) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);

  const settle = () => {
    const next = clampDraft(draft, min, max);
    if (next === null) {
      setDraft(String(value));
      return;
    }
    setDraft(String(next));
    if (next !== value) onCommit(next);
  };

  return (
    <input
      type="number"
      className={className}
      style={style}
      min={min}
      max={max}
      step={step}
      aria-label={ariaLabel}
      disabled={disabled}
      value={draft}
      onChange={(e) => {
        const text = e.target.value;
        setDraft(text);
        // 已经是范围里的整数就不用等：依赖它的按钮、预览马上跟上
        const next = clampDraft(text, min, max);
        if (next !== null && String(next) === text.trim() && next !== value) onCommit(next);
      }}
      onBlur={settle}
      onKeyDown={(e) => {
        if (e.key === "Enter") settle();
      }}
    />
  );
}
