/**
 * 子语料：按年代、流派、语言、歌词完整度切一块出来，而不是把整个曲库当成一个同质的整体。
 * 条件可以起名存下来（`subcorpora` 表），下次一选就回来。
 *
 * 演唱者和「仅日文」仍由统计页原来那两个控件管，这里只加其余的条件；
 * 存子语料时存的是**整份**条件（含演唱者）。
 */

import { useEffect, useRef, useState } from "react";

import type { StatsFilter } from "../api";
import {
  describeFilter,
  isWholeCorpus,
  LANGUAGE_LABELS,
  qualityApi,
  sameFilter,
  type FilterFacets,
  type Scope,
  type SavedSubcorpus,
} from "../corpusQuality";

interface Props {
  filter: StatsFilter;
  onChange: (filter: StatsFilter) => void;
  onError: (message: string) => void;
}

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));
const COMPLETENESS_OPTIONS: [string, number | null][] = [
  ["完整度不限", null],
  ["完整度 ≥ 50%", 0.5],
  ["完整度 ≥ 80%", 0.8],
  ["完整度 ≥ 95%", 0.95],
];

export function SubcorpusFilters({ filter, onChange, onError }: Props) {
  const [facets, setFacets] = useState<FilterFacets | null>(null);
  const [saved, setSaved] = useState<SavedSubcorpus[]>([]);
  const [scope, setScope] = useState<Scope | null>(null);
  const [naming, setNaming] = useState<string | null>(null);
  const previewRequest = useRef(0);

  useEffect(() => {
    qualityApi.facets().then(setFacets).catch((e) => onError(message(e)));
    qualityApi.subcorpora().then(setSaved).catch((e) => onError(message(e)));
  }, [onError]);

  useEffect(() => {
    const id = ++previewRequest.current;
    qualityApi
      .preview(filter)
      .then((s) => id === previewRequest.current && setScope(s))
      .catch((e) => id === previewRequest.current && onError(message(e)));
  }, [filter, onError]);

  const set = (patch: Partial<StatsFilter>) => onChange({ ...filter, ...patch });
  const year = (value: string) => (value.trim() === "" ? null : Number.parseInt(value, 10) || null);
  const active = saved.find((s) => sameFilter(s.filter, filter));

  const saveAs = async (name: string) => {
    try {
      const s = await qualityApi.saveSubcorpus(name, filter);
      setSaved(await qualityApi.subcorpora());
      setNaming(null);
      onChange(s.filter);
    } catch (err) {
      onError(message(err));
    }
  };

  return (
    <div className="subcorpus">
      <div className="subcorpus-row">
        <select
          className="rate"
          value={active?.id ?? ""}
          onChange={(e) => {
            const pick = saved.find((s) => String(s.id) === e.target.value);
            onChange(pick ? pick.filter : { performerIds: [], jpOnly: filter.jpOnly });
          }}
          title="存过的子语料"
        >
          <option value="">{active || isWholeCorpus(filter) ? "整个库" : "（未保存的条件）"}</option>
          {saved.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}（{s.songCount} 首）
            </option>
          ))}
        </select>

        <label className="year-range" title="发行年份（含两端）。设了年份时，年份不明的歌不算进来">
          <input
            type="number"
            placeholder={facets?.yearMin ? String(facets.yearMin) : "起"}
            value={filter.yearFrom ?? ""}
            onChange={(e) => set({ yearFrom: year(e.target.value) })}
          />
          <span>–</span>
          <input
            type="number"
            placeholder={facets?.yearMax ? String(facets.yearMax) : "止"}
            value={filter.yearTo ?? ""}
            onChange={(e) => set({ yearTo: year(e.target.value) })}
          />
          <span className="muted small">年</span>
        </label>

        <select
          className="rate"
          value={filter.genres?.[0] ?? ""}
          onChange={(e) => set({ genres: e.target.value ? [e.target.value] : [] })}
        >
          <option value="">全部流派</option>
          {facets?.genres.map(([g, n]) => (
            <option key={g} value={g}>
              {g}（{n}）
            </option>
          ))}
        </select>

        <select
          className="rate"
          value={filter.languages?.[0] ?? ""}
          onChange={(e) => set({ languages: e.target.value ? [e.target.value] : [] })}
          title="按歌词字符推断的语言，可以在歌的「来源与校验」里手动改"
        >
          <option value="">全部语言</option>
          {facets?.languages.map(([code, n]) => (
            <option key={code} value={code}>
              {LANGUAGE_LABELS[code] ?? code}（{n}）
            </option>
          ))}
        </select>

        <select
          className="rate"
          value={filter.minCompleteness ?? ""}
          onChange={(e) => set({ minCompleteness: e.target.value === "" ? null : Number(e.target.value) })}
          title="按歌词校验估计的完整度排除可能漏行、截断、乱码的歌"
        >
          {COMPLETENESS_OPTIONS.map(([label, value]) => (
            <option key={label} value={value ?? ""}>
              {label}
            </option>
          ))}
        </select>

        <label className="check" title="同一首歌里文本相同的行只算第一次：看「用了哪些词」时开，看「听到多少次」时关">
          <input type="checkbox" checked={!!filter.distinctLines} onChange={(e) => set({ distinctLines: e.target.checked })} />
          副歌只算一遍
        </label>
      </div>

      <div className="subcorpus-row muted small">
        <span>
          范围：{describeFilter(filter)}
          {scope && ` · ${scope.songCount} 首 / ${scope.lineCount} 行`}
        </span>
        {naming === null ? (
          <>
            <button className="link-btn" onClick={() => setNaming(active?.name ?? "")}>
              存为子语料…
            </button>
            {active && (
              <button
                className="link-btn danger"
                onClick={async () => {
                  try {
                    await qualityApi.deleteSubcorpus(active.id);
                    setSaved(await qualityApi.subcorpora());
                  } catch (err) {
                    onError(message(err));
                  }
                }}
              >
                删除「{active.name}」
              </button>
            )}
          </>
        ) : (
          <form
            className="inline-form"
            onSubmit={(e) => {
              e.preventDefault();
              if (naming.trim()) void saveAs(naming);
            }}
          >
            <input type="text" autoFocus value={naming} placeholder="名字，比如「2010 年代」" onChange={(e) => setNaming(e.target.value)} />
            <button className="primary" type="submit" disabled={!naming.trim()}>
              保存
            </button>
            <button className="ghost" type="button" onClick={() => setNaming(null)}>
              取消
            </button>
          </form>
        )}
      </div>
    </div>
  );
}
