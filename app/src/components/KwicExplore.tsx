/**
 * 检索页的两个下钻视图：按年份 / 演唱者分解、搭配网络。计算都在 Rust（`jp-corpus/src/explore.rs`）。
 *
 * - 分解：这次检索在每一年、每位演唱者的歌里各出现几次，折成每万词——不折的话歌多的年份永远最高。
 *   条形长度是每万词；比整体高一倍以上的标出来。命中按全部词次数（不去重），分母也是全部词次；
 * - 网络：节点词在中间，前 20 个搭配词按搭配强度围一圈（越强越靠里），搭配词彼此之间常一起出现的连线。
 *   点一个词：回到列表，只看附近有它的行。画法是固定的放射布局，不用力导向——每次打开位置都一样，好比较。
 */

import { useEffect, useMemo, useState } from "react";

import {
  api,
  POS_LABELS,
  type AnalysisScope,
  type BreakdownResult,
  type CollocationMeasure,
  type CqlQuery,
  type KwicQuery,
  type MatchField,
  type NetworkResult,
} from "../api";
import { tableCsv } from "../pyFormat";
import { saveCsv } from "./saveCsv";
import { tr } from "../i18n";

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

/** 一组词次少于这个数时每万词很不稳（一两首歌里出现一次就能翻倍），标出来 */
const SMALL_GROUP = 1000;

/** 当前检索：关键词或 CQL */
export type SearchSpec = { kind: "kwic"; query: KwicQuery } | { kind: "cql"; query: CqlQuery };

// ────────────────────────────── 分解 ──────────────────────────────

export function KwicBreakdown({ search, onError }: { search: SearchSpec; onError: (m: string) => void }) {
  const [by, setBy] = useState<"year" | "performer">("year");
  const [result, setResult] = useState<BreakdownResult | null>(null);
  const key = JSON.stringify(search);

  useEffect(() => {
    let cancelled = false;
    setResult(null);
    api
      .kwicBreakdown({ by, ...(search.kind === "cql" ? { cql: search.query } : { kwic: search.query }) })
      .then((r) => !cancelled && setResult(r))
      .catch((e) => !cancelled && onError(message(e)));
    return () => {
      cancelled = true;
    };
    // key 就是 search 的内容
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, by, onError]);

  const max = useMemo(() => Math.max(0.0001, ...(result?.rows ?? []).map((r) => r.per10k)), [result]);

  const exportCsv = async () => {
    if (!result) return;
    const csv = tableCsv(
      [by, "hits", "hitSongs", "tokens", "songs", "per10k", "ratio"],
      result.rows.map((r) => [r.label || "", r.hits, r.hitSongs, r.tokens, r.songs, r.per10k, r.ratio]),
    );
    await saveCsv(csv, `breakdown_${by}.csv`, onError);
  };

  return (
    <div className="kwic-explore">
      <div className="explore-head">
        <div className="segmented small-seg">
          <button className={by === "year" ? "on" : ""} onClick={() => setBy("year")}>{tr("按年份")}</button>
          <button className={by === "performer" ? "on" : ""} onClick={() => setBy("performer")}>{tr("按演唱者")}</button>
        </div>
        {result && (
          <span className="muted small">
            {tr("{hits} 次 · 整体每万词 {per10k}（不去重，分母不含标点）", { hits: result.hits.toLocaleString(), per10k: result.per10k.toFixed(2) })}
          </span>
        )}
        <button className="link-btn" disabled={!result?.rows.length} onClick={() => void exportCsv()}>{tr("导出 CSV")}</button>
      </div>
      {!result && <p className="muted small">{tr("计算中…")}</p>}
      {result && (
        <div className="breakdown-rows">
          {result.rows.map((r) => (
            <div
              key={r.key || "none"}
              className={`breakdown-row${r.hits === 0 ? " zero" : ""}${r.tokens < SMALL_GROUP ? " small-sample" : ""}`}
              title={tr("{hits} 次 · {hitSongs} / {songs} 首歌有 · 这一组 {tokens} 词 · 是整体的 {ratio} 倍", {
                hits: r.hits,
                hitSongs: r.hitSongs,
                songs: r.songs,
                tokens: r.tokens.toLocaleString(),
                ratio: r.ratio.toFixed(2),
              })}
            >
              <span className="breakdown-label">
                {r.label || tr("年份不明")}
                {r.tokens < SMALL_GROUP && <sup className="muted"> *</sup>}
              </span>
              <span className="breakdown-bar">
                <span className={r.ratio >= 2 ? "high" : r.ratio > 0 && r.ratio <= 0.5 ? "low" : ""} style={{ width: `${(r.per10k / max) * 100}%` }} />
                <i className="breakdown-mean" style={{ left: `${(result.per10k / max) * 100}%` }} />
              </span>
              <span className="num">{r.per10k.toFixed(1)}</span>
              <span className="num muted">{r.hits}</span>
              <span className="num muted">
                {r.hitSongs}/{r.songs}
              </span>
            </div>
          ))}
          <p className="muted small breakdown-legend">
            {tr("条形是每万词，竖线是整体水平。右边两列：次数、有这个词的歌 / 这一组的歌数。")}{" "}
            {tr("* 这一组不到 {n} 个词，每万词很不稳，一两次出现就能翻倍。", { n: SMALL_GROUP.toLocaleString() })}
          </p>
        </div>
      )}
    </div>
  );
}

