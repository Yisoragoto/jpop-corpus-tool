/**
 * 分析页的 N-gram（词簇）：连续 2~5 个词的组合按次数排，配上出现曲数和离散度。
 *
 * 计算在 Rust（`analysis::ngrams`）：不跨行、不跨标点。默认至少出现在两首歌里——
 * 只在一首歌里反复出现的是那首歌的副歌，不是惯用的说法。
 * 点「检索」把整个词组当多词表达（A+B+C）送到 KWIC，回到具体的句子。
 */

import { useEffect, useState } from "react";

import { api, POS_LABELS, type MatchField, type NgramResult, type StatsFilter } from "../api";
import { toSubcorpus } from "../corpusQuality";
import { pyGrouped, tableCsv } from "../pyFormat";
import { ProvenanceLine } from "./CorpusCompare";
import { saveCsv } from "./saveCsv";
import { tr } from "../i18n";

const SHOWN = 100;

interface Props {
  filter: StatsFilter;
  onSearchPhrase: (phrase: string, field: MatchField) => void;
  onError: (message: string) => void;
}

export function NgramCard({ filter, onSearchPhrase, onError }: Props) {
  const [n, setN] = useState(3);
  const [field, setField] = useState<MatchField>("lemma");
  const [minSongs, setMinSongs] = useState(2);
  // 输入框里是草稿，失焦或回车才提交：边打边夹会把「删掉 2 再打 3」变成 13，每一下还都要重算
  const [minDraft, setMinDraft] = useState("2");
  const [containsDraft, setContainsDraft] = useState("");
  const [contains, setContains] = useState("");
  const [result, setResult] = useState<NgramResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");
  const filterKey = JSON.stringify(filter);

  useEffect(() => {
    let cancelled = false;
    setBusy(true);
    api
      .ngrams({
        scope: { subcorpus: toSubcorpus(filter), unit: filter.distinctLines ? "distinctLines" : "tokens", jpOnly: filter.jpOnly },
        n,
        field,
        minFreq: 2,
        minSongs,
        contains: contains.trim() || null,
        limit: 2000,
      })
      .then((r) => !cancelled && setResult(r))
      .catch((e) => !cancelled && onError(e instanceof Error ? e.message : String(e)))
      .finally(() => !cancelled && setBusy(false));
    return () => {
      cancelled = true;
    };
    // filterKey 就是 filter 的内容（对象每次渲染都是新的）
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filterKey, n, field, minSongs, contains, onError]);

  const commitMinSongs = () => {
    const v = Math.min(999, Math.max(1, parseInt(minDraft, 10) || 1));
    setMinSongs(v);
    setMinDraft(String(v));
  };

  const exportCsv = async () => {
    if (!result) return;
    const csv = tableCsv(
      ["rank", "ngram", "surface", "pos", "freq", "per10k", "songs", "DP", "ARF"],
      result.items.map((g, i) => [i + 1, g.words.join(" "), g.surface, g.pos.join(" "), g.freq, g.per10k, g.songs, g.dispersion.dp, g.dispersion.arf]),
    );
    const path = await saveCsv(csv, `ngram_${n}.csv`, onError);
    if (path) setNote(tr("已保存：{path}", { path }));
  };

  return (
    <section className="card">
      <div className="card-head">
        <h2>{tr("N-gram（词簇）")}</h2>
        <div className="card-tools">
          <span className="muted small" title={result && !busy ? tr("左边是达到次数和曲数要求的（最多列 2,000 个），右边是范围里一共几种（填了「含某个词」就只算含它的）") : undefined}>
            {busy ? tr("计算中…") : result ? tr("{shown} / {total} 种", { shown: pyGrouped(result.items.length), total: pyGrouped(result.types) }) : ""}
          </span>
          <button disabled={!result?.items.length} onClick={() => void exportCsv()}>{tr("导出 CSV")}</button>
        </div>
      </div>
      <p className="muted small">
        {tr("连续几个词的固定组合，按次数排。不跨行、不跨标点；至少出现在几首歌里可调——只在一首歌里反复出现的多半是那首歌的副歌。点「检索」看具体是哪几句。")}
        {note && <span className="export-note"> {note}</span>}
      </p>
      <div className="card-filters">
        <div className="segmented" role="radiogroup" aria-label={tr("词数")}>
          {[2, 3, 4, 5].map((k) => (
            <button key={k} type="button" role="radio" aria-checked={n === k} className={n === k ? "on" : ""} onClick={() => setN(k)}>
              {tr("{k} 词", { k })}
            </button>
          ))}
        </div>
        <div className="segmented" role="radiogroup" aria-label={tr("按词元/表层")}>
          <button type="button" role="radio" aria-checked={field === "lemma"} className={field === "lemma" ? "on" : ""} onClick={() => setField("lemma")} title={tr("按辞书形组合，「消えた」「消えて」算同一个")}>{tr("词元")}</button>
          <button type="button" role="radio" aria-checked={field === "surface"} className={field === "surface" ? "on" : ""} onClick={() => setField("surface")} title={tr("按歌词里的实际写法组合")}>{tr("表层")}</button>
        </div>
        <label className="ctl-label">
          {tr("至少")}
          <input
            className="min-songs"
            type="number"
            min={1}
            max={999}
            aria-label={tr("至少出现在几首歌里")}
            value={minDraft}
            onChange={(e) => setMinDraft(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && commitMinSongs()}
            onBlur={commitMinSongs}
          />
          {tr("首歌")}
        </label>
        <input
          type="search"
          className="ngram-contains"
          aria-label={tr("只看含某个词的词组")}
          placeholder={tr("含某个词…")}
          value={containsDraft}
          onChange={(e) => {
            setContainsDraft(e.target.value);
            // 点框里自带的 × 清空时立即生效
            if (e.target.value === "") setContains("");
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") setContains(containsDraft);
            else if (e.key === "Escape") {
              setContainsDraft("");
              setContains("");
            }
          }}
          onBlur={() => setContains(containsDraft)}
        />
      </div>
      {result && result.items.length > 0 && (
        <div className={`table-wrap${busy ? " stale" : ""}`}>
          <table className="data">
            <thead>
              <tr>
                <th className="num">#</th>
                <th>{tr("词组")}</th>
                <th className="wrap-col">{tr("最常见的写法")}</th>
                <th className="num">{tr("次数")}</th>
                <th className="num" title={tr("每万词")}>{tr("每万词")}</th>
                <th className="num">{tr("出现曲数")}</th>
                <th className="num" title={tr("Gries 的 DP：0 = 按歌的大小均匀分布，1 = 全集中在一首里")}>DP</th>
                <th><span className="sr-only">{tr("操作")}</span></th>
              </tr>
            </thead>
            <tbody>
              {result.items.slice(0, SHOWN).map((g, i) => (
                <tr key={g.words.join("\u{1f}")}>
                  <td className="num muted">{i + 1}</td>
                  <td title={g.pos.map((p) => POS_LABELS[p] ?? p).join(" + ")}>
                    {g.words.map((w, k) => (
                      <span key={k} className="ngram-word">
                        {w}
                      </span>
                    ))}
                  </td>
                  <td className="muted wrap-col">{g.surface}</td>
                  <td className="num">{pyGrouped(g.freq)}</td>
                  <td className="num">{g.per10k.toFixed(1)}</td>
                  <td className="num">{pyGrouped(g.songs)}</td>
                  <td className="num">
                    <span className="dp-cell" style={{ ["--dp" as string]: `${g.dispersion.dp * 100}%` }}>{g.dispersion.dp.toFixed(2)}</span>
                  </td>
                  <td className="row-actions">
                    <button className="link-btn" aria-label={tr("检索 {p}", { p: g.words.join(" ") })} onClick={() => onSearchPhrase(g.words.join("+"), field)}>{tr("检索")}</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {result && result.items.length === 0 && !busy && <p className="muted">{tr("没有符合条件的词组")}</p>}
      {result && result.items.length > SHOWN && (
        <p className="muted small">{tr("只显示前 {shown} 个；导出 CSV 有全部 {total} 个", { shown: pyGrouped(SHOWN), total: pyGrouped(result.items.length) })}</p>
      )}
      {result && <ProvenanceLine p={result.provenance} />}
    </section>
  );
}
