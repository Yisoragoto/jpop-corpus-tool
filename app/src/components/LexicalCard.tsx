/**
 * 分析页的「词汇丰富度与文字」：整个范围、每位演唱者、每首歌各一行，放在一起比。
 *
 * TTR 随文本变长单调下降，一首歌和一位歌手的全部作品没法用它比，所以并排给
 * MATTR（窗口 50）、MTLD、Yule's K。文字种是日语歌词特有的一层风格：
 * 同一个「君」写成汉字、平假名还是片假名，英文混了多少。
 * 计算在 Rust（`jp-corpus/src/lexical.rs`），定义也写在那里。
 */

import { useEffect, useMemo, useState } from "react";

import { api, type LexicalGroup, type LexicalResult, type LexicalRow, type StatsFilter } from "../api";
import { toSubcorpus } from "../corpusQuality";
import { tableCsv } from "../pyFormat";
import { ProvenanceLine } from "./CorpusCompare";
import { saveCsv } from "./saveCsv";
import { tr } from "../i18n";

const SCRIPTS = [
  { key: "kanji", label: tr("汉字") },
  { key: "hiragana", label: tr("平假名") },
  { key: "katakana", label: tr("片假名") },
  { key: "latin", label: tr("拉丁字母") },
  { key: "other", label: tr("其他") },
] as const;

type Col = "tokens" | "songs" | "ttr" | "mattr" | "mtld" | "yuleK" | "kanji" | "katakana" | "latin";

const value = (r: LexicalRow, col: Col): number => {
  switch (col) {
    case "kanji":
    case "katakana":
    case "latin":
      return r.scripts[col];
    case "mattr":
    case "mtld":
      return r[col] ?? Number.NEGATIVE_INFINITY;
    default:
      return r[col];
  }
};

const fixed = (x: number | null, digits: number) => (x === null ? "—" : x.toFixed(digits));

interface Props {
  filter: StatsFilter;
  onError: (message: string) => void;
}

