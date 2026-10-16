/**
 * 选「当前子语料」的下拉框。KWIC、词云、单词例句都放一个，选的是同一个（见 `subcorpusScope.ts`）。
 * 子语料在「分析」页的筛选条里建。
 */

import { setActiveScope, useActiveScope, useSubcorpora } from "../subcorpusScope";

interface Props {
  onError: (message: string) => void;
  /** 选了之后要做的事（比如重查当前这个词） */
  onChange?: () => void;
  className?: string;
}

export function SubcorpusPicker({ onError, onChange, className = "rate subcorpus-picker" }: Props) {
  const active = useActiveScope();
  const list = useSubcorpora(onError);
  if (list.length === 0 && !active) return null;
  return (
    <select
      className={className}
      value={active?.id ?? ""}
      title="只在这块子语料里找。子语料在「分析」页的筛选条里建"
      onChange={(e) => {
        const pick = list.find((s) => String(s.id) === e.target.value);
        setActiveScope(pick ? { id: pick.id, name: pick.name, filter: pick.filter } : null);
        onChange?.();
      }}
    >
      <option value="">整个库</option>
      {list.map((s) => (
        <option key={s.id} value={s.id}>
          子语料：{s.name}（{s.songCount} 首）
        </option>
      ))}
    </select>
  );
}
