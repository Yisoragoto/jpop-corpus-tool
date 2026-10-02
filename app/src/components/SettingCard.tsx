/**
 * 设置页的一行（WinUI 的 SettingsCard）：左边图标，中间标题和说明，右边控件。
 *
 * 只做版面，不管状态——每一行的值都来自各自的存储（`settings.ts`、`lyricsDisplay.ts`、
 * `dict/mine.ts`），改了立刻生效、自动记住，所以没有「确定」按钮。
 *
 * `SettingGroup` 是一组的标题；同一组里的行挨在一起，圆角只在头尾两行上，
 * 视觉上是一块，和 Windows 11 设置里一样。
 */

import type { ReactNode } from "react";

export function SettingGroup({
  title,
  id,
  children,
}: {
  title?: string | undefined;
  /** 左边目录跳转用的锚点 */
  id?: string | undefined;
  children: ReactNode;
}) {
  return (
    <section className="setting-group" id={id}>
      {title !== undefined && <h2 className="setting-group-title">{title}</h2>}
      <div className="setting-rows">{children}</div>
    </section>
  );
}

interface CardProps {
  icon?: ReactNode;
  title: ReactNode;
  description?: ReactNode;
  /** 右边的控件 */
  children?: ReactNode;
  /** 整行可点（比如「打开词典页」这种）时给一个 onClick，行会有悬停反馈 */
  onClick?: () => void;
}

export function SettingCard({ icon, title, description, children, onClick }: CardProps) {
  const body = (
    <>
      {icon !== undefined && <span className="setting-icon">{icon}</span>}
      <span className="setting-text">
        <span className="setting-title">{title}</span>
        {description !== undefined && <span className="setting-desc">{description}</span>}
      </span>
      {children !== undefined && <span className="setting-control">{children}</span>}
    </>
  );
  if (onClick !== undefined) {
    return (
      <button type="button" className="setting-card clickable" onClick={onClick}>
        {body}
      </button>
    );
  }
  return <div className="setting-card">{body}</div>;
}

/** 一组设置行下面的整块说明（例：某个开关为什么默认是关的） */
export function SettingNote({ children }: { children: ReactNode }) {
  return <p className="setting-note">{children}</p>;
}

/** On / Off 开关。文字在左边、开关在右边，和 Windows 11 设置一致 */
export function Switch({
  checked,
  onChange,
  disabled = false,
  label,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  label?: string;
}) {
  return (
    <label className={`switch ${disabled ? "disabled" : ""}`} title={label}>
      <span className="switch-state">{checked ? "On" : "Off"}</span>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
        aria-label={label}
      />
      <span className="switch-track" aria-hidden="true">
        <span className="switch-knob" />
      </span>
    </label>
  );
}

/** 设置行右边的数字条（字号、虚化强度这类） */
export function SettingSlider({
  value,
  limits: [min, max],
  onChange,
  unit = "px",
}: {
  value: number;
  limits: readonly [number, number];
  onChange: (value: number) => void;
  unit?: string;
}) {
  return (
    <span className="setting-slider">
      <input type="range" min={min} max={max} step={1} value={value} onChange={(event) => onChange(Number(event.target.value))} />
      <span className="setting-value">
        {value}
        {unit}
      </span>
    </span>
  );
}

/** 设置行右边的下拉 */
export function SettingSelect<T extends string>({
  value,
  options,
  onChange,
  disabled = false,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
  disabled?: boolean;
}) {
  return (
    <select
      className="setting-select"
      value={value}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value as T)}
    >
      {options.map((option) => (
        <option key={option.value} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  );
}