export function LexicalCard({ filter, onError }: Props) {
  const [group, setGroup] = useState<LexicalGroup>("performer");
  const [result, setResult] = useState<LexicalResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [sort, setSort] = useState<{ col: Col; up: boolean }>({ col: "tokens", up: false });
  const [note, setNote] = useState("");
  const filterKey = JSON.stringify(filter);
  const unit = filter.distinctLines ? "distinctLines" : "tokens";

  useEffect(() => {
    let cancelled = false;
    setBusy(true);
    api
      .lexicalProfile({
        scope: { subcorpus: toSubcorpus(filter), unit, jpOnly: filter.jpOnly },
        group,
        window: 50,
        minTokens: group === "whole" ? 1 : 100,
        limit: 1000,
      })
      .then((r) => !cancelled && setResult(r))
      .catch((e) => !cancelled && onError(e instanceof Error ? e.message : String(e)))
      .finally(() => !cancelled && setBusy(false));
    return () => {
      cancelled = true;
    };
    // filterKey 就是 filter 的内容
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filterKey, group, onError]);

  const rows = useMemo(() => {
    if (!result) return [];
    const sign = sort.up ? 1 : -1;
    return [...result.rows].sort((a, b) => sign * (value(a, sort.col) - value(b, sort.col)) || b.tokens - a.tokens);
  }, [result, sort]);

  const head = (col: Col, label: string, title?: string) => (
    <th
      className="num sortable"
      title={title}
      onClick={() => setSort((s) => (s.col === col ? { col, up: !s.up } : { col, up: col === "yuleK" }))}
    >
      {label}
      {sort.col === col ? (sort.up ? " ↑" : " ↓") : ""}
    </th>
  );

  const exportCsv = async () => {
    const csv = tableCsv(
      ["name", "songs", "tokens", "types", "TTR", "MATTR50", "MTLD", "YuleK", "hapaxRatio", "kanji", "hiragana", "katakana", "latin", "other"],
      rows.map((r) => [
        r.label || tr("整个范围"),
        r.songs,
        r.tokens,
        r.types,
        r.ttr,
        r.mattr,
        r.mtld,
        r.yuleK,
        r.hapaxRatio,
        r.scripts.kanji,
        r.scripts.hiragana,
        r.scripts.katakana,
        r.scripts.latin,
        r.scripts.other,
      ]),
    );
    const path = await saveCsv(csv, `lexical_${group}.csv`, onError);
    if (path) setNote(tr("已保存：{path}", { path }));
  };

  return (
    <section className="card">
      <div className="card-head">
        <h2>{tr("词汇丰富度与文字")}</h2>
        <div className="card-tools">
          <div className="segmented">
            <button className={group === "whole" ? "on" : ""} onClick={() => setGroup("whole")}>{tr("整体")}</button>
            <button className={group === "performer" ? "on" : ""} onClick={() => setGroup("performer")}>{tr("按演唱者")}</button>
            <button className={group === "song" ? "on" : ""} onClick={() => setGroup("song")}>{tr("按歌")}</button>
          </div>
          <button className="ghost" disabled={!rows.length} onClick={() => void exportCsv()}>{tr("导出 CSV")}</button>
        </div>
      </div>
      <p className="muted small">
        {tr("TTR 会随文本变长一路下降，不同长度的文本没法比；MATTR（50 词窗口的平均 TTR）和 MTLD 基本不受长度影响，越大越丰富；Yule's K 越大越重复。都按词元算。词次不到 100 的演唱者、歌不列出。")}
        {unit === "tokens" && " " + tr("现在按全部词次算，副歌唱几遍算几遍；勾上筛选里的「副歌只算一遍」更接近「写了什么」。")}
        {note && <span className="export-note"> {note}</span>}
      </p>
      <div className="script-legend small muted">
        {SCRIPTS.map((s) => (
          <span key={s.key}>
            <i className={`script-swatch s-${s.key}`} /> {s.label}
          </span>
        ))}
      </div>
      {rows.length > 0 && (
        <div className={`table-wrap${busy ? " stale" : ""}`}>
          <table className="data">
            <thead>
              <tr>
                <th>{group === "song" ? tr("歌") : group === "performer" ? tr("演唱者") : ""}</th>
                {head("songs", tr("曲数"))}
                {head("tokens", tr("词次"))}
                {head("ttr", "TTR")}
                {head("mattr", "MATTR", tr("50 词窗口的移动平均 TTR"))}
                {head("mtld", "MTLD", tr("TTR 降到 0.72 之前平均能走多少词，正反两遍取平均"))}
                {head("yuleK", "Yule K", tr("越大越重复"))}
                {head("kanji", tr("汉字"))}
                {head("katakana", tr("片假名"))}
                {head("latin", tr("拉丁"))}
                <th className="script-col">{tr("文字构成")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.key || "whole"}>
                  <td className="lexical-name" title={r.label}>
                    {r.label || tr("整个范围")}
                  </td>
                  <td className="num">{r.songs}</td>
                  <td className="num">{r.tokens.toLocaleString()}</td>
                  <td className="num">{r.ttr.toFixed(3)}</td>
                  <td className="num">{fixed(r.mattr, 3)}</td>
                  <td className="num">{fixed(r.mtld, 1)}</td>
                  <td className="num">{r.yuleK.toFixed(1)}</td>
                  <td className="num">{(r.scripts.kanji * 100).toFixed(0)}%</td>
                  <td className="num">{(r.scripts.katakana * 100).toFixed(0)}%</td>
                  <td className="num">{(r.scripts.latin * 100).toFixed(0)}%</td>
                  <td className="script-col">
                    <span
                      className="script-bar"
                      title={SCRIPTS.map((s) => `${s.label} ${(r.scripts[s.key] * 100).toFixed(1)}%`).join(" · ")}
                    >
                      {SCRIPTS.map((s) => (
                        <i key={s.key} className={`s-${s.key}`} style={{ width: `${r.scripts[s.key] * 100}%` }} />
                      ))}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {result && rows.length === 0 && !busy && <p className="muted">{tr("没有词次够 100 的组")}</p>}
      {result && <ProvenanceLine p={result.provenance} />}
    </section>
  );
}
