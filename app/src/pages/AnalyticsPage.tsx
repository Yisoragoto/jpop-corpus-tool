/**
 * 分析页：顶上一条范围条，下面按工具分页（WinUI 的 SelectorBar）。
 *
 * 以前是一整页竖着堆十张卡，筛选条吸顶浮在内容上，往下滚就和表头叠在一起。现在：
 *
 * - **范围条**（`ScopeBar`）是页面的一部分，不浮动；每一页都按它圈出的歌算；
 * - **分页**：概览 · 词表 · 词簇 · 活用·写法 · 丰富度 · 对比 · 报告 · 记录。一页一个工具，表格占满高度、
 *   跟着页面滚，表头吸在顶上，不再套一个 560px 的小滚动框；
 * - 选了哪一页记住（`jp.analytics.tab`）；页第一次打开才挂载，之后切走也保留状态（对比填好的两边不丢）。
 *
 * 概览只用其余几页本来就要取的数据（报告、全部词频、整体丰富度），每一块都能点进对应的页或检索。
 * 图表仍然全部用内联 SVG / CSS 画，不引图表库（要求书第十五条第 3 点）。
 * 数字的写法照 Python（千分位、正好一半取偶），和导出的 TXT 一致。
 */

import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { save } from "@tauri-apps/plugin-dialog";

import {
  api,
  formatLongDuration,
  POS_LABELS,
  type Dispersion,
  type DurationBackfillReport,
  type FrequencyItem,
  type FrequencyRow,
  type LexicalRow,
  type MatchField,
  type Overview,
  type StatsFilter,
  type StatsReport,
  type YearStats,
} from "../api";
import { AnalysisRuns } from "../components/AnalysisRuns";
import { KeynessCard, RepeatEffectCard } from "../components/CorpusCompare";
import { LexicalCard } from "../components/LexicalCard";
import { InflectionCard, OrthographyCard } from "../components/MorphologyCards";
import { NgramCard } from "../components/NgramCard";
import { QualityCard } from "../components/QualityCard";
import { saveCsv } from "../components/saveCsv";
import { ScopeBar } from "../components/ScopeBar";
import { Stat } from "../components/Stat";
import { describeFilter, toSubcorpus } from "../corpusQuality";
import { pyFixed, pyGrouped, tableCsv } from "../pyFormat";
import type { LibraryActions } from "../useLibrary";
import { tr } from "../i18n";

/** Python `pos_options`：统计页可选的词性 */
const POS_FILTERS = ["", "NOUN", "VERB", "ADJ", "ADV", "AUX", "PRON", "PROPN", "INTJ", "ADP", "CCONJ"] as const;

/** Python `_POS_JA`：报告里的「和名」一列，和导出的 TXT 一样用日文 */
const POS_JA: Record<string, string> = {
  NOUN: "名詞", VERB: "動詞", ADJ: "形容詞", ADV: "副詞", AUX: "助動詞", PRON: "代名詞",
  PROPN: "固有名詞", INTJ: "感動詞", ADP: "助詞", CCONJ: "接続詞", NUM: "数詞", PART: "接辞",
  SCONJ: "従属接", DET: "限定詞",
};

/** 统计表行数。和 Python `StatsWorker` 的 LIMIT 300 一样 */
const TABLE_LIMIT = 300;

/** 概览里「高频实词」算哪些词性 */
const CONTENT_POS = new Set(["NOUN", "VERB", "ADJ", "ADV", "PROPN"]);

/** 词频表可以按哪一列排。DP 越小越均匀，所以按 DP 排默认从小到大 */
type FreqSort = "freq" | "songs" | "dp" | "arf";

const SORT_UP: Record<FreqSort, boolean> = { freq: false, songs: false, dp: true, arf: false };

const dispersionKey = (lemma: string, pos: string) => `${pos}\u{1f}${lemma}`;

type Tab = "overview" | "words" | "ngrams" | "morph" | "richness" | "compare" | "report" | "runs";

const TABS: { key: Tab; label: string }[] = [
  { key: "overview", label: tr("概览") },
  { key: "words", label: tr("词表") },
  { key: "ngrams", label: tr("词簇") },
  { key: "morph", label: tr("活用·写法") },
  { key: "richness", label: tr("丰富度") },
  { key: "compare", label: tr("对比") },
  { key: "report", label: tr("报告") },
  { key: "runs", label: tr("记录") },
];

