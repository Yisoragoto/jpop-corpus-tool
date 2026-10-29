/**
 * 分析页「活用·写法」：用 tokens 的形态信息（Sudachi 的活用形、读音、正规化表记）做的两张表。
 *
 * - 活用形：有活用的词按活用形数（命令形、意志推量形……）。点一个例词，带着这个活用形去 KWIC；
 * - 表记变体：同一个词在歌词里的不同写法（君 / きみ / キミ）。默认只列汉字和假名混用的，
 *   那才是作词时的写法选择；只差送假名的（会う / 逢う 也算）可以打开「全部」看。
 *
 * 计算和规则在 Rust（`jp-corpus/src/morphology.rs`）。形态信息是升级后在后台补的，
 * 补完之前表上会提示还有多少词没标。
 */

import { useEffect, useMemo, useState } from "react";

import {
  api,
  POS_LABELS,
  type InflectionResult,
  type MatchField,
  type OrthographyResult,
  type StatsFilter,
  type Writing,
} from "../api";
import { toSubcorpus } from "../corpusQuality";
import { tableCsv } from "../pyFormat";
import { ProvenanceLine } from "./CorpusCompare";
import { saveCsv } from "./saveCsv";
import { tr } from "../i18n";

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));
const scopeOf = (filter: StatsFilter) =>
  ({ subcorpus: toSubcorpus(filter), unit: filter.distinctLines ? "distinctLines" : "tokens", jpOnly: filter.jpOnly }) as const;

function Pending({ count }: { count: number }) {
  if (count <= 0) return null;
  return (
    <p className="morph-pending small">
      {tr("还有 {count} 个词的形态信息在后台补，补完之前这张表不全。", { count: count.toLocaleString() })}
    </p>
  );
}

// ────────────────────────────── 活用形 ──────────────────────────────

const INFLECTING_POS = ["", "VERB", "ADJ", "AUX"] as const;

