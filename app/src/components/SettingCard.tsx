/**
 * 设置页的一行（WinUI 的 SettingsCard）：左边图标，中间标题和说明，右边控件。
 *
 * 只做版面，不管状态——每一行的值都来自各自的存储（`settings.ts`、`lyricsDisplay.ts`、
 * `dict/mine.ts`），改了立刻生效、自动记住，所以没有「确定」按钮。
 *
 * `SettingGroup` 是一组的标题；同一组里的行挨在一起，圆角只在头尾两行上，
 * 视觉上是一块，和 Windows 11 设置里一样。
 */

import { createContext, type CSSProperties, useContext, useId, type ReactNode } from "react";

/**
 * 这一行的标题 / 说明的 id。行里的滑块、下拉、开关自动拿它当可访问名称，
 * 读屏才知道调的是「字号」还是「虚化强度」，各处调用不用每个都写 aria-label
 */
const RowLabel = createContext<{ titleId: string; descId: string | undefined } | null>(null);

function useRowLabel(label?: string) {
  const row = useContext(RowLabel);
  if (label !== undefined || row === null) return { "aria-label": label };
  return { "aria-labelledby": row.titleId, "aria-describedby": row.descId };
}

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
  const id = useId();
  const titleId = `${id}-title`;
  const descId = description !== undefined ? `${id}-desc` : undefined;
  const body = (
    <RowLabel.Provider value={{ titleId, descId }}>
      {icon !== undefined && <span className="setting-icon" aria-hidden="true">{icon}</span>}
      <span className="setting-text">
        <span className="setting-title" id={titleId}>{title}</span>
        {description !== undefined && <span className="setting-desc" id={descId}>{description}</span>}
      </span>
      {children !== undefined && <span className="setting-control">{children}</span>}
    </RowLabel.Provider>
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
  const name = useRowLabel(label);
  return (
    <label className={`switch ${disabled ? "disabled" : ""}`} title={label}>
      <span className="switch-state" aria-hidden="true">{checked ? "On" : "Off"}</span>
      <input
        type="checkbox"
        role="switch"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
        {...name}
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
  const name = useRowLabel();
  return (
    <span className="setting-slider">
      <input
        type="range"
        style={{ "--range-progress": `${((value - min) / (max - min)) * 100}%` } as CSSProperties}
        min={min}
        max={max}
        step={1}
        value={value}
        aria-valuetext={`${value}${unit}`}
        onChange={(event) => onChange(Number(event.target.value))}
        {...name}
      />
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
  const name = useRowLabel();
  return (
    <select
      className="setting-select"
      value={value}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value as T)}
      {...name}
    >
      {options.map((option) => (
        <option key={option.value} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  );
}
