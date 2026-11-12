import { YearInput } from "../components/YearInput";
import { Segmented } from "../components/Segmented";
import { Icon } from "../components/Icon";
/**
 * KWIC 检索页。
 *
 * 和普通歌词搜索的区别是**关键词居中**：左语境 / 关键词 / 右语境三段对齐，
 * 一眼能看出这个词在不同歌里的搭配。这是语料库工具的标准视图。
 *
 * 检索和播放器是连着的：
 *
 * - 每条前面的 ▶ 原地试听这一句（到行尾自动停），⟲ 连前后各一行一起听；
 * - 单击一条跳到那首歌的那一行，从那里接着放，歌词跟着高亮；
 * - 正在放的那一条会亮起来。
 *
 * 精度只到**行**：时间戳来自 LRC，没有词级时间。行的起止规则在 Rust 的 `segments.rs`。
 *
 * 右边的「搭配」面板列出关键词左右几个词内常一起出现的词，点一个就把 KWIC 收窄到
 * 「附近有这个词」的那些行——从统计回到具体的歌、具体的那一句。
 *
 * 结果能导出 CSV，列和编码照 PyQt 版 `export_csv`（utf-8-sig、七列、语境不截断）。
 *
 * 和 AntConc 一样能按关键词左右第几个词排序（`kwicSort.ts`），也能切到「分布图」：
 * 每首歌一条，命中的行在歌里的位置画成刻度，一眼看出是散在全曲还是挤在副歌。
 *
 * 「CQL」模式按词的属性组合条件找词串（`[lemma="君"] [pos="ADP"]`，语法见 Rust 的 `cql.rs`），
 * 边输边检查语法。结果和关键词检索同一个形状，排序、分布图、导出都照用。
 * 还能按年份 / 演唱者分解（每万词），单个关键词时能看搭配网络（`KwicExplore.tsx`）。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";

import {
  api,
  formatDuration,
  POS_LABELS,
  type CollocationMeasure,
  type CollocationResult,
  type Collocate,
  type CqlQuery,
  type KwicHit,
  type KwicQuery,
  type WordFrequency,
  type LyricHit,
  type MatchField,
  type NearFilter,
  type PlaybackState,
  type Subcorpus,
} from "../api";
import { PerformerPicker } from "../components/PerformerPicker";
import { SubcorpusPicker } from "../components/SubcorpusPicker";
import { mergeSubcorpus, useActiveScope } from "../subcorpusScope";
import type { WordJump } from "../wordCloud";
import { VirtualList } from "../components/VirtualList";
import { TokenCorrectionDialog } from "../components/TokenCorrectionDialog";
import { audition, isPlayingSpan, playFrom } from "../listen";
import { wordStart } from "../wordTiming";
import { kwicCsv } from "../pyFormat";
import { KWIC_SORTS, sortKwic, type KwicSort } from "../kwicSort";
import { KwicPlot } from "../components/KwicPlot";
import { CollocationNetwork, KwicBreakdown, type SearchSpec } from "../components/KwicExplore";
import type { LibraryActions } from "../useLibrary";
import { tr } from "../i18n";

type Mode = "kwic" | "cql" | "fulltext";

/** CQL 的例子：点一下填进去查 */
const CQL_EXAMPLES: { query: string; note: string }[] = [
  { query: '[lemma="君"] [pos="ADP"]', note: tr("「君」后面紧跟一个助词") },
  { query: '[pos="VERB" & cform="命令形.*"]', note: tr("所有命令形的动词") },
  { query: '[lemma="て"] []{0,2} [lemma="しまう"]', note: tr("中间隔 0–2 个词") },
  { query: '[reading="きみ"]', note: tr("读作 きみ 的各种写法") },
  { query: '[norm="会う"] [lemma="たい"]', note: tr("「会いたい」「逢いたい」…") },
  { query: '[pos="ADJ"] [pos="NOUN"]', note: tr("形容词 + 名词") },
  { query: '"夢" []? [pos="NOUN"]', note: tr("夢，隔 0–1 个词接名词") },
];

/** 可筛选的词性。只列实词——助词做 KWIC 没有研究价值。 */
const POS_FILTERS = ["", "NOUN", "PROPN", "VERB", "ADJ", "ADV"] as const;

/** 结果行高。必须和 CSS 里 .kwic-row / .text-row 的高度一致，
 *  不一致会让虚拟列表滚动时跳动。 */
const KWIC_ROW_HEIGHT = 30;
/** 正文 13px + 出处 11px，行高 1.45，加上 2px 间距和上下 6px 内边距 ≈ 49px，留一点余量 */
const TEXT_ROW_HEIGHT = 52;