export function InflectionCard({
  filter,
  onSearchInflected,
  onError,
}: {
  filter: StatsFilter;
  /** 带着活用形去 KWIC 查一个词（按词元） */
  onSearchInflected: (lemma: string, cform: string) => void;
  onError: (message: string) => void;
}) {
  const [pos, setPos] = useState<string>("VERB");
  const [result, setResult] = useState<InflectionResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");
  const filterKey = JSON.stringify(filter);

  useEffect(() => {
    let cancelled = false;
    setBusy(true);
    api
      .inflection({ scope: scopeOf(filter), pos: pos || null, examples: 8 })
      .then((r) => !cancelled && setResult(r))
      .catch((e) => !cancelled && onError(message(e)))
      .finally(() => !cancelled && setBusy(false));
    return () => {
      cancelled = true;
    };
    // filterKey 就是 filter 的内容
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filterKey, pos, onError]);

  /** 大类合计（連用形 = 連用形-一般 + 連用形-促音便 + …） */
  const coarse = useMemo(() => {
    const sums = new Map<string, number>();
    for (const f of result?.forms ?? []) sums.set(f.coarse, (sums.get(f.coarse) ?? 0) + f.share);
    return [...sums.entries()].sort((a, b) => b[1] - a[1]);
  }, [result]);
  const maxShare = result?.forms[0]?.share ?? 1;

  const exportCsv = async () => {
    if (!result) return;
    const csv = tableCsv(
      ["cform", "coarse", "freq", "share", "songs", "lemmas"],
      result.forms.map((f) => [f.cform, f.coarse, f.freq, f.share, f.songs, f.lemmas.map(([l, n]) => `${l}:${n}`).join(" ")]),
    );
    const path = await saveCsv(csv, "inflection.csv", onError);
    if (path) setNote(tr("已保存：{path}", { path }));
  };

  return (
    <section className="card">
      <div className="card-head">
        <h2>{tr("活用形")}</h2>
        <div className="card-tools">
          <span className="muted small">{result ? tr("{total} 个有活用的词", { total: result.total.toLocaleString() }) : ""}</span>
          <div className="segmented small-seg">
            {INFLECTING_POS.map((p) => (
              <button key={p} className={pos === p ? "on" : ""} onClick={() => setPos(p)}>
                {p === "" ? tr("全部") : (POS_LABELS[p] ?? p)}
              </button>
            ))}
          </div>
          <button className="ghost" disabled={!result?.forms.length} onClick={() => void exportCsv()}>{tr("导出 CSV")}</button>
        </div>
      </div>
      <p className="muted small">
        {tr("动词、形容词、助动词按活用形数。歌词里命令形、意志形往往比书面语多。点例词带着这个活用形去检索。活用形的名字照 UniDic（Sudachi）的写法。")}
        {note && <span className="export-note"> {note}</span>}
      </p>
      {result && <Pending count={result.unannotated} />}
      {coarse.length > 0 && (
        <div className="coarse-bar" title={coarse.map(([c, s]) => `${c} ${(s * 100).toFixed(1)}%`).join(" · ")}>
          {coarse.slice(0, 8).map(([c, s], i) => (
            <span key={c} className={`coarse-seg c${i}`} style={{ flexGrow: s }}>
              {s > 0.06 && (
                <>
                  {c} <b>{(s * 100).toFixed(0)}%</b>
                </>
              )}
            </span>
          ))}
        </div>
      )}
      {result && result.forms.length > 0 && (
        <div className={`table-wrap${busy ? " stale" : ""}`}>
          <table className="data">
            <thead>
              <tr>
                <th>{tr("活用形")}</th>
                <th className="num">{tr("次数")}</th>
                <th className="bar-col" />
                <th className="num">{tr("比例")}</th>
                <th className="num">{tr("出现曲数")}</th>
                <th>{tr("常见的词")}</th>
              </tr>
            </thead>
            <tbody>
              {result.forms.map((f) => (
                <tr key={f.cform}>
                  <td className="cform-name">{f.cform}</td>
                  <td className="num">{f.freq.toLocaleString()}</td>
                  <td className="bar-col">
                    <span className="freq-bar">
                      <span style={{ width: `${(f.share / maxShare) * 100}%` }} />
                    </span>
                  </td>
                  <td className="num">{(f.share * 100).toFixed(1)}%</td>
                  <td className="num">{f.songs}</td>
                  <td className="chip-cell">
                    {f.lemmas.map(([lemma, n]) => (
                      <button
                        key={lemma}
                        className="chip link"
                        onClick={() => onSearchInflected(lemma, f.cform)}
                        title={tr("检索「{lemma}」的{cform}", { lemma, cform: f.cform })}
                      >
                        {lemma} <span className="muted">{n}</span>
                      </button>
                    ))}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {result && result.forms.length === 0 && !busy && <p className="muted">{tr("没有数据")}</p>}
      {result && <ProvenanceLine p={result.provenance} />}
    </section>
  );
}

// ────────────────────────────── 表记变体 ──────────────────────────────

const WRITING_LABELS: Record<Writing, string> = {
  kanji: tr("汉字"),
  hiragana: tr("平假名"),
  katakana: tr("片假名"),
  kana: tr("假名混写"),
  other: tr("其他"),
};

export function OrthographyCard({
  filter,
  onSearch,
  onError,
}: {
  filter: StatsFilter;
  /** 去 KWIC 查（按正规化表记查整组，或按词元查一种写法） */
  onSearch: (word: string, field: MatchField) => void;
  onError: (message: string) => void;
}) {
  const [result, setResult] = useState<OrthographyResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [mixedOnly, setMixedOnly] = useState(true);
  const [note, setNote] = useState("");
  const filterKey = JSON.stringify(filter);

  useEffect(() => {
    let cancelled = false;
    setBusy(true);
    api
      .orthography({ scope: scopeOf(filter), minFreq: 3, minFormFreq: 1, contentOnly: true, limit: 2000 })
      .then((r) => !cancelled && setResult(r))
      .catch((e) => !cancelled && onError(message(e)))
      .finally(() => !cancelled && setBusy(false));
    return () => {
      cancelled = true;
    };
    // filterKey 就是 filter 的内容
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filterKey, onError]);

  const groups = useMemo(
    () => (result?.groups ?? []).filter((g) => !mixedOnly || new Set(g.forms.map((f) => f.writing)).size >= 2),
    [result, mixedOnly],
  );

  const exportCsv = async () => {
    const csv = tableCsv(
      ["normalized", "reading", "pos", "freq", "forms"],
      groups.map((g) => [g.normalized, g.reading, g.pos, g.freq, g.forms.map((f) => `${f.lemma}:${f.freq}:${f.writing}`).join(" ")]),
    );
    const path = await saveCsv(csv, "orthography.csv", onError);
    if (path) setNote(tr("已保存：{path}", { path }));
  };

  return (
    <section className="card">
      <div className="card-head">
        <h2>{tr("表记变体")}</h2>
        <div className="card-tools">
          <span className="muted small">{result ? tr("{shown} / {total} 组", { shown: groups.length, total: result.totalGroups }) : ""}</span>
          <div className="segmented small-seg">
            <button className={mixedOnly ? "on" : ""} onClick={() => setMixedOnly(true)} title={tr("同一个词既写成汉字又写成假名（君 / きみ / キミ）")}>{tr("汉字与假名混用")}</button>
            <button className={!mixedOnly ? "on" : ""} onClick={() => setMixedOnly(false)} title={tr("也列出只差用字的（会う / 逢う）")}>{tr("全部")}</button>
          </div>
          <button className="ghost" disabled={!groups.length} onClick={() => void exportCsv()}>{tr("导出 CSV")}</button>
        </div>
      </div>
      <p className="muted small">
        {tr("同一个词在歌词里的不同写法，按 Sudachi 的正规化表记归组；全是假名的写法按读音并到同词性的汉字写法（只对不活用的词）。只看实词。点写法看是哪几句。")}
        {note && <span className="export-note"> {note}</span>}
      </p>
      {result && <Pending count={result.unannotated} />}
      <div className="script-legend small muted">
        {(["kanji", "hiragana", "katakana", "kana", "other"] as Writing[]).map((w) => (
          <span key={w}>
            <i className={`script-swatch w-${w}`} /> {WRITING_LABELS[w]}
          </span>
        ))}
      </div>
      {groups.length > 0 && (
        <div className={`table-wrap${busy ? " stale" : ""}`}>
          <table className="data">
            <thead>
              <tr>
                <th>{tr("词")}</th>
                <th>{tr("读音")}</th>
                <th>{tr("词性")}</th>
                <th className="num">{tr("合计")}</th>
                <th className="variant-col">{tr("写法（次数）")}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {groups.map((g) => (
                <tr key={`${g.normalized}:${g.pos}`}>
                  <td className="variant-head">{g.normalized}</td>
                  <td className="muted">{g.reading}</td>
                  <td className="muted">{POS_LABELS[g.pos] ?? g.pos}</td>
                  <td className="num">{g.freq.toLocaleString()}</td>
                  <td className="variant-col">
                    <span className="variant-bar" title={g.forms.map((f) => `${f.lemma} ${f.freq}`).join(" · ")}>
                      {g.forms.map((f) => (
                        <i key={f.lemma} className={`w-${f.writing}`} style={{ flexGrow: f.freq }} />
                      ))}
                    </span>
                    <span className="variant-forms">
                      {g.forms.map((f) => (
                        <button
                          key={f.lemma}
                          className={`variant-form w-${f.writing}-text`}
                          onClick={() => onSearch(f.lemma, "lemma")}
                          title={tr("{writing} · {songs} 首歌 · 检索这种写法", { writing: WRITING_LABELS[f.writing], songs: f.songs })}
                        >
                          {f.lemma}
                          <span className="muted">{f.freq}</span>
                        </button>
                      ))}
                    </span>
                  </td>
                  <td className="row-actions">
                    <button
                      className="link-btn"
                      onClick={() => onSearch(g.mergedByReading ? g.reading : g.normalized, g.mergedByReading ? "reading" : "normalized")}
                      title={g.mergedByReading ? tr("按读音检索，各种写法一起看") : tr("按正规化表记检索，各种写法一起看")}
                    >
                      {tr("检索")}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {result && groups.length === 0 && !busy && <p className="muted">{tr("没有符合条件的词")}</p>}
      {result && <ProvenanceLine p={result.provenance} />}
    </section>
  );
}