// ────────────────────────────── 网络 ──────────────────────────────

export function CollocationNetwork({
  node,
  field,
  scope,
  measure,
  contentOnly,
  onPick,
  onError,
}: {
  node: string;
  field: MatchField;
  scope: AnalysisScope;
  measure: CollocationMeasure;
  contentOnly: boolean;
  onPick: (lemma: string) => void;
  onError: (m: string) => void;
}) {
  const [result, setResult] = useState<NetworkResult | null>(null);
  const key = JSON.stringify({ node, field, scope, measure, contentOnly });

  useEffect(() => {
    let cancelled = false;
    setResult(null);
    api
      .collocationNetwork({
        collocation: { node, field, scope, left: 3, right: 3, minFreq: 2, contentOnly, measure, limit: 20 },
        size: 20,
        minPair: 2,
        perNode: 3,
      })
      .then((r) => !cancelled && setResult(r))
      .catch((e) => !cancelled && onError(message(e)));
    return () => {
      cancelled = true;
    };
    // key 就是这几个参数的内容
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, onError]);

  const layout = useMemo(() => {
    if (!result) return null;
    const W = 760;
    const H = 560;
    const cx = W / 2;
    const cy = H / 2;
    const n = result.nodes.length;
    const scores = result.nodes.map((x) => x.score);
    const hi = Math.max(...scores, 0);
    const lo = Math.min(...scores, 0);
    const maxFreq = Math.max(1, ...result.nodes.map((x) => x.freq));
    // 强的靠里：半径 150~250；角度按名次均匀铺开，强弱交错，免得一侧全是强的
    const pts = result.nodes.map((x, i) => {
      const t = hi > lo ? (hi - x.score) / (hi - lo) : 0.5;
      const r = 150 + t * 100;
      const slot = i % 2 === 0 ? i / 2 : n - 1 - (i - 1) / 2;
      const angle = (slot / Math.max(1, n)) * Math.PI * 2 - Math.PI / 2;
      return { x: cx + r * Math.cos(angle), y: cy + r * Math.sin(angle) * 0.82, size: 4 + 10 * Math.sqrt(x.freq / maxFreq) };
    });
    const maxEdge = Math.max(1, ...result.edges.map((e) => e.freq));
    return { W, H, cx, cy, pts, maxEdge };
  }, [result]);

  if (!result || !layout) return <p className="muted pad">{tr("计算中…")}</p>;
  if (result.nodes.length === 0) return <p className="muted pad">{tr("没有共现 2 次以上的词")}</p>;
  const { W, H, cx, cy, pts, maxEdge } = layout;
  return (
    <div className="kwic-explore network">
      <p className="muted small">
        {tr("「{node}」（{nodeFreq} 次）和它最强的 {n} 个搭配词：越靠中间越强，圆点大小是总频次；灰线是搭配词彼此之间常一起出现（左右 3 词内）。点一个词回到列表，只看附近有它的行。", {
          node: result.node,
          nodeFreq: result.nodeFreq,
          n: result.nodes.length,
        })}
      </p>
      <svg className="network-svg" viewBox={`0 0 ${W} ${H}`} role="img" aria-label={tr("搭配网络")}>
        {result.edges.map((e) => (
          <line
            key={`${e.a}-${e.b}`}
            className="net-edge"
            x1={pts[e.a]!.x}
            y1={pts[e.a]!.y}
            x2={pts[e.b]!.x}
            y2={pts[e.b]!.y}
            strokeWidth={1 + 3 * (e.freq / maxEdge)}
          >
            <title>
              {tr("{a} — {b}：共现 {freq} 次 · logDice {ld}", {
                a: result.nodes[e.a]!.lemma,
                b: result.nodes[e.b]!.lemma,
                freq: e.freq,
                ld: e.logDice.toFixed(2),
              })}
            </title>
          </line>
        ))}
        {result.nodes.map((_, i) => (
          <line key={`s${i}`} className="net-spoke" x1={cx} y1={cy} x2={pts[i]!.x} y2={pts[i]!.y} />
        ))}
        <circle className="net-center" cx={cx} cy={cy} r={22} />
        <text className="net-center-label" x={cx} y={cy} dominantBaseline="central" textAnchor="middle">
          {result.node}
        </text>
        {result.nodes.map((x, i) => (
          <g key={`${x.lemma}:${x.pos}`} className="net-node" onClick={() => onPick(x.lemma)} role="button" tabIndex={0}
            onKeyDown={(e) => e.key === "Enter" && onPick(x.lemma)}>
            <title>
              {tr("{lemma}（{pos}）· 全库 {freq} 次 · 和「{node}」共现 {cofreq} 次 · logDice {ld}", {
                lemma: x.lemma,
                pos: POS_LABELS[x.pos] ?? x.pos,
                freq: x.freq,
                node: result.node,
                cofreq: x.cofreq,
                ld: x.logDice.toFixed(2),
              })}
            </title>
            <circle cx={pts[i]!.x} cy={pts[i]!.y} r={pts[i]!.size} />
            <text x={pts[i]!.x} y={pts[i]!.y - pts[i]!.size - 5} textAnchor="middle">
              {x.lemma}
            </text>
          </g>
        ))}
      </svg>
    </div>
  );
}