/** 上限。虚拟化之后渲染不再是瓶颈，但一次查询返回太多行本身就没有研究价值。 */
const KWIC_LIMIT = 5000;

const MEASURES: { value: CollocationMeasure; label: string; title: string }[] = [
  { value: "logDice", label: "logDice", title: tr("不受语料大小影响") },
  { value: "mi", label: "MI", title: tr("互信息：偏爱罕见的固定搭配") },
  { value: "mi3", label: "MI3", title: tr("加权互信息，少偏罕见词") },
  { value: "logLikelihood", label: "LL", title: tr("对数似然：显著性") },
  { value: "deltaP", label: "ΔP", title: tr("有方向的关联强度") },
  { value: "frequency", label: tr("次数"), title: tr("共现次数") },
];

function measureValue(c: Collocate, measure: CollocationMeasure): string {
  switch (measure) {
    case "logDice":
      return c.logDice.toFixed(2);
    case "mi":
      return c.mi.toFixed(2);
    case "mi3":
      return c.mi3.toFixed(1);
    case "logLikelihood":
      return c.logLikelihood.toFixed(1);
    case "deltaP":
      return c.deltaP.toFixed(3);
    default:
      return String(c.freq);
  }
}

/** 别的页面要求查一个词（分析页的「检索」）。nonce 让同一个词点两次也会再查 */
export interface KwicRequest {
  keyword: string;
  field: MatchField;
  nonce: number;
  /** 从词云来的：歌手、年份、词性和词云一样。没有就沿用这一页现有的筛选 */
  jump?: WordJump;
  /** 只看这种活用形（分析页「活用形」点进来的）。不给就是不限 */
  cform?: string;
}

/** 匹配字段 */
const FIELDS: { value: MatchField; label: string; title: string }[] = [
  { value: "surface", label: tr("表层"), title: tr("按词在歌词里的实际写法匹配") },
  { value: "lemma", label: tr("词元"), title: tr("按辞书形：駆けた→駆ける") },
  { value: "reading", label: tr("读音"), title: tr("按读音：きみ→君、キミ") },
  { value: "normalized", label: tr("表记"), title: tr("按表记：会う→逢う") },
];

/** 可筛选的活用形（大类，按前缀匹配）。用日语语法名，和 Sudachi / UniDic 一致 */
const CFORMS = ["", "命令形", "意志推量形", "仮定形", "未然形", "連用形", "終止形", "連体形"] as const;

/**
 * 多个关键词：空格分隔，或者 PyQt 版的 `A|B|C`、`(A|B|C)` 写法。
 * `A+B` 是连续的多个词（「て+しまう」），整体算一个关键词，不在这里拆。
 */
export function parseKeywords(raw: string): string[] {
  let s = raw.trim();
  if (s.startsWith("(") && s.endsWith(")")) s = s.slice(1, -1);
  return s
    .replace(/\s*\+\s*/g, "+")
    .split(/[|\s]+/)
    .filter(Boolean);
}

/** 一次 KWIC 检索里「筛选条件」那部分（不含关键词和附近词）。两次的结果一样，条件就没变 */
export function kwicFilterKey(q: Pick<KwicQuery, "field" | "pos" | "cform" | "crossLine" | "dedup" | "jpOnly" | "subcorpus">): string {
  return JSON.stringify([q.field, q.pos ?? null, q.cform ?? null, q.crossLine ?? false, q.dedup ?? false, q.jpOnly ?? false, q.subcorpus ?? null]);
}

/** CQL 检索的筛选条件 */
export function cqlFilterKey(q: Pick<CqlQuery, "subcorpus" | "dedup" | "crossLine">): string {
  return JSON.stringify([q.crossLine ?? false, q.dedup ?? false, q.subcorpus ?? null]);
}

/** 年份输入框里的字符串 → 子语料。空的、不是四位数的都当没填 */
export function yearSubcorpus(from: string, to: string, performerIds: number[]): Subcorpus {
  const year = (s: string) => (/^\d{4}$/.test(s.trim()) ? Number(s.trim()) : null);
  return { yearFrom: year(from), yearTo: year(to), performerIds };
}

interface Props {
  actions: LibraryActions;
  playback: PlaybackState;
  onNavigate: () => void;
  onError: (message: string) => void;
  request: KwicRequest | null;
  onRequestConsumed: () => void;
}

