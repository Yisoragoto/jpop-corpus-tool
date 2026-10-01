/**
 * KWIC 检索页。
 *
 * 和普通歌词搜索的区别是**关键词居中**：左语境 / 关键词 / 右语境三段对齐，
 * 一眼能看出这个词在不同歌里的搭配。这是语料库工具的标准视图。
 *
 * 每条命中都能点开——跳到那首歌的那一行并定位到时间点。检索不是死胡同，
 * 是进入曲目的另一个入口。
 *
 * 结果能导出 CSV，列和编码照 PyQt 版 `export_csv`（utf-8-sig、七列、语境不截断）。
 */

import { useCallback, useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";

import {
  api,
  formatDuration,
  POS_LABELS,
  type KwicHit,
  type WordFrequency,
  type LyricHit,
  type MatchField,
} from "../api";
import { PerformerPicker } from "../components/PerformerPicker";
import { VirtualList } from "../components/VirtualList";
import { TokenCorrectionDialog } from "../components/TokenCorrectionDialog";
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

/** 别的页面要求查一个词（分析页的「检索」）。nonce 让同一个词点两次也会再查 */
export interface KwicRequest {
  keyword: string;
  field: MatchField;
  nonce: number;
}

/**
 * 多个关键词：空格分隔，或者 PyQt 版的 `A|B|C`、`(A|B|C)` 写法。
 */
export function parseKeywords(raw: string): string[] {
  let s = raw.trim();
  if (s.startsWith("(") && s.endsWith(")")) s = s.slice(1, -1);
  return s.split(/[|\s]+/).filter(Boolean);
}

interface Props {
  actions: LibraryActions;
  onNavigate: () => void;
  onError: (message: string) => void;
  request: KwicRequest | null;
  onRequestConsumed: () => void;
}

export function KwicPage({ actions, onNavigate, onError, request, onRequestConsumed }: Props) {
  const [mode, setMode] = useState<Mode>("kwic");
  const [text, setText] = useState("");
  const [field, setField] = useState<MatchField>("surface");
  const [pos, setPos] = useState("");
  const [crossLine, setCrossLine] = useState(false);
  const [dedup, setDedup] = useState(true);
  const [jpOnly, setJpOnly] = useState(false);
  const [personIds, setPersonIds] = useState<number[]>([]);
  // 还没查过时给几个高频词当起点：一页空白比什么都不写更劝退
  const [suggestions, setSuggestions] = useState<WordFrequency[]>([]);

  const [kwicHits, setKwicHits] = useState<KwicHit[] | null>(null);
  const [textHits, setTextHits] = useState<LyricHit[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [exportNote, setExportNote] = useState("");
  /** 正在校正分词的那一行 */
  const [editing, setEditing] = useState<number | null>(null);
  const closeEditor = useCallback(() => setEditing(null), []);

  const runKwic = useCallback(
    async (query: string, matchField: MatchField) => {
      const keywords = parseKeywords(query);
      if (!keywords.length) return;
      setBusy(true);
      setExportNote("");
      try {
        setTextHits(null);
        setKwicHits(
          await api.kwic({
            keywords,
            field: matchField,
            pos: pos || null,
            personIds,
            crossLine,
            dedup,
            jpOnly,
            limit: KWIC_LIMIT,
          }),
        );
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err));
      } finally {
        setBusy(false);
      }
    },
    [pos, personIds, crossLine, dedup, jpOnly, onError],
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
    onRequestConsumed();
    void runKwic(request.keyword, request.field);
  }, [request, runKwic, onRequestConsumed]);

  const open = useCallback(
    async (songId: string, utteranceId: number, timeSec: number | null) => {
      onNavigate();
      await actions.openTrackById(songId, {
        scrollTo: utteranceId,
        seekTo: timeSec,
        autoPlay: false,
      });
    },
    [actions, onNavigate],
  );

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
          placeholder={mode === "kwic" ? "词元 / 表层形，空格或 | 分隔多个" : "歌词原文片段"}
          value={text}
          onChange={(e) => setText(e.target.value)}
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
            <label className="check" title="把命中行上下各一行拼进语境">
              <input
                type="checkbox"
                checked={crossLine}
                onChange={(e) => setCrossLine(e.target.checked)}
              />
              跨行
            </label>
            <label className="check" title="同一首歌里重复的相同歌词行折叠成一条">
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
            {hitCount >= KWIC_LIMIT && `（已截断到 ${KWIC_LIMIT} 条）`}
          </span>
          {kwicHits && kwicHits.length > 0 && (
            <button className="link-btn" onClick={() => void exportCsv()} title="歌手、曲名、时刻、左右语境、全文，UTF-8（带 BOM）">
              导出 CSV
            </button>
          )}
          {exportNote && <span className="muted small">{exportNote}</span>}
        </div>
      )}

      {hitCount === null && !busy && (
        <div className="kwic-intro">
          <p className="muted">
            KWIC 把关键词居中对齐，一眼能看出它在不同歌里的搭配。
            点任意一条可以跳到那首歌的那一行，右键一条可以校正分词。
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
                      void runKwic(w.lemma, "lemma");
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

      {kwicHits && (
        <VirtualList
          className="results"
          items={kwicHits}
          itemHeight={KWIC_ROW_HEIGHT}
          itemKey={(hit, i) => `${hit.utteranceId}-${i}`}
          empty={<p className="muted pad">没有命中</p>}
          renderItem={(hit) => (
            <button
              className="kwic-row"
              onClick={() => void open(hit.songId, hit.utteranceId, hit.timeSec)}
              onContextMenu={(e) => {
                e.preventDefault();
                setEditing(hit.utteranceId);
              }}
              title="右键校正分词"
            >
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
                {hit.timeSec !== null && (
                  <span className="kwic-time">{formatDuration(hit.timeSec)}</span>
                )}
              </span>
            </button>
          )}
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