const TAB_KEY = "jp.analytics.tab";

function loadTab(): Tab {
  try {
    const saved = localStorage.getItem(TAB_KEY);
    if (saved && TABS.some((t) => t.key === saved)) return saved as Tab;
  } catch {
    // 存储不可用时就从概览开始
  }
  return "overview";
}

const SCRIPTS = [
  { key: "kanji", label: tr("汉字") },
  { key: "hiragana", label: tr("平假名") },
  { key: "katakana", label: tr("片假名") },
  { key: "latin", label: tr("拉丁字母") },
  { key: "other", label: tr("其他") },
] as const;

interface Props {
  overview: Overview | null;
  onOverviewChanged: () => void;
  actions: LibraryActions;
  onNavigate: () => void;
  /** 跳到检索页，按词元查这个词 */
  onSearchWord: (lemma: string) => void;
  /** 跳到检索页查一个多词表达（`A+B+C`），按 field 匹配 */
  onSearchPhrase: (phrase: string, field: MatchField) => void;
  /** 跳到检索页，按词元查一个词的某种活用形 */
  onSearchInflected: (lemma: string, cform: string) => void;
  /** 词云在资料库里：点了跳过去 */
  onOpenCloud: () => void;
  onError: (message: string) => void;
}

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

export function AnalyticsPage({
  overview,
  onOverviewChanged,
  actions,
  onNavigate,
  onSearchWord,
  onSearchPhrase,
  onSearchInflected,
  onOpenCloud,
  onError,
}: Props) {
  const [tab, setTabState] = useState<Tab>(loadTab);
  /** 打开过的页：第一次打开才挂载，之后切走也保留状态 */
  const [visited, setVisited] = useState<Set<Tab>>(() => new Set([loadTab()]));
  const setTab = (next: Tab) => {
    setTabState(next);
    setVisited((v) => (v.has(next) ? v : new Set(v).add(next)));
    try {
      localStorage.setItem(TAB_KEY, next);
    } catch {
      // 记不住就算了
    }
  };

  const [timeline, setTimeline] = useState<YearStats[]>([]);
  const [filter, setFilter] = useState<StatsFilter>({ performerIds: [], jpOnly: false });
  const [pos, setPos] = useState("");
  const [rows, setRows] = useState<FrequencyRow[] | null>(null);
  /** 范围里全部词（analysis.rs）：离散度拼到统计表上，概览的高频实词、副歌撑大的词也从这里取 */
  const [allWords, setAllWords] = useState<FrequencyItem[] | null>(null);
  const [whole, setWhole] = useState<LexicalRow | null>(null);
  const [freqSort, setFreqSort] = useState<{ col: FreqSort; up: boolean }>({ col: "freq", up: false });
  const [report, setReport] = useState<StatsReport | null>(null);
  const [loadingRows, setLoadingRows] = useState(false);
  const [loadingReport, setLoadingReport] = useState(false);
  const [exportNote, setExportNote] = useState("");
  // 筛选连着改几次时，只认最后一次的结果
  const rowsRequest = useRef(0);
  const reportRequest = useRef(0);

  useEffect(() => {
    api.timeline().then(setTimeline).catch((e) => onError(message(e)));
  }, [onError]);

  useEffect(() => {
    const id = ++rowsRequest.current;
    setLoadingRows(true);
    api
      .statsFrequency(filter, pos, TABLE_LIMIT)
      .then((result) => id === rowsRequest.current && setRows(result))
      .catch((e) => id === rowsRequest.current && onError(message(e)))
      .finally(() => id === rowsRequest.current && setLoadingRows(false));
  }, [filter, pos, onError]);

  useEffect(() => {
    let cancelled = false;
    const scope = { subcorpus: toSubcorpus(filter), unit: filter.distinctLines ? "distinctLines" : "tokens", jpOnly: filter.jpOnly } as const;
    api
      .corpusFrequency({ scope, pos: null, limit: 1_000_000 })
      .then((r) => !cancelled && setAllWords(r.items))
      .catch((e) => !cancelled && onError(message(e)));
    api
      .lexicalProfile({ scope, group: "whole", window: 50, minTokens: 1 })
      .then((r) => !cancelled && setWhole(r.rows[0] ?? null))
      .catch((e) => !cancelled && onError(message(e)));
    return () => {
      cancelled = true;
    };
  }, [filter, onError]);

  useEffect(() => {
    const id = ++reportRequest.current;
    setLoadingReport(true);
    setExportNote("");
    api
      .statsReport(filter)
      .then((result) => id === reportRequest.current && setReport(result))
      .catch((e) => id === reportRequest.current && onError(message(e)))
      .finally(() => id === reportRequest.current && setLoadingReport(false));
  }, [filter, onError]);

  const dispersion = useMemo(
    () => new Map<string, Dispersion>((allWords ?? []).map((i) => [dispersionKey(i.lemma, i.pos), i.dispersion])),
    [allWords],
  );

  const sortedRows = useMemo(() => {
    if (!rows) return null;
    const value = (w: FrequencyRow): number => {
      const d = dispersion.get(dispersionKey(w.lemma, w.pos));
      switch (freqSort.col) {
        case "songs":
          return w.songCount;
        case "dp":
          return d?.dp ?? Number.POSITIVE_INFINITY;
        case "arf":
          return d?.arf ?? 0;
        default:
          return w.freq;
      }
    };
    if (freqSort.col === "freq" && !freqSort.up) return rows;
    const sign = freqSort.up ? 1 : -1;
    return [...rows].sort((a, b) => sign * (value(a) - value(b)) || b.freq - a.freq);
  }, [rows, dispersion, freqSort]);

  const sortBy = (col: FreqSort) =>
    setFreqSort((s) => (s.col === col ? { col, up: !s.up } : { col, up: SORT_UP[col] }));
  const sortMark = (col: FreqSort) => (freqSort.col === col ? (freqSort.up ? " ↑" : " ↓") : "");

  const exportFrequency = async () => {
    if (!sortedRows) return;
    const csv = tableCsv(
      ["rank", "lemma", "pos", "jlpt", "freq", "songs", "DP", "DPnorm", "JuillandD", "ARF", "surfaces"],
      sortedRows.map((w, i) => {
        const d = dispersion.get(dispersionKey(w.lemma, w.pos));
        return [i + 1, w.lemma, w.pos, w.jlpt, w.freq, w.songCount, d?.dp ?? null, d?.dpNorm ?? null, d?.juillandD ?? null, d?.arf ?? null, w.surfaces.join(" ")];
      }),
    );
    const path = await saveCsv(csv, "word_frequency.csv", onError);
    if (path) setExportNote(tr("已保存：{path}", { path }));
  };

  const openWord = async (lemma: string) => {
    await actions.openWord(lemma);
    onNavigate();
  };

  const exportReport = async () => {
    if (!report) return;
    try {
      const path = await save({
        title: tr("保存统计报告"),
        defaultPath: "corpus_report.txt",
        filters: [{ name: tr("文本文件"), extensions: ["txt"] }],
      });
      if (!path) return;
      await api.exportText(path, report.text);
      setExportNote(tr("已保存：{path}", { path }));
    } catch (err) {
      onError(message(err));
    }
  };

  const filterDesc = describeFilter(filter, report?.performers ?? []);
  const show = (t: Tab) => (tab === t ? undefined : true);

  return (
    <div className="page analytics">
      <ScopeBar filter={filter} onChange={setFilter} onError={onError} />

      <nav className="selector-bar" role="tablist" aria-label={tr("分析")}>
        {TABS.map((t) => (
          <button
            key={t.key}
            role="tab"
            aria-selected={tab === t.key}
            className={tab === t.key ? "on" : ""}
            onClick={() => setTab(t.key)}
          >
            {t.label}
          </button>
        ))}
      </nav>

      {visited.has("overview") && (
        <div className="analytics-tab" hidden={show("overview")}>
          <OverviewTab
            report={report}
            allWords={allWords}
            whole={whole}
            timeline={timeline}
            overview={overview}
            onOverviewChanged={onOverviewChanged}
            onGo={setTab}
            onSearchWord={onSearchWord}
            onOpenSong={(songId) => {
              void actions.openTrackById(songId).then((ok) => ok && onNavigate());
            }}
            onOpenCloud={onOpenCloud}
            onError={onError}
          />
        </div>
      )}

      {visited.has("words") && (
        <div className="analytics-tab" hidden={show("words")}>
          <section className="card">
            <div className="card-head">
              <h2>{tr("词频统计")}</h2>
              <div className="card-tools">
                <span className="muted small">
                  {loadingRows ? tr("读取中…") : rows ? tr("{length} 个词", { length: rows.length }) : ""}
                </span>
                <select className="rate" value={pos} onChange={(e) => setPos(e.target.value)}>
                  {POS_FILTERS.map((p) => (
                    <option key={p} value={p}>
                      {p === "" ? tr("全部词性") : `${POS_LABELS[p] ?? p} ${p}`}
                    </option>
                  ))}
                </select>
                <button className="ghost" disabled={!rows?.length} onClick={() => void exportFrequency()}>{tr("导出 CSV")}</button>
              </div>
            </div>
            <p className="muted small">{tr("词元 × 词性按出现次数排，前 {TABLE_LIMIT} 个，不含标点和符号。JLPT 等级来自本地缓存。 点词元看例句，点「检索」到检索页看 KWIC。", { TABLE_LIMIT })}{" "}{tr("点表头换排序。DP 越接近 0 越均匀地散在各首歌里，越接近 1 越集中在少数几首；ARF 是把扎堆出现折算掉以后「相当于均匀出现了几次」，比出现次数小得多的词多半是副歌堆出来的。")}
              {exportNote && <span className="export-note"> {exportNote}</span>}
            </p>
            {sortedRows && sortedRows.length > 0 && (
              <div className={`table-wrap${loadingRows ? " stale" : ""}`}>
                <table className="data">
                  <thead>
                    <tr>
                      <th className="num">#</th>
                      <th>{tr("词元")}</th>
                      <th>{tr("词性")}</th>
                      <th className="mid">JLPT</th>
                      <th className="num sortable" onClick={() => sortBy("freq")}>{tr("出现次数")}{sortMark("freq")}</th>
                      <th className="num sortable" onClick={() => sortBy("songs")}>{tr("出现曲数")}{sortMark("songs")}</th>
                      <th className="num sortable" onClick={() => sortBy("dp")} title={tr("Gries 的 DP：0 = 按歌的大小均匀分布，1 = 全集中在一首里")}>DP{sortMark("dp")}</th>
                      <th className="num sortable" onClick={() => sortBy("arf")} title={tr("平均缩减频次：扎堆出现的次数折算掉之后相当于均匀出现几次")}>ARF{sortMark("arf")}</th>
                      <th>{tr("表层形例")}</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {sortedRows.map((w, i) => {
                      const d = dispersion.get(dispersionKey(w.lemma, w.pos));
                      return (
                        <tr key={`${w.pos}:${w.lemma}`}>
                          <td className="num muted">{i + 1}</td>
                          <td>
                            <button className="word-link" onClick={() => void openWord(w.lemma)}>
                              {w.lemma}
                            </button>
                          </td>
                          <td className="muted" title={w.pos}>
                            {POS_LABELS[w.pos] ?? w.pos}
                          </td>
                          <td className="mid">{w.jlpt || <span className="muted">—</span>}</td>
                          <td className="num">{pyGrouped(w.freq)}</td>
                          <td className="num">{pyGrouped(w.songCount)}</td>
                          <td
                            className="num"
                            title={d ? tr("DP {dp} · DPnorm {dpNorm} · Juilland D {d}", { dp: d.dp.toFixed(3), dpNorm: d.dpNorm.toFixed(3), d: d.juillandD === null ? "—" : d.juillandD.toFixed(3) }) : undefined}
                          >
                            {d ? <span className="dp-cell" style={{ ["--dp" as string]: `${d.dp * 100}%` }}>{d.dp.toFixed(2)}</span> : <span className="muted">—</span>}
                          </td>
                          <td className="num">{d ? d.arf.toFixed(1) : <span className="muted">—</span>}</td>
                          <td title={w.surfaces.join("、")}>
                            <span className="surfaces">{w.surfaces.slice(0, 5).join("、")}</span>
                          </td>
                          <td className="row-actions">
                            <button className="link-btn" onClick={() => onSearchWord(w.lemma)}>{tr("检索")}</button>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}
            {rows && rows.length === 0 && !loadingRows && <p className="muted">{tr("没有数据")}</p>}
          </section>
        </div>
      )}

      {visited.has("ngrams") && (
        <div className="analytics-tab" hidden={show("ngrams")}>
          <NgramCard filter={filter} onSearchPhrase={onSearchPhrase} onError={onError} />
        </div>
      )}

      {visited.has("morph") && (
        <div className="analytics-tab" hidden={show("morph")}>
          <InflectionCard filter={filter} onSearchInflected={onSearchInflected} onError={onError} />
          <OrthographyCard filter={filter} onSearch={onSearchPhrase} onError={onError} />
        </div>
      )}

      {visited.has("richness") && (
        <div className="analytics-tab" hidden={show("richness")}>
          <LexicalCard filter={filter} onError={onError} />
        </div>
      )}

      {visited.has("compare") && (
        <div className="analytics-tab" hidden={show("compare")}>
          <KeynessCard jpOnly={filter.jpOnly} onSearchWord={onSearchWord} onError={onError} />
          <RepeatEffectCard subcorpus={toSubcorpus(filter)} jpOnly={filter.jpOnly} onSearchWord={onSearchWord} onError={onError} />
        </div>
      )}

      {visited.has("report") && (
        <div className="analytics-tab" hidden={show("report")}>
          <section className="card report">
            <div className="card-head">
              <h2>{tr("语料报告")}</h2>
              <div className="card-tools">
                <span className="muted small">
                  {loadingReport
                    ? tr("计算中…")
                    : report
                      ? tr("{songCount} 曲 / {tokenCount} tokens", { songCount: pyGrouped(report.report.songCount), tokenCount: pyGrouped(report.report.tokenCount) })
                      : ""}
                </span>
                <button className="ghost" disabled={!report || loadingReport} onClick={() => void exportReport()}>{tr("导出 TXT")}</button>
              </div>
            </div>
            <p className="muted small">{tr("TTR、STTR、Hapax、词性分布、高频词覆盖率。导出的 TXT 和 PyQt 版格式相同。")}{exportNote && <span className="export-note"> {exportNote}</span>}
            </p>
            {report && <ReportBody data={report} filterDesc={filterDesc} stale={loadingReport} onWord={openWord} />}
          </section>
        </div>
      )}

      {visited.has("runs") && (
        <div className="analytics-tab" hidden={show("runs")}>
          <AnalysisRuns filter={filter} pos={pos} frequencyLimit={TABLE_LIMIT} onError={onError} />
        </div>
      )}
    </div>
  );
}

// ────────────────────────────── 概览 ──────────────────────────────

function OverviewTab({
  report,
  allWords,
  whole,
  timeline,
  overview,
  onOverviewChanged,
  onGo,
  onSearchWord,
  onOpenSong,
  onOpenCloud,
  onError,
}: {
  report: StatsReport | null;
  allWords: FrequencyItem[] | null;
  whole: LexicalRow | null;
  timeline: YearStats[];
  overview: Overview | null;
  onOverviewChanged: () => void;
  onGo: (tab: Tab) => void;
  onSearchWord: (lemma: string) => void;
  onOpenSong: (songId: string) => void;
  onOpenCloud: () => void;
  onError: (message: string) => void;
}) {
  const r = report?.report;
  const repeatShare = useMemo(() => {
    if (!allWords?.length) return null;
    const raw = allWords.reduce((s, w) => s + w.rawFreq, 0);
    const distinct = allWords.reduce((s, w) => s + w.distinctFreq, 0);
    return raw > 0 ? 1 - distinct / raw : null;
  }, [allWords]);

  const topContent = useMemo(
    () => (allWords ?? []).filter((w) => CONTENT_POS.has(w.pos)).slice(0, 10),
    [allWords],
  );
  const inflated = useMemo(
    () =>
      (allWords ?? [])
        // 只在一首歌里出现的词（英文副歌、口癖）重复占比天然接近 100%，说明不了什么，至少要两首
        .filter((w) => w.rawFreq >= 10 && w.songs >= 2 && w.pos !== "AUX" && w.pos !== "ADP")
        .sort((a, b) => b.repeatShare - a.repeatShare || b.rawFreq - a.rawFreq)
        .slice(0, 10),
    [allWords],
  );
  const maxTop = topContent[0]?.freq ?? 1;
  const maxTracks = Math.max(1, ...timeline.map((y) => y.trackCount));

  return (
    <>
      <div className="kpi-row">
        <Kpi label={tr("曲目")} value={r ? pyGrouped(r.songCount) : "…"} />
        <Kpi label={tr("歌词行")} value={r ? pyGrouped(r.utteranceCount) : "…"} />
        <Kpi label={tr("词次")} value={r ? pyGrouped(r.tokenCount) : "…"} hint={tr("不含标点")} />
        <Kpi label={tr("词种")} value={r ? pyGrouped(r.typeCount) : "…"} hint={tr("不同的词元")} />
        <Kpi
          label={tr("来自重复行")}
          value={repeatShare === null ? "…" : `${(repeatShare * 100).toFixed(1)}%`}
          hint={tr("同一首歌里重复唱的行占的词次")}
        />
        <Kpi
          label="MATTR"
          value={whole?.mattr == null ? "—" : whole.mattr.toFixed(3)}
          hint={tr("不受长度影响的词汇丰富度")}
          onClick={() => onGo("richness")}
        />
      </div>

      <div className="ov-grid">
        <OvBlock title={tr("文字构成")} action={tr("按演唱者看 →")} onAction={() => onGo("richness")}>
          {whole ? (
            <>
              <span className="script-bar big" title={SCRIPTS.map((s) => `${s.label} ${(whole.scripts[s.key] * 100).toFixed(1)}%`).join(" · ")}>
                {SCRIPTS.map((s) => (
                  <i key={s.key} className={`s-${s.key}`} style={{ width: `${whole.scripts[s.key] * 100}%` }} />
                ))}
              </span>
              <ul className="script-figures">
                {SCRIPTS.map((s) => (
                  <li key={s.key}>
                    <i className={`script-swatch s-${s.key}`} />
                    <span>{s.label}</span>
                    <b>{(whole.scripts[s.key] * 100).toFixed(1)}%</b>
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <p className="muted small">{tr("计算中…")}</p>
          )}
        </OvBlock>

        <OvBlock title={tr("曲库按年份")} hint={tr("整个曲库，不受范围影响")}>
          {timeline.length === 0 ? (
            <p className="muted small">{tr("没有带年份的曲目")}</p>
          ) : (
            <div className="timeline">
              {timeline.map((y) => (
                <div className="year" key={y.year} title={tr("{trackCount} 首 · {vocabulary} 词", { trackCount: y.trackCount, vocabulary: y.vocabulary })}>
                  <div className="year-bars">
                    <span className="bar tracks" style={{ height: `${(y.trackCount / maxTracks) * 100}%` }} />
                  </div>
                  <span className="year-label">{y.year.slice(2)}</span>
                </div>
              ))}
            </div>
          )}
        </OvBlock>

        <OvBlock title={tr("高频实词")} action={tr("完整词表 →")} onAction={() => onGo("words")}>
          <ol className="ov-list">
            {topContent.map((w) => (
              <li key={`${w.pos}:${w.lemma}`}>
                <button className="ov-word" onClick={() => onSearchWord(w.lemma)} title={tr("检索「{lemma}」", { lemma: w.lemma })}>
                  {w.lemma}
                </button>
                <span className="muted small">{POS_LABELS[w.pos] ?? w.pos}</span>
                <span className="ov-bar">
                  <span style={{ width: `${(w.freq / maxTop) * 100}%` }} />
                </span>
                <span className="num">{pyGrouped(w.freq)}</span>
                <span className="num muted" title={tr("DP：越接近 0 越均匀")}>DP {w.dispersion.dp.toFixed(2)}</span>
              </li>
            ))}
          </ol>
        </OvBlock>

        <OvBlock title={tr("副歌撑大的词")} hint={tr("频次大半来自重复唱的行")} action={tr("两种口径对比 →")} onAction={() => onGo("compare")}>
          <ol className="ov-list">
            {inflated.map((w) => (
              <li key={`${w.pos}:${w.lemma}`}>
                <button className="ov-word" onClick={() => onSearchWord(w.lemma)} title={tr("检索「{lemma}」", { lemma: w.lemma })}>
                  {w.lemma}
                </button>
                <span className="muted small">{POS_LABELS[w.pos] ?? w.pos}</span>
                <span className="ov-bar warn">
                  <span style={{ width: `${w.repeatShare * 100}%` }} />
                </span>
                <span className="num">{(w.repeatShare * 100).toFixed(0)}%</span>
                <span className="num muted" title={tr("全部词次 → 重复行只算一次")}>{w.rawFreq} → {w.distinctFreq}</span>
              </li>
            ))}
          </ol>
        </OvBlock>
      </div>

      <LibraryBlock overview={overview} onOverviewChanged={onOverviewChanged} onError={onError} />

      <QualityCard onOpenSong={onOpenSong} onError={onError} />

      <button className="cloud-moved" onClick={onOpenCloud}>
        <span className="cloud-moved-art" aria-hidden="true">
          <b>夢</b>
          <i>君</i>
          <em>世界</em>
        </span>
        <span>
          <strong>{tr("词云搬到「资料库」了")}</strong>
          <small>{tr("可以选词性（默认名词）、换字体、点歌就从那一句开始放，还能全屏")}</small>
        </span>
        <span className="cloud-moved-go">{tr("打开 →")}</span>
      </button>
    </>
  );
}

function Kpi({ label, value, hint, onClick }: { label: string; value: string; hint?: string; onClick?: () => void }) {
  const body = (
    <>
      <b>{value}</b>
      <span>{label}</span>
    </>
  );
  return onClick ? (
    <button className="kpi" title={hint} onClick={onClick}>
      {body}
    </button>
  ) : (
    <div className="kpi" title={hint}>
      {body}
    </div>
  );
}

function OvBlock({
  title,
  hint,
  action,
  onAction,
  children,
}: {
  title: string;
  hint?: string;
  action?: string;
  onAction?: () => void;
  children: ReactNode;
}) {
  return (
    <section className="ov-block">
      <header>
        <h3>{title}</h3>
        {hint && <span className="muted small">{hint}</span>}
        {action && onAction && (
          <button className="link-btn" onClick={onAction}>
            {action}
          </button>
        )}
      </header>
      {children}
    </section>
  );
}

/** 整个曲库的规模（不受范围影响），以及时长没回填时的维护入口 */
function LibraryBlock({
  overview,
  onOverviewChanged,
  onError,
}: {
  overview: Overview | null;
  onOverviewChanged: () => void;
  onError: (message: string) => void;
}) {
  const [backfill, setBackfill] = useState<DurationBackfillReport | null>(null);
  const [backfilling, setBackfilling] = useState(false);
  if (!overview) return null;
  return (
    <section className="card">
      <div className="card-head">
        <h2>{tr("曲库")}</h2>
        <span className="muted small">{tr("整个曲库，不受范围影响")}</span>
      </div>
      <div className="overview-grid">
        <Stat label={tr("专辑")} value={overview.albums} align="start" />
        <Stat label={tr("人物")} value={overview.people} align="start" />
        <Stat label={tr("演唱")} value={overview.performers} align="start" />
        <Stat label={tr("作曲")} value={overview.composers} align="start" />
        <Stat label={tr("作词")} value={overview.lyricists} align="start" />
        <Stat label={tr("编曲")} value={overview.arrangers} align="start" />
        <Stat label={tr("词汇量")} value={overview.vocabulary} align="start" />
        <Stat label={tr("总时长")} value={formatLongDuration(overview.totalDurationSec)} align="start" />
      </div>

      {overview.totalDurationSec === 0 && !backfill && (
        <div className="maintenance">
          <p className="muted small">{tr("总时长为空：")}<code>songs.duration_sec</code>{" "}{tr("还没回填。 回填会逐个读音频文件的时长写回库，幂等，可随时重跑。")}</p>
          <button
            className="primary"
            disabled={backfilling}
            onClick={async () => {
              setBackfilling(true);
              try {
                const result = await api.backfillDurations();
                setBackfill(result);
                onOverviewChanged();
              } catch (err) {
                onError(message(err));
              } finally {
                setBackfilling(false);
              }
            }}
          >
            {backfilling ? tr("回填中…") : tr("回填时长")}
          </button>
        </div>
      )}

      {backfill && (
        <div className="maintenance">
          <p className="small">{tr("扫描 {scanned} 首：写入 {written}", { scanned: backfill.scanned, written: backfill.written })}{backfill.missing > 0 && tr("，文件缺失 {missing}", { missing: backfill.missing })}
            {backfill.unknown > 0 && tr("，无时长 {unknown}", { unknown: backfill.unknown })}
            {backfill.failed > 0 && tr("，失败 {failed}", { failed: backfill.failed })}
          </p>
          {backfill.samples.length > 0 && (
            <ul className="muted small samples">
              {backfill.samples.map((s) => (
                <li key={s}>{s}</li>
              ))}
            </ul>
          )}
        </div>
      )}
    </section>
  );
}

// ────────────────────────────── 报告 ──────────────────────────────

function ReportCard({ label, value, note, children }: { label: string; value: string; note?: string; children?: React.ReactNode }) {
  return (
    <div className="report-card">
      <b>{value}</b>
      <span>{label}</span>
      {note && <small>{note}</small>}
      {children}
    </div>
  );
}

function Bar({ value, max }: { value: number; max: number }) {
  return (
    <span className="freq-bar">
      <span style={{ width: `${max > 0 ? Math.min(100, (value / max) * 100) : 0}%` }} />
    </span>
  );
}

function ReportBody({
  data,
  filterDesc,
  stale,
  onWord,
}: {
  data: StatsReport;
  filterDesc: string;
  stale: boolean;
  onWord: (lemma: string) => void;
}) {
  const r = data.report;
  const maxPct = r.posDist[0]?.tokenPct ?? 1;
  const maxFreq = r.topWords[0]?.[1] ?? 1;
  return (
    <div className={`report-body${stale ? " stale" : ""}`}>
      <p className="small">{tr("筛选：{filterDesc}", { filterDesc })}</p>

      <div className="report-cards">
        <ReportCard label={tr("曲数")} value={pyGrouped(r.songCount)} />
        <ReportCard label={tr("歌词行数")} value={pyGrouped(r.utteranceCount)} />
        <ReportCard label={tr("Token 数")} value={pyGrouped(r.tokenCount)} note={tr("实词 token，去除标点")} />
        <ReportCard label={tr("Type 数")} value={pyGrouped(r.typeCount)} note={tr("唯一 lemma 数")} />
      </div>

      <h3>{tr("词汇多样性")}</h3>
      <div className="report-cards">
        <ReportCard label="TTR" value={pyFixed(r.ttr, 4)} note="Types / Tokens" />
        <ReportCard
          label="STTR"
          value={r.sttr === null ? "—" : pyFixed(r.sttr, 4)}
          note={r.sttrChunk ? tr("标准化 TTR / {sttrChunk} tokens", { sttrChunk: r.sttrChunk }) : tr("标准化 TTR（词次不足 1000）")}
        />
        <ReportCard
          label={tr("Hapax 比率")}
          value={`${pyFixed(r.hapaxRatio * 100, 1)}%`}
          note={tr("{hapaxCount} 个词只出现 1 次", { hapaxCount: pyGrouped(r.hapaxCount) })}
        />
        <ReportCard label={tr("平均每曲词汇量")} value={pyFixed(r.avgTypesPerSong, 0)} note="Types / Song" />
        <ReportCard label={tr("平均每行 token")} value={pyFixed(r.avgTokensPerLine, 1)} note="Tokens / Line" />
      </div>

      <h3>{tr("词性分布")}</h3>
      <table className="data">
        <thead>
          <tr>
            <th>{tr("词性")}</th>
            <th>{tr("和名")}</th>
            <th className="num">{tr("Token 数")}</th>
            <th className="num">{tr("比率")}</th>
            <th className="num">{tr("Type 数")}</th>
            <th className="bar-col" />
          </tr>
        </thead>
        <tbody>
          {r.posDist.map((p) => (
            <tr key={p.pos}>
              <td title={POS_LABELS[p.pos]}>{p.pos}</td>
              <td>{POS_JA[p.pos] ?? p.pos}</td>
              <td className="num">{pyGrouped(p.tokenCount)}</td>
              <td className="num">{pyFixed(p.tokenPct, 1)}%</td>
              <td className="num">{pyGrouped(p.typeCount)}</td>
              <td className="bar-col">
                <Bar value={p.tokenPct} max={maxPct} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <h3>{tr("高频词覆盖")}</h3>
      <div className="report-cards">
        {r.coverage.map((c) => (
          <ReportCard key={c.n} label={`Top ${pyGrouped(c.n)}`} value={`${pyFixed(c.pct, 1)}%`}>
            <Bar value={c.pct} max={100} />
          </ReportCard>
        ))}
      </div>

      <h3>{tr("高频词元 Top 20")}</h3>
      <table className="data">
        <thead>
          <tr>
            <th className="num">{tr("排名")}</th>
            <th>{tr("词元")}</th>
            <th className="num">{tr("出现次数")}</th>
            <th className="bar-col" />
          </tr>
        </thead>
        <tbody>
          {r.topWords.map(([lemma, freq], i) => (
            <tr key={lemma}>
              <td className="num muted">{i + 1}</td>
              <td>
                <button className="word-link" onClick={() => onWord(lemma)}>
                  {lemma}
                </button>
              </td>
              <td className="num">{pyGrouped(freq)}</td>
              <td className="bar-col">
                <Bar value={freq} max={maxFreq} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