export function KwicPage({ actions, playback, onNavigate, onError, request, onRequestConsumed }: Props) {
  const [mode, setMode] = useState<Mode>("kwic");
  const [text, setText] = useState("");
  const [field, setField] = useState<MatchField>("surface");
  const [pos, setPos] = useState("");
  const [cform, setCform] = useState("");
  const [crossLine, setCrossLine] = useState(false);
  const [dedup, setDedup] = useState(true);
  const [jpOnly, setJpOnly] = useState(false);
  const [personIds, setPersonIds] = useState<number[]>([]);
  const scope = useActiveScope();
  const [yearFrom, setYearFrom] = useState("");
  const [yearTo, setYearTo] = useState("");
  /** 搭配面板点出来的「附近有某词」 */
  const [near, setNear] = useState<NearFilter | null>(null);
  // 还没查过时给几个高频词当起点：一页空白比什么都不写更劝退
  const [suggestions, setSuggestions] = useState<WordFrequency[]>([]);

  const [kwicHits, setKwicHits] = useState<KwicHit[] | null>(null);
  const [sort, setSort] = useState<KwicSort>("default");
  const [view, setView] = useState<"list" | "plot" | "breakdown" | "network">("list");
  /** 上一次检索的完整条件（关键词或 CQL）。分布图、分解用它再查一遍 */
  const [lastSearch, setLastSearch] = useState<SearchSpec | null>(null);
  /** CQL 的语法错误（边输边查） */
  const [cqlError, setCqlError] = useState("");
  const sortedHits = useMemo(() => (kwicHits ? sortKwic(kwicHits, sort) : null), [kwicHits, sort]);
  const [textHits, setTextHits] = useState<LyricHit[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [exportNote, setExportNote] = useState("");
  /** 正在校正分词的那一行 */
  const [editing, setEditing] = useState<number | null>(null);
  const closeEditor = useCallback(() => setEditing(null), []);

  const [showColloc, setShowColloc] = useState(false);
  const [colloc, setColloc] = useState<CollocationResult | null>(null);
  const [collocBusy, setCollocBusy] = useState(false);
  const [contentOnly, setContentOnly] = useState(true);
  const [measure, setMeasure] = useState<CollocationMeasure>("logDice");

  // 这一页自己选的歌手、年份叠在「当前子语料」上：这里填了的条件以这里为准
  const subcorpus = useMemo(
    () => mergeSubcorpus(yearSubcorpus(yearFrom, yearTo, personIds), scope),
    [yearFrom, yearTo, personIds, scope],
  );
  /** 上一次检索（KWIC / CQL）用的词和筛选条件。筛选条件变了就拿它重查 */
  const ran = useRef<{ mode: "kwic" | "cql"; query: string; filters: string } | null>(null);
  /** 上一次 KWIC 查的是什么。搭配面板跟着它走，而不是跟着输入框里正在打的字 */
  const [searched, setSearched] = useState<{ query: string; field: MatchField } | null>(null);

  const runKwic = useCallback(
    async (
      query: string,
      matchField: MatchField,
      nearFilter: NearFilter | null = near,
      // 刚从别处带着筛选跳过来时，state 还没更新到这一轮，直接用带来的
      given?: { subcorpus: Subcorpus; pos: string; cform: string },
    ) => {
      const keywords = parseKeywords(query);
      if (!keywords.length) return;
      const q: KwicQuery = {
        keywords,
        field: matchField,
        pos: (given ? given.pos : pos) || null,
        cform: (given ? given.cform : cform) || null,
        crossLine,
        dedup,
        jpOnly,
        limit: KWIC_LIMIT,
        subcorpus: given?.subcorpus ?? subcorpus,
        near: nearFilter,
      };
      // 查之前就记下：查失败了也算查过这组条件，不然下面的「条件变了就重查」会一直重试
      ran.current = { mode: "kwic", query, filters: kwicFilterKey(q) };
      setBusy(true);
      setExportNote("");
      try {
        setTextHits(null);
        setKwicHits(await api.kwic(q));
        setLastSearch({ kind: "kwic", query: q });
        setSearched({ query, field: matchField });
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err));
      } finally {
        setBusy(false);
      }
    },
    [pos, cform, crossLine, dedup, jpOnly, subcorpus, near, onError],
  );

  const runCql = useCallback(
    async (query: string) => {
      const q: CqlQuery = { query, subcorpus, dedup, crossLine, limit: KWIC_LIMIT };
      ran.current = { mode: "cql", query, filters: cqlFilterKey(q) };
      setBusy(true);
      setExportNote("");
      try {
        setTextHits(null);
        setKwicHits(await api.cqlSearch(q));
        setLastSearch({ kind: "cql", query: q });
        // 搭配、网络都要一个单独的节点词，CQL 没有
        setSearched(null);
        setShowColloc(false);
        setNear(null);
        setView((v) => (v === "network" ? "list" : v));
        setCqlError("");
      } catch (err) {
        setCqlError(err instanceof Error ? err.message : String(err));
      } finally {
        setBusy(false);
      }
    },
    [subcorpus, dedup, crossLine],
  );

  // CQL 边输边查语法（停手 300 毫秒后）
  useEffect(() => {
    if (mode !== "cql" || !text.trim()) {
      setCqlError("");
      return;
    }
    const timer = window.setTimeout(() => {
      api
        .cqlCheck(text)
        .then(() => setCqlError(""))
        .catch((err) => setCqlError(err instanceof Error ? err.message : String(err)));
    }, 300);
    return () => window.clearTimeout(timer);
  }, [mode, text]);

  const search = useCallback(async () => {
    const query = text.trim();
    if (!query) return;
    if (mode === "fulltext") ran.current = null;
    if (mode === "kwic") {
      await runKwic(query, field);
      return;
    }
    if (mode === "cql") {
      await runCql(query);
      return;
    }
    setBusy(true);
    setExportNote("");
    try {
      setKwicHits(null);
      setLastSearch(null);
      setTextHits(await api.searchLyrics(query, 500));
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [mode, text, field, runKwic, runCql, onError]);

  // 查过之后再改筛选条件（匹配方式、词性、活用形、歌手、年份、跨行、去重……）：按上次的词马上重查。
  // 原来结果停在旧条件上，工具栏显示的却是新条件，看不出哪些命中其实不符合现在的筛选
  const kwicKey = kwicFilterKey({ field, pos: pos || null, cform: cform || null, crossLine, dedup, jpOnly, subcorpus });
  const cqlKey = cqlFilterKey({ subcorpus, dedup, crossLine });
  useEffect(() => {
    const last = ran.current;
    // 正在查的那一趟回来之后（busy 变回 false）再比一次，不叠着发
    if (last === null || busy || last.mode !== mode) return;
    if (mode === "kwic" && last.filters !== kwicKey) void runKwic(last.query, field);
    else if (mode === "cql" && last.filters !== cqlKey) void runCql(last.query);
  }, [mode, busy, kwicKey, cqlKey, field, runKwic, runCql]);

  useEffect(() => {
    api
      .wordFrequency("NOUN", 12)
      .then(setSuggestions)
      .catch(() => undefined);
  }, []);

  // 分析页、词云点了「检索」：切到 KWIC、按词元查
  useEffect(() => {
    if (!request) return;
    setMode("kwic");
    setText(request.keyword);
    setField(request.field);
    setNear(null);
    const wantCform = request.cform ?? "";
    setCform(wantCform);
    let given: { subcorpus: Subcorpus; pos: string; cform: string } = { subcorpus, pos, cform: wantCform };
    const jump = request.jump;
    if (jump) {
      const from = jump.yearFrom === null ? "" : String(jump.yearFrom);
      const to = jump.yearTo === null ? "" : String(jump.yearTo);
      const p = (POS_FILTERS as readonly string[]).includes(jump.pos) ? jump.pos : "";
      setPersonIds(jump.performerIds);
      setYearFrom(from);
      setYearTo(to);
      setPos(p);
      given = { subcorpus: mergeSubcorpus(yearSubcorpus(from, to, jump.performerIds), scope), pos: p, cform: wantCform };
    }
    onRequestConsumed();
    void runKwic(request.keyword, request.field, null, given);
    // 只在来了新请求时跑；subcorpus / pos 是当时的值
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [request, runKwic, onRequestConsumed, scope]);

  // 搭配：只对单个词有意义（多个候选词、多词表达都没有唯一的节点）
  const node = useMemo(() => {
    const words = searched ? parseKeywords(searched.query) : [];
    return words.length === 1 && !words[0]!.includes("+") ? words[0]! : "";
  }, [searched]);

  useEffect(() => {
    if (!showColloc || !node || !searched) {
      setColloc(null);
      return;
    }
    let cancelled = false;
    setCollocBusy(true);
    api
      .collocations({
        node,
        field: searched.field,
        scope: { subcorpus, unit: dedup ? "distinctLines" : "tokens", jpOnly },
        left: 3,
        right: 3,
        minFreq: 2,
        contentOnly,
        measure,
        limit: 60,
      })
      .then((r) => !cancelled && setColloc(r))
      .catch((err) => !cancelled && onError(err instanceof Error ? err.message : String(err)))
      .finally(() => !cancelled && setCollocBusy(false));
    return () => {
      cancelled = true;
    };
  }, [showColloc, node, searched, subcorpus, dedup, jpOnly, contentOnly, measure, onError]);

  const open = useCallback(
    async (songId: string, utteranceId: number, timeSec: number | null) => {
      onNavigate();
      await playFrom(actions, songId, utteranceId, timeSec);
    },
    [actions, onNavigate],
  );

  /**
   * 单击一条：从命中的那个词开始放（有逐字时间就从词起，没有就从行首），人留在检索页，
   * 底部「正在唱」字幕跟着走。要看整首再双击，或点字幕上的「回到这一行」。和词云同一套规矩。
   */
  const playHit = useCallback(
    async (hit: KwicHit) => {
      try {
        const timings = await api.wordTimings([hit.utteranceId]).catch(() => []);
        const at = wordStart(hit, timings);
        await playFrom(actions, hit.songId, hit.utteranceId, at?.sec ?? hit.timeSec);
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err));
      }
    },
    [actions, onError],
  );

  const listen = useCallback(
    async (utteranceId: number, context: boolean) => {
      try {
        const ok = await audition(actions, utteranceId, context ? { before: 1, after: 1 } : {});
        if (!ok) onError(tr("这一行没有时间戳"));
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err));
      }
    },
    [actions, onError],
  );

  const pickCollocate = useCallback(
    (lemma: string) => {
      if (!searched) return;
      const filter: NearFilter = { word: lemma, field: "lemma", window: 3 };
      setNear(filter);
      void runKwic(searched.query, searched.field, filter);
    },
    [searched, runKwic],
  );

  const clearNear = useCallback(() => {
    setNear(null);
    if (searched) void runKwic(searched.query, searched.field, null);
  }, [searched, runKwic]);

  const exportCsv = useCallback(async () => {
    if (!kwicHits?.length) return;
    try {
      const name = parseKeywords(text).join("_").replace(/[\\/:*?"<>|]/g, "_") || "kwic";
      const path = await save({
        title: tr("保存检索结果"),
        defaultPath: `${name}.csv`,
        filters: [{ name: tr("CSV 文件"), extensions: ["csv"] }],
      });
      if (!path) return;
      await api.exportText(path, kwicCsv(sortedHits ?? kwicHits));
      setExportNote(tr("已保存 {length} 条：{path}", { length: kwicHits.length, path }));
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  }, [kwicHits, sortedHits, text, onError]);

  const hitCount = kwicHits?.length ?? textHits?.length ?? null;
  const songCount = useMemo(() => (kwicHits ? new Set(kwicHits.map((h) => h.songId)).size : 0), [kwicHits]);

  return (
    <div className="page kwic-page">
      <div className="toolbar">
        <Segmented
          label={tr("检索方式")}
          value={mode}
          onChange={setMode}
          options={[
            { value: "kwic", label: "KWIC" },
            { value: "cql", label: "CQL", title: '[lemma="君"] [pos="ADP"]' },
            { value: "fulltext", label: tr("全文"), title: tr("对歌词原文做全文检索（FTS5）") },
          ]}
        />

        <input
          className="search grow"
          placeholder={
            mode === "kwic"
              ? tr("空格分隔多个，+ 连成词组")
              : mode === "cql"
                ? '[lemma="君"] [pos="ADP"]'
                : tr("歌词原文片段")
          }
          spellCheck={false}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            setNear(null);
          }}
          onKeyDown={(e) => e.key === "Enter" && void search()}
          autoFocus
        />
        <button className="primary" onClick={() => void search()} disabled={busy || !text.trim() || (mode === "cql" && !!cqlError)}>
          {busy ? tr("检索中…") : tr("检索")}
        </button>

        {mode !== "fulltext" && (
          <>
            {mode === "kwic" && (
            <>
            <Segmented label={tr("按什么匹配")} value={field} onChange={setField} options={FIELDS} />
            <select className="rate" value={pos} onChange={(e) => setPos(e.target.value)} aria-label={tr("词性")}>
              {POS_FILTERS.map((p) => (
                <option key={p} value={p}>
                  {p === "" ? tr("全部词性") : (POS_LABELS[p] ?? p)}
                </option>
              ))}
            </select>
            <select
              className="rate"
              value={cform}
              onChange={(e) => setCform(e.target.value)}
              aria-label={tr("活用形")}
              title={tr("只看这种活用形（含细类）")}
            >
              {CFORMS.map((c) => (
                <option key={c} value={c}>
                  {c === "" ? tr("全部活用形") : c}
                </option>
              ))}
            </select>
            </>
            )}
            <PerformerPicker selected={personIds} onChange={setPersonIds} onError={onError} />
            <SubcorpusPicker onError={onError} />
            <span className="year-range" title={tr("按发行年份筛选，含两端")}>
              <YearInput value={yearFrom} onCommit={setYearFrom} ariaLabel={tr("起始年份")} />
              –
              <YearInput value={yearTo} onCommit={setYearTo} ariaLabel={tr("结束年份")} />
            </span>
            <label className="check" title={tr("把命中行上下各一行拼进语境")}>
              <input
                type="checkbox"
                checked={crossLine}
                onChange={(e) => setCrossLine(e.target.checked)}
              />{tr("跨行")}</label>
            <label className="check" title={tr("同一首里重复的行只算一次")}>
              <input
                type="checkbox"
                checked={dedup}
                onChange={(e) => setDedup(e.target.checked)}
              />{tr("去重")}</label>
            {mode === "kwic" && (
            <label className="check" title={tr("去掉英文、数字的命中")}>
              <input
                type="checkbox"
                checked={jpOnly}
                onChange={(e) => setJpOnly(e.target.checked)}
              />{tr("仅日文")}</label>
            )}
          </>
        )}

      </div>

      {mode === "cql" && cqlError && <p className="cql-error pad-x">{cqlError}</p>}

      {hitCount !== null && (
        <div className="hit-bar pad-x">
          <span className="muted small">{tr("{hitCount} 条命中", { hitCount })}{kwicHits && tr(" · {songCount} 首歌", { songCount })}
            {hitCount >= KWIC_LIMIT && tr("（已截断到 {KWIC_LIMIT} 条）", { KWIC_LIMIT })}
          </span>
          {near && (
            <span className="near-chip" title={tr("左右 3 个词内有它的行")}>{tr("附近有「{word}」", { word: near.word })}<button className="link-btn" onClick={clearNear} aria-label={tr("取消附近词筛选")}>
                <Icon name="close" />
              </button>
            </span>
          )}
          {kwicHits && kwicHits.length > 0 && (
            <>
              <Segmented
                className="small-seg"
                label={tr("结果怎么看")}
                value={view}
                onChange={setView}
                options={[
                  { value: "list" as const, label: tr("列表") },
                  { value: "plot" as const, label: tr("分布图"), title: tr("命中在每首歌里的位置") },
                  { value: "breakdown" as const, label: tr("年份·歌手"), title: tr("按年份、歌手，每万词") },
                  ...(node ? [{ value: "network" as const, label: tr("网络"), title: tr("关键词和搭配词的共现网") }] : []),
                ]}
              />
              {view === "list" && (
                <label className="kwic-sort" title={tr("按左右第几个词排序")}>
                  {tr("排序")}
                  <select className="rate" value={sort} onChange={(e) => setSort(e.target.value as KwicSort)}>
                    {KWIC_SORTS.map((s) => (
                      <option key={s.value} value={s.value}>
                        {s.label}
                      </option>
                    ))}
                  </select>
                </label>
              )}
              <button className="link-btn" onClick={() => void exportCsv()} title={tr("按当前排序，UTF-8")}>{tr("导出 CSV")}</button>
            </>
          )}
          {kwicHits && (
            <button
              className="link-btn"
              onClick={() => setShowColloc((v) => !v)}
              disabled={!node}
              title={node ? tr("左右 3 个词内的常见搭配") : tr("搭配只对单个关键词有意义")}
            >
              {showColloc ? tr("收起搭配") : tr("搭配")}
            </button>
          )}
          {exportNote && <span className="muted small">{exportNote}</span>}
        </div>
      )}

      {hitCount === null && !busy && mode === "cql" && (
        <div className="kwic-intro cql-help">
          <p className="muted">{tr("[…] 是一个词，连写是相邻的词")}</p>
          <div className="cql-examples">
            {CQL_EXAMPLES.map((ex) => (
              <button
                key={ex.query}
                className="cql-example"
                onClick={() => {
                  setText(ex.query);
                  void runCql(ex.query);
                }}
              >
                <code>{ex.query}</code>
                <span className="muted small">{ex.note}</span>
              </button>
            ))}
          </div>
          <table className="cql-ref small">
            <tbody>
              <tr><td><code>word</code> <code>lemma</code></td><td>{tr("写法、辞书形")}</td></tr>
              <tr><td><code>pos</code></td><td>{tr("词性")} NOUN VERB ADJ ADV ADP AUX PRON PART PUNCT …</td></tr>
              <tr><td><code>tag</code> <code>ctype</code> <code>cform</code></td><td>{tr("日语词性、活用型、活用形")}</td></tr>
              <tr><td><code>reading</code> <code>norm</code></td><td>{tr("读音、正规化表记")}</td></tr>
              <tr><td><code>&amp;</code> <code>|</code> <code>!</code> <code>!=</code></td><td>{tr("且、或、非、不等于")}</td></tr>
              <tr><td><code>[]</code> <code>?</code> <code>*</code> <code>+</code> <code>{"{m,n}"}</code></td><td>{tr("任意词；0–1、≥0、≥1、m–n 次")}</td></tr>
            </tbody>
          </table>
          <p className="muted small">{tr("不跨行，标点也算一个词。")}</p>
        </div>
      )}

      {hitCount === null && !busy && mode !== "cql" && (
        <div className="kwic-intro">
          {suggestions.length > 0 && (
            <>
              <p className="muted small">{tr("从语料里最常出现的词开始：")}</p>
              <div className="word-cloud">
                {suggestions.map((w) => (
                  <button
                    key={`${w.lemma}-${w.pos}`}
                    className="word-chip"
                    onClick={() => {
                      setMode("kwic");
                      setField("lemma");
                      setText(w.lemma);
                      setNear(null);
                      void runKwic(w.lemma, "lemma", null);
                    }}
                  >
                    {w.lemma}
                    <em>{w.freq}</em>
                  </button>
                ))}
              </div>
            </>
          )}
        </div>
      )}

      <div className="kwic-body">
        {kwicHits && view === "plot" && lastSearch && (
          <KwicPlot
            queryKey={JSON.stringify(lastSearch)}
            limit={KWIC_LIMIT}
            fetch={() =>
              lastSearch.kind === "cql"
                ? api.cqlSearch({ ...lastSearch.query, dedup: false })
                : api.kwic({ ...lastSearch.query, dedup: false })
            }
            onOpen={(h) => void playHit(h)}
            onError={onError}
          />
        )}

        {kwicHits && view === "breakdown" && lastSearch && <KwicBreakdown search={lastSearch} onError={onError} />}

        {kwicHits && view === "network" && node && searched && (
          <CollocationNetwork
            node={node}
            field={searched.field}
            scope={{ subcorpus, unit: dedup ? "distinctLines" : "tokens", jpOnly }}
            measure={measure}
            contentOnly={contentOnly}
            onPick={(lemma) => {
              setView("list");
              pickCollocate(lemma);
            }}
            onError={onError}
          />
        )}

        {sortedHits && view === "list" && (
          <VirtualList
            className="results"
            items={sortedHits}
            itemHeight={KWIC_ROW_HEIGHT}
            itemKey={(hit, i) => `${hit.utteranceId}-${i}`}
            empty={<p className="muted pad">{tr("没有命中")}</p>}
            renderItem={(hit) => {
              const playing = isPlayingSpan(playback, hit.songId, hit.timeSec, hit.endSec);
              const timed = hit.timeSec !== null;
              return (
                <div
                  className={`kwic-row with-actions ${playing ? "playing" : ""}`}
                  role="button"
                  tabIndex={0}
                  onClick={() => void playHit(hit)}
                  onDoubleClick={() => void open(hit.songId, hit.utteranceId, hit.timeSec)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void playHit(hit);
                    if (e.key === " ") {
                      e.preventDefault();
                      void listen(hit.utteranceId, false);
                    }
                  }}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setEditing(hit.utteranceId);
                  }}
                  title={tr("单击播放 · 双击看整首 · 空格试听 · 右键校正分词")}
                >
                  <span className="kwic-actions">
                    <button
                      className="kwic-play"
                      disabled={!timed}
                      onClick={(e) => {
                        e.stopPropagation();
                        void listen(hit.utteranceId, false);
                      }}
                      title={
                        timed
                          ? tr("试听这一句（{timeSec}–{formatDuration}{v}）", { timeSec: formatDuration(hit.timeSec!), formatDuration: formatDuration(hit.endSec ?? hit.timeSec!), v: hit.endKind === "estimated" ? tr("，结尾按字数估") : "" })
                          : tr("这一行没有时间戳")
                      }
                      aria-label={tr("试听这一句")}
                    >
                      <Icon name="play" />
                    </button>
                    <button
                      className="kwic-play"
                      disabled={!timed}
                      onClick={(e) => {
                        e.stopPropagation();
                        void listen(hit.utteranceId, true);
                      }}
                      title={tr("连前后各一行一起听")}
                      aria-label={tr("连前后文试听")}
                    >
                      <Icon name="contextPlay" />
                    </button>
                  </span>
                  <span className="kwic-left">{hit.left}</span>
                  <span className="kwic-key">{hit.keyword}</span>
                  <span className="kwic-right">{hit.right}</span>
                  <span className="kwic-meta">
                    {hit.corrected && (
                      <span className="tc-mark" title={tr("这一行做过分词校正")}>
                        <Icon name="edit" />
                      </span>
                    )}
                    {hit.artist} · {hit.title}
                    {hit.repeatCount > 1 && <em className="repeat"> <Icon name="close" />{hit.repeatCount}</em>}
                    {timed && <span className="kwic-time">{formatDuration(hit.timeSec!)}</span>}
                  </span>
                </div>
              );
            }}
          />
        )}

        {textHits && (
          <VirtualList
            className="results"
            items={textHits}
            itemHeight={TEXT_ROW_HEIGHT}
            itemKey={(hit) => String(hit.utteranceId)}
            empty={<p className="muted pad">{tr("没有命中")}</p>}
            renderItem={(hit) => (
              <button
                className="text-row"
                onClick={() => void open(hit.songId, hit.utteranceId, hit.timeSec)}
                title={hit.text}
              >
                <span className="ex-text">{hit.text}</span>
                <span className="ex-meta">
                  {hit.artist} · {hit.title}
                  {hit.timeSec !== null && ` · ${formatDuration(hit.timeSec)}`}
                </span>
              </button>
            )}
          />
        )}

        {showColloc && kwicHits && (
          <aside className="colloc-panel">
            <div className="colloc-head">
              <strong>{tr("「{node}」的搭配", { node })}</strong>
              <span className="muted small">{tr("左右 3 词 · 不跨行")}</span>
            </div>
            <div className="colloc-controls">
              <Segmented label={tr("搭配强度怎么算")} value={measure} onChange={setMeasure} options={MEASURES} />
              <label className="check" title={tr("去掉助词、助动词、连词")}>
                <input type="checkbox" checked={contentOnly} onChange={(e) => setContentOnly(e.target.checked)} />{tr("只看实词")}</label>
            </div>
            {collocBusy && <p className="muted small">{tr("计算中…")}</p>}
            {colloc && !collocBusy && (
              <>
                <table className="colloc-table">
                  <thead>
                    <tr>
                      <th>{tr("词")}</th>
                      <th title={tr("共现次数（左 / 右）")}>{tr("共现")}</th>
                      <th title={tr("出现在几首歌里")}>{tr("歌")}</th>
                      <th>{MEASURES.find((m) => m.value === measure)?.label}</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {colloc.collocates.map((c) => (
                      <tr
                        key={`${c.lemma}-${c.pos}`}
                        className={near?.word === c.lemma ? "on" : ""}
                        tabIndex={0}
                        aria-selected={near?.word === c.lemma}
                        onClick={() => pickCollocate(c.lemma)}
                        onKeyDown={(e) => {
                          if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) {
                            e.preventDefault();
                            pickCollocate(c.lemma);
                          }
                        }}
                        title={tr("只看附近有「{lemma}」的行 · 全库 {collocateFreq} 次", { lemma: c.lemma, collocateFreq: c.collocateFreq })}
                      >
                        <td>
                          {c.lemma} <span className="muted small">{POS_LABELS[c.pos] ?? c.pos}</span>
                        </td>
                        <td className="num">
                          {c.freq}{" "}
                          <span className="muted small">
                            {c.freqLeft}/{c.freqRight}
                          </span>
                        </td>
                        <td className="num">{c.songs}</td>
                        <td
                          className="num"
                          title={tr("logDice {logDice} · MI {mi} · MI3 {mi3} · LL {ll} · t {t} · z {z} · ΔP 关键词→此词 {dp} / 此词→关键词 {dpr}", {
                            logDice: c.logDice.toFixed(2),
                            mi: c.mi.toFixed(2),
                            mi3: c.mi3.toFixed(1),
                            ll: c.logLikelihood.toFixed(1),
                            t: c.tScore.toFixed(2),
                            z: c.zScore.toFixed(2),
                            dp: c.deltaP.toFixed(3),
                            dpr: c.deltaPReverse.toFixed(3),
                          })}
                        >
                          {measureValue(c, measure)}
                        </td>
                        <td>
                          <button
                            className="kwic-play"
                            onClick={(e) => {
                              e.stopPropagation();
                              void listen(c.exampleUtteranceId, false);
                            }}
                            title={tr("听一句两者一起出现的歌词")}
                            aria-label={tr("试听一句例子")}
                          >
                            <Icon name="play" />
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {colloc.collocates.length === 0 && <p className="muted small">{tr("没有共现 2 次以上的词")}</p>}
                <p
                  className="provenance muted small"
                  title={tr("规则 {rules}；库指纹 {fingerprint}", { rules: colloc.provenance.rules, fingerprint: colloc.provenance.snapshot.fingerprint })}
                >{tr("节点 {nodeFreq} 次 · {scope} · {songs} 首 /{v}{tokens} 词（", { nodeFreq: colloc.nodeFreq, scope: colloc.provenance.scope, songs: colloc.provenance.songs, v: " ", tokens: colloc.provenance.tokens.toLocaleString() })}{colloc.provenance.unit === "distinctLines" ? tr("重复行只算一次") : tr("全部词次")}{tr("）")}
                </p>
              </>
            )}
          </aside>
        )}
      </div>

      {editing !== null && (
        <TokenCorrectionDialog
          utteranceId={editing}
          onClose={closeEditor}
          onChanged={() => void search()}
          onError={onError}
        />
      )}
    </div>
  );
}
