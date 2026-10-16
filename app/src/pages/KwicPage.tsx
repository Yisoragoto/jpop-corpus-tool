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
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";

import {
  api,
  formatDuration,
  POS_LABELS,
  type CollocationMeasure,
  type CollocationResult,
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
import { VirtualList } from "../components/VirtualList";
import { TokenCorrectionDialog } from "../components/TokenCorrectionDialog";
import { audition, isPlayingSpan, playFrom } from "../listen";
import { kwicCsv } from "../pyFormat";
import type { LibraryActions } from "../useLibrary";

type Mode = "kwic" | "fulltext";

/** 可筛选的词性。只列实词——助词做 KWIC 没有研究价值。 */
const POS_FILTERS = ["", "NOUN", "PROPN", "VERB", "ADJ", "ADV"] as const;

/** 结果行高。必须和 CSS 里 .kwic-row / .text-row 的高度一致，
 *  不一致会让虚拟列表滚动时跳动。 */
const KWIC_ROW_HEIGHT = 30;
const TEXT_ROW_HEIGHT = 46;

/** 上限。虚拟化之后渲染不再是瓶颈，但一次查询返回太多行本身就没有研究价值。 */
const KWIC_LIMIT = 5000;

const MEASURES: { value: CollocationMeasure; label: string; title: string }[] = [
  { value: "logDice", label: "logDice", title: "不受语料大小影响，低频噪音少" },
  { value: "mi", label: "MI", title: "互信息：偏爱罕见的固定搭配" },
  { value: "logLikelihood", label: "LL", title: "对数似然：显著性" },
  { value: "frequency", label: "次数", title: "共现次数" },
];

/** 别的页面要求查一个词（分析页的「检索」）。nonce 让同一个词点两次也会再查 */
export interface KwicRequest {
  keyword: string;
  field: MatchField;
  nonce: number;
}

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
  const [crossLine, setCrossLine] = useState(false);
  const [dedup, setDedup] = useState(true);
  const [jpOnly, setJpOnly] = useState(false);
  const [personIds, setPersonIds] = useState<number[]>([]);
  const [yearFrom, setYearFrom] = useState("");
  const [yearTo, setYearTo] = useState("");
  /** 搭配面板点出来的「附近有某词」 */
  const [near, setNear] = useState<NearFilter | null>(null);
  // 还没查过时给几个高频词当起点：一页空白比什么都不写更劝退
  const [suggestions, setSuggestions] = useState<WordFrequency[]>([]);

  const [kwicHits, setKwicHits] = useState<KwicHit[] | null>(null);
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

  const subcorpus = useMemo(() => yearSubcorpus(yearFrom, yearTo, personIds), [yearFrom, yearTo, personIds]);
  /** 上一次 KWIC 查的是什么。搭配面板跟着它走，而不是跟着输入框里正在打的字 */
  const [searched, setSearched] = useState<{ query: string; field: MatchField } | null>(null);

  const runKwic = useCallback(
    async (query: string, matchField: MatchField, nearFilter: NearFilter | null = near) => {
      const keywords = parseKeywords(query);
      if (!keywords.length) return;
      setBusy(true);
      setExportNote("");
      try {
        setTextHits(null);
        const q: KwicQuery = {
          keywords,
          field: matchField,
          pos: pos || null,
          crossLine,
          dedup,
          jpOnly,
          limit: KWIC_LIMIT,
          subcorpus,
          near: nearFilter,
        };
        setKwicHits(await api.kwic(q));
        setSearched({ query, field: matchField });
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err));
      } finally {
        setBusy(false);
      }
    },
    [pos, crossLine, dedup, jpOnly, subcorpus, near, onError],
  );

  const search = useCallback(async () => {
    const query = text.trim();
    if (!query) return;
    if (mode === "kwic") {
      await runKwic(query, field);
      return;
    }
    setBusy(true);
    setExportNote("");
    try {
      setKwicHits(null);
      setTextHits(await api.searchLyrics(query, 500));
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [mode, text, field, runKwic, onError]);

  useEffect(() => {
    api
      .wordFrequency("NOUN", 12)
      .then(setSuggestions)
      .catch(() => undefined);
  }, []);

  // 分析页点了「检索」：切到 KWIC、按词元查
  useEffect(() => {
    if (!request) return;
    setMode("kwic");
    setText(request.keyword);
    setField(request.field);
    setNear(null);
    onRequestConsumed();
    void runKwic(request.keyword, request.field, null);
  }, [request, runKwic, onRequestConsumed]);

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

  const listen = useCallback(
    async (utteranceId: number, context: boolean) => {
      try {
        const ok = await audition(actions, utteranceId, context ? { before: 1, after: 1 } : {});
        if (!ok) onError("这一行没有时间戳，没法定位到音频");
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
        title: "保存检索结果",
        defaultPath: `${name}.csv`,
        filters: [{ name: "CSV 文件", extensions: ["csv"] }],
      });
      if (!path) return;
      await api.exportText(path, kwicCsv(kwicHits));
      setExportNote(`已保存 ${kwicHits.length} 条：${path}`);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  }, [kwicHits, text, onError]);

  const hitCount = kwicHits?.length ?? textHits?.length ?? null;
  const songCount = useMemo(() => (kwicHits ? new Set(kwicHits.map((h) => h.songId)).size : 0), [kwicHits]);

  return (
    <div className="page kwic-page">
      <div className="toolbar">
        <div className="segmented">
          <button className={mode === "kwic" ? "on" : ""} onClick={() => setMode("kwic")}>
            KWIC
          </button>
          <button
            className={mode === "fulltext" ? "on" : ""}
            onClick={() => setMode("fulltext")}
            title="对歌词原文做全文检索（FTS5）"
          >
            全文
          </button>
        </div>

        <input
          className="search grow"
          placeholder={mode === "kwic" ? "词元 / 表层形；空格或 | 分隔多个，+ 连接多词表达（て+しまう）" : "歌词原文片段"}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            setNear(null);
          }}
          onKeyDown={(e) => e.key === "Enter" && void search()}
          autoFocus
        />

        {mode === "kwic" && (
          <>
            <div className="segmented">
              <button
                className={field === "surface" ? "on" : ""}
                onClick={() => setField("surface")}
                title="按词在歌词里的实际写法匹配"
              >
                表层
              </button>
              <button
                className={field === "lemma" ? "on" : ""}
                onClick={() => setField("lemma")}
                title="按辞书形匹配，能把「駆ける/駆けた/駆けろ」归到一起"
              >
                词元
              </button>
            </div>
            <select className="rate" value={pos} onChange={(e) => setPos(e.target.value)}>
              {POS_FILTERS.map((p) => (
                <option key={p} value={p}>
                  {p === "" ? "全部词性" : (POS_LABELS[p] ?? p)}
                </option>
              ))}
            </select>
            <PerformerPicker selected={personIds} onChange={setPersonIds} onError={onError} />
            <span className="year-range" title="按歌曲年份筛选（含两端）。没有年份的歌在设了年份时不计入">
              <input
                className="year-input"
                inputMode="numeric"
                placeholder="年份"
                aria-label="起始年份"
                value={yearFrom}
                onChange={(e) => setYearFrom(e.target.value)}
              />
              –
              <input
                className="year-input"
                inputMode="numeric"
                placeholder="年份"
                aria-label="结束年份"
                value={yearTo}
                onChange={(e) => setYearTo(e.target.value)}
              />
            </span>
            <label className="check" title="把命中行上下各一行拼进语境">
              <input
                type="checkbox"
                checked={crossLine}
                onChange={(e) => setCrossLine(e.target.checked)}
              />
              跨行
            </label>
            <label className="check" title="同一首歌里重复的相同歌词行折叠成一条；搭配统计也只算第一次">
              <input
                type="checkbox"
                checked={dedup}
                onChange={(e) => setDedup(e.target.checked)}
              />
              去重
            </label>
            <label className="check" title="只留关键词含假名或汉字的命中（去掉英文、数字）">
              <input
                type="checkbox"
                checked={jpOnly}
                onChange={(e) => setJpOnly(e.target.checked)}
              />
              仅日文
            </label>
          </>
        )}

        <button className="primary" onClick={() => void search()} disabled={busy || !text.trim()}>
          {busy ? "检索中…" : "检索"}
        </button>
      </div>

      {hitCount !== null && (
        <div className="hit-bar pad-x">
          <span className="muted small">
            {hitCount} 条命中
            {kwicHits && ` · ${songCount} 首歌`}
            {hitCount >= KWIC_LIMIT && `（已截断到 ${KWIC_LIMIT} 条）`}
          </span>
          {near && (
            <span className="near-chip" title="只看关键词左右 3 个词内有这个词的行">
              附近有「{near.word}」
              <button className="link-btn" onClick={clearNear} aria-label="取消附近词筛选">
                ×
              </button>
            </span>
          )}
          {kwicHits && kwicHits.length > 0 && (
            <button className="link-btn" onClick={() => void exportCsv()} title="歌手、曲名、时刻、左右语境、全文，UTF-8（带 BOM）">
              导出 CSV
            </button>
          )}
          {kwicHits && (
            <button
              className="link-btn"
              onClick={() => setShowColloc((v) => !v)}
              disabled={!node}
              title={node ? "关键词左右 3 个词内常一起出现的词" : "搭配只对单个关键词有意义"}
            >
              {showColloc ? "收起搭配" : "搭配"}
            </button>
          )}
          {exportNote && <span className="muted small">{exportNote}</span>}
        </div>
      )}

      {hitCount === null && !busy && (
        <div className="kwic-intro">
          <p className="muted">
            KWIC 把关键词居中对齐，一眼能看出它在不同歌里的搭配。
            ▶ 原地试听那一句，⟲ 连前后各一行一起听；单击一条跳到那首歌的那一行接着放。
            右键一条可以校正分词。用 <code>+</code> 连接可以查多词表达，比如 <code>て+しまう</code>（选「词元」）。
          </p>
          {suggestions.length > 0 && (
            <>
              <p className="muted small">从语料里最常出现的词开始：</p>
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
        {kwicHits && (
          <VirtualList
            className="results"
            items={kwicHits}
            itemHeight={KWIC_ROW_HEIGHT}
            itemKey={(hit, i) => `${hit.utteranceId}-${i}`}
            empty={<p className="muted pad">没有命中</p>}
            renderItem={(hit) => {
              const playing = isPlayingSpan(playback, hit.songId, hit.timeSec, hit.endSec);
              const timed = hit.timeSec !== null;
              return (
                <div
                  className={`kwic-row with-actions ${playing ? "playing" : ""}`}
                  role="button"
                  tabIndex={0}
                  onClick={() => void open(hit.songId, hit.utteranceId, hit.timeSec)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void open(hit.songId, hit.utteranceId, hit.timeSec);
                    if (e.key === " ") {
                      e.preventDefault();
                      void listen(hit.utteranceId, false);
                    }
                  }}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setEditing(hit.utteranceId);
                  }}
                  title="单击：跳到这首歌从这一句放 · 空格：试听这一句 · 右键：校正分词"
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
                          ? `试听这一句（${formatDuration(hit.timeSec!)}–${formatDuration(hit.endSec ?? hit.timeSec!)}${hit.endKind === "estimated" ? "，结尾按字数估" : ""}）`
                          : "这一行没有时间戳"
                      }
                      aria-label="试听这一句"
                    >
                      ▶
                    </button>
                    <button
                      className="kwic-play"
                      disabled={!timed}
                      onClick={(e) => {
                        e.stopPropagation();
                        void listen(hit.utteranceId, true);
                      }}
                      title="连前后各一行一起听"
                      aria-label="连前后文试听"
                    >
                      ⟲
                    </button>
                  </span>
                  <span className="kwic-left">{hit.left}</span>
                  <span className="kwic-key">{hit.keyword}</span>
                  <span className="kwic-right">{hit.right}</span>
                  <span className="kwic-meta">
                    {hit.corrected && (
                      <span className="tc-mark" title="这一行做过分词校正">
                        ✏
                      </span>
                    )}
                    {hit.artist} · {hit.title}
                    {hit.repeatCount > 1 && <em className="repeat"> ×{hit.repeatCount}</em>}
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
            empty={<p className="muted pad">没有命中</p>}
            renderItem={(hit) => (
              <button
                className="text-row"
                onClick={() => void open(hit.songId, hit.utteranceId, hit.timeSec)}
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
              <strong>「{node}」的搭配</strong>
              <span className="muted small">左右 3 词 · 不跨行</span>
            </div>
            <div className="colloc-controls">
              <div className="segmented">
                {MEASURES.map((m) => (
                  <button
                    key={m.value}
                    className={measure === m.value ? "on" : ""}
                    onClick={() => setMeasure(m.value)}
                    title={m.title}
                  >
                    {m.label}
                  </button>
                ))}
              </div>
              <label className="check" title="去掉助词、助动词、连词">
                <input type="checkbox" checked={contentOnly} onChange={(e) => setContentOnly(e.target.checked)} />
                只看实词
              </label>
            </div>
            {collocBusy && <p className="muted small">计算中…</p>}
            {colloc && !collocBusy && (
              <>
                <table className="colloc-table">
                  <thead>
                    <tr>
                      <th>词</th>
                      <th title="共现次数（左 / 右）">共现</th>
                      <th title="出现在几首歌里">歌</th>
                      <th>{MEASURES.find((m) => m.value === measure)?.label}</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {colloc.collocates.map((c) => (
                      <tr
                        key={`${c.lemma}-${c.pos}`}
                        className={near?.word === c.lemma ? "on" : ""}
                        onClick={() => pickCollocate(c.lemma)}
                        title={`点一下：只看附近有「${c.lemma}」的行。全库 ${c.collocateFreq} 次`}
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
                        <td className="num">
                          {measure === "logDice"
                            ? c.logDice.toFixed(2)
                            : measure === "mi"
                              ? c.mi.toFixed(2)
                              : measure === "logLikelihood"
                                ? c.logLikelihood.toFixed(1)
                                : c.freq}
                        </td>
                        <td>
                          <button
                            className="kwic-play"
                            onClick={(e) => {
                              e.stopPropagation();
                              void listen(c.exampleUtteranceId, false);
                            }}
                            title="听一句两者一起出现的歌词"
                            aria-label="试听一句例子"
                          >
                            ▶
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {colloc.collocates.length === 0 && <p className="muted small">没有共现 2 次以上的词</p>}
                <p
                  className="provenance muted small"
                  title={`规则 ${colloc.provenance.rules}；库指纹 ${colloc.provenance.snapshot.fingerprint}`}
                >
                  节点 {colloc.nodeFreq} 次 · {colloc.provenance.scope} · {colloc.provenance.songs} 首 /{" "}
                  {colloc.provenance.tokens.toLocaleString()} 词（
                  {colloc.provenance.unit === "distinctLines" ? "重复行只算一次" : "全部词次"}）
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
