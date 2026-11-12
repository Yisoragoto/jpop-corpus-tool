/**
 * 年份输入框：只收数字，写满四位才算数。
 *
 * 每敲一个字就往上报的话，输 2010 会按 2、20、201 各查一遍；不是四位数又会被悄悄当成没填。
 * 所以这里自己留一份草稿：满四位立刻提交；清空、或者写了一半，等回车 / 离开输入框时再说——
 * 空的提交成「不限」，写了一半的标红、不提交（上面生效的还是原来那个年份）。
 */

import { useEffect, useState } from "react";

import { tr } from "../i18n";

interface Props {
  /** 生效的年份：四位数字，或者空串（不限） */
  value: string;
  onCommit: (value: string) => void;
  ariaLabel: string;
  placeholder?: string;
  disabled?: boolean;
}

/** 草稿能不能提交：空（不限）或者四位数字 */
export function yearDraftOk(draft: string): boolean {
  return draft === "" || /^\d{4}$/.test(draft);
}

export function YearInput({ value, onCommit, ariaLabel, placeholder, disabled }: Props) {
  const [draft, setDraft] = useState(value);
  /** 写了一半就想提交（回车、离开）过：这时才标红，正打着字不标 */
  const [flagged, setFlagged] = useState(false);
  // 外面换了生效的值（从别的页面带着年份跳过来、切了子语料）：草稿跟上
  useEffect(() => {
    setDraft(value);
    setFlagged(false);
  }, [value]);

  const commit = () => {
    if (!yearDraftOk(draft)) {
      setFlagged(true);
      return;
    }
    if (draft !== value) onCommit(draft);
  };
  const invalid = flagged && !yearDraftOk(draft);

  return (
    <input
      type="text"
      className="year-input"
      inputMode="numeric"
      maxLength={4}
      placeholder={placeholder ?? tr("年份")}
      aria-label={ariaLabel}
      aria-invalid={invalid || undefined}
      title={invalid ? tr("年份要写四位数") : undefined}
      disabled={disabled}
      value={draft}
      onChange={(e) => {
        const next = e.target.value.replace(/\D/g, "").slice(0, 4);
        setDraft(next);
        if (next.length === 4) {
          setFlagged(false);
          if (next !== value) onCommit(next);
        }
      }}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
      }}
    />
  );
}
