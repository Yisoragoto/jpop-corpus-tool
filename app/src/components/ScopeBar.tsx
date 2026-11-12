/**
 * 分析页的范围条：分析页每一页（概览、词表、词簇、丰富度、报告、记录）都按这里圈出的歌算。
 *
 * 一行放最常改的：存过的子语料、演唱者、仅日文、副歌只算一遍，右边是这批歌有多大。
 * 年份、流派、语言、歌词完整度收在「更多条件」里，展开时就在这一行下面，不弹窗、不浮在内容上
 * （以前的筛选条是吸顶的半透明条，往下滚会和表头叠在一起）。
 * 条件可以起名存下来（`subcorpora` 表），下次一选就回来；存的是**整份**条件（含演唱者）。
 */

import { useEffect, useRef, useState, type KeyboardEvent } from "react";

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
import { PerformerPicker } from "./PerformerPicker";
import { tr } from "../i18n";
import { pyGrouped } from "../pyFormat";

interface Props {
  filter: StatsFilter;
  onChange: (filter: StatsFilter) => void;
  onError: (message: string) => void;
}

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));
const COMPLETENESS_OPTIONS: [string, number | null][] = [
  [tr("完整度不限"), null],
  [tr("完整度 ≥ 50%"), 0.5],
  [tr("完整度 ≥ 80%"), 0.8],
  [tr("完整度 ≥ 95%"), 0.95],
];

/** 「更多条件」里设了几项，按钮上标出来，收起时也看得出范围被切过 */
function extraConditions(f: StatsFilter): number {
  return (
    (f.yearFrom != null || f.yearTo != null ? 1 : 0) +
    (f.genres?.length ? 1 : 0) +
    (f.languages?.length ? 1 : 0) +
    (f.minCompleteness != null ? 1 : 0)
  );
}

export function ScopeBar({ filter, onChange, onError }: Props) {
  const [facets, setFacets] = useState<FilterFacets | null>(null);
  const [saved, setSaved] = useState<SavedSubcorpus[]>([]);
  const [scope, setScope] = useState<Scope | null>(null);
  const [naming, setNaming] = useState<string | null>(null);
  const [more, setMore] = useState(false);
  const [pending, setPending] = useState(false);
  // 年份先记在本地，失焦或回车才提交：逐键提交会让 2、20、201 各算一遍
  const [yearFromDraft, setYearFromDraft] = useState(filter.yearFrom != null ? String(filter.yearFrom) : "");
  const [yearToDraft, setYearToDraft] = useState(filter.yearTo != null ? String(filter.yearTo) : "");
  const previewRequest = useRef(0);

  // 范围从外面变了（选子语料、整个库）时草稿跟着走
  useEffect(() => {
    setYearFromDraft(filter.yearFrom != null ? String(filter.yearFrom) : "");
  }, [filter.yearFrom]);
  useEffect(() => {
    setYearToDraft(filter.yearTo != null ? String(filter.yearTo) : "");
  }, [filter.yearTo]);

  useEffect(() => {
    qualityApi.facets().then(setFacets).catch((e) => onError(message(e)));
    qualityApi.subcorpora().then(setSaved).catch((e) => onError(message(e)));
  }, [onError]);

  useEffect(() => {
    const id = ++previewRequest.current;
    setPending(true);
    qualityApi
      .preview(filter)
      .then((s) => {
        if (id !== previewRequest.current) return;
        setScope(s);
        setPending(false);
      })
      .catch((e) => {
        if (id !== previewRequest.current) return;
        setPending(false);
        onError(message(e));
      });
  }, [filter, onError]);

  const set = (patch: Partial<StatsFilter>) => onChange({ ...filter, ...patch });
  const year = (value: string) => (value.trim() === "" ? null : Number.parseInt(value, 10) || null);
  const active = saved.find((s) => sameFilter(s.filter, filter));
  const extra = extraConditions(filter);
  const commitYear = (key: "yearFrom" | "yearTo", draft: string) => {
    const value = year(draft);
    if (value !== (filter[key] ?? null)) set({ [key]: value });
    // 值没变（比如输了 0）就把草稿还原成当前值
    else if (key === "yearFrom") setYearFromDraft(value != null ? String(value) : "");
    else setYearToDraft(value != null ? String(value) : "");
  };
  const onYearKey = (key: "yearFrom" | "yearTo", draft: string) => (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") commitYear(key, draft);
  };

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
    <div className="scope-bar">
      <div className="scope-row">
        <span className="scope-label">{tr("范围")}</span>
        <select
          className="rate"
          value={active ? String(active.id) : isWholeCorpus(filter) ? "" : "__unsaved"}
          onChange={(e) => {
            const pick = saved.find((s) => String(s.id) === e.target.value);
            onChange(pick ? pick.filter : { performerIds: [], jpOnly: filter.jpOnly, ...(filter.distinctLines ? { distinctLines: true } : {}) });
          }}
          title={tr("存过的子语料")}
        >
          <option value="">{tr("整个库")}</option>
          {!active && !isWholeCorpus(filter) && (
            <option value="__unsaved" disabled hidden>{tr("（未保存的条件）")}</option>
          )}
          {saved.map((s) => (
            <option key={s.id} value={s.id}>{tr("{name}（{songCount} 首）", { name: s.name, songCount: pyGrouped(s.songCount) })}</option>
          ))}
        </select>
        <PerformerPicker
          selected={filter.performerIds}
          onChange={(performerIds) => set({ performerIds })}
          onError={onError}
        />
        <button
          type="button"
          className={`picker-button scope-more${more ? " open" : ""}${extra > 0 ? " on" : ""}`}
          aria-expanded={more}
          onClick={() => setMore((v) => !v)}
          title={tr("年份、流派、语言、歌词完整度")}
        >
          {tr("更多条件")}
          {extra > 0 && <span className="scope-badge">{extra}</span>}
        </button>
        <span className="scope-sep" aria-hidden="true" />
        <label className="check" title={tr("只统计含假名或汉字的词元（去掉英文、数字）")}>
          <input type="checkbox" checked={filter.jpOnly} onChange={(e) => set({ jpOnly: e.target.checked })} />
          {tr("仅日文")}
        </label>
        <label className="check" title={tr("同一首歌里文本相同的行只算第一次：看「用了哪些词」时开，看「听到多少次」时关")}>
          <input type="checkbox" checked={!!filter.distinctLines} onChange={(e) => set({ distinctLines: e.target.checked })} />
          {tr("副歌只算一遍")}
        </label>
        <span className={`scope-size${pending ? " stale" : ""}`} aria-busy={pending} title={describeFilter(filter)}>
          {scope ? tr("{songCount} 首 · {lineCount} 行", { songCount: pyGrouped(scope.songCount), lineCount: pyGrouped(scope.lineCount) }) : "…"}
        </span>
      </div>

      {more && (
        <div className="scope-row scope-extra">
          <span
            className="year-range"
            role="group"
            aria-label={tr("发行年份")}
            title={tr("发行年份（含两端）。设了年份时，年份不明的歌不算进来")}
          >
            <input
              type="text"
              inputMode="numeric"
              maxLength={4}
              aria-label={tr("起始年份")}
              placeholder={facets?.yearMin ? String(facets.yearMin) : tr("起")}
              value={yearFromDraft}
              onChange={(e) => setYearFromDraft(e.target.value.replace(/\D/g, ""))}
              onBlur={() => commitYear("yearFrom", yearFromDraft)}
              onKeyDown={onYearKey("yearFrom", yearFromDraft)}
            />
            <span aria-hidden="true">–</span>
            <input
              type="text"
              inputMode="numeric"
              maxLength={4}
              aria-label={tr("结束年份")}
              placeholder={facets?.yearMax ? String(facets.yearMax) : tr("止")}
              value={yearToDraft}
              onChange={(e) => setYearToDraft(e.target.value.replace(/\D/g, ""))}
              onBlur={() => commitYear("yearTo", yearToDraft)}
              onKeyDown={onYearKey("yearTo", yearToDraft)}
            />
            <span className="muted small">{tr("年")}</span>
          </span>
          <select
            className="rate"
            aria-label={tr("流派")}
            value={filter.genres?.[0] ?? ""}
            onChange={(e) => set({ genres: e.target.value ? [e.target.value] : [] })}
          >
            <option value="">{tr("全部流派")}</option>
            {facets?.genres.map(([g, n]) => (
              <option key={g} value={g}>
                {g}{tr("（")}{pyGrouped(n)}{tr("）")}
              </option>
            ))}
          </select>
          <select
            className="rate"
            aria-label={tr("语言")}
            value={filter.languages?.[0] ?? ""}
            onChange={(e) => set({ languages: e.target.value ? [e.target.value] : [] })}
            title={tr("按歌词字符推断的语言，可以在歌的「来源与校验」里手动改")}
          >
            <option value="">{tr("全部语言")}</option>
            {facets?.languages.map(([code, n]) => (
              <option key={code} value={code}>
                {LANGUAGE_LABELS[code] ?? code}{tr("（")}{pyGrouped(n)}{tr("）")}
              </option>
            ))}
          </select>
          <select
            className="rate"
            aria-label={tr("歌词完整度")}
            value={filter.minCompleteness ?? ""}
            onChange={(e) => set({ minCompleteness: e.target.value === "" ? null : Number(e.target.value) })}
            title={tr("按歌词校验估计的完整度排除可能漏行、截断、乱码的歌")}
          >
            {COMPLETENESS_OPTIONS.map(([label, value]) => (
              <option key={label} value={value ?? ""}>
                {label}
              </option>
            ))}
          </select>
          <span className="scope-save">
            {naming === null ? (
              <>
                <button type="button" className="link-btn" onClick={() => setNaming(active?.name ?? "")}>{tr("存为子语料…")}</button>
                {active && (
                  <button
                    type="button"
                    className="link-btn danger"
                    onClick={async () => {
                      try {
                        await qualityApi.deleteSubcorpus(active.id);
                        setSaved(await qualityApi.subcorpora());
                      } catch (err) {
                        onError(message(err));
                      }
                    }}
                  >{tr("删除「{name}」", { name: active.name })}</button>
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
                <input type="text" autoFocus value={naming} placeholder={tr("名字，比如「2010 年代」")} onChange={(e) => setNaming(e.target.value)} />
                <button className="primary" type="submit" disabled={!naming.trim()}>{tr("保存")}</button>
                <button className="ghost" type="button" onClick={() => setNaming(null)}>{tr("取消")}</button>
              </form>
            )}
          </span>
        </div>
      )}
      {!isWholeCorpus(filter) && <p className="scope-desc muted small">{tr("范围：{filter}", { filter: describeFilter(filter) })}</p>}
    </div>
  );
}
