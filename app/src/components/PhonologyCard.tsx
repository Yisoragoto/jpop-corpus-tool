/**
 * 分析页「音系」：元音分布、单元音词和单元音行。
 *
 * 读音来自 Sudachi，按写法记（助词は记ハ、東京记トウキョウ）；Rust 里先改成按发音
 * （ワ、トーキョー）再拆拍，规则和与 UniDic 的对账见 `jp-corpus/src/phonology.rs`。
 * 读音里有假名以外的字的词（多是英文）拆不了，不算在里面，表上写出来有多少。
 */

import { useEffect, useState } from "react";

import { api, type StatsFilter, type Vowel, type VowelResult } from "../api";
import { toSubcorpus } from "../corpusQuality";
import { pyGrouped, tableCsv } from "../pyFormat";
import { ProvenanceLine } from "./CorpusCompare";
import { saveCsv } from "./saveCsv";
import { tr } from "../i18n";

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));
const scopeOf = (filter: StatsFilter) =>
  ({ subcorpus: toSubcorpus(filter), unit: filter.distinctLines ? "distinctLines" : "tokens", jpOnly: filter.jpOnly }) as const;

/** 元音按段显示：a → ア段 */
const ROW_LABEL: Record<Vowel, string> = { a: tr("ア段"), i: tr("イ段"), u: tr("ウ段"), e: tr("エ段"), o: tr("オ段") };
const KANA: Record<Vowel, string> = { a: "ア", i: "イ", u: "ウ", e: "エ", o: "オ" };
const MIN_MORAE = [2, 3, 4] as const;

const pct = (share: number) => `${(share * 100).toFixed(1)}%`;
/** 单元音词表先列这么多行，其余点「显示全部」 */
const PAGE_ROWS = 50;

export function PhonologyCard({
  filter,
  onSearchWord,
  onError,
}: {
  filter: StatsFilter;
  /** 点一个词：打开「词的去处」 */
  onSearchWord: (lemma: string) => void;
  onError: (message: string) => void;
}) {
  const [minMorae, setMinMorae] = useState<number>(2);
  const [vowel, setVowel] = useState<Vowel>("a");
  const [result, setResult] = useState<VowelResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");
  const filterKey = JSON.stringify(filter);
  // 记下展开时的筛选和段，一变就回到只列前几行
  const pageKey = JSON.stringify([filterKey, minMorae, vowel]);
  const [expandedFor, setExpandedFor] = useState<string | null>(null);
  const showAll = expandedFor === pageKey;

  useEffect(() => {
    let cancelled = false;
    setBusy(true);
    api
      .vowels({ scope: scopeOf(filter), minWordMorae: minMorae })
      .then((r) => !cancelled && setResult(r))
      .catch((e) => !cancelled && onError(message(e)))
      .finally(() => !cancelled && setBusy(false));
    return () => {
      cancelled = true;
    };
    // filterKey 就是 filter 的内容
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filterKey, minMorae, onError]);

  const group = result?.univocalic.find((g) => g.vowel === vowel) ?? null;

  const exportCsv = async () => {
    if (!result) return;
    const rows = result.univocalic.flatMap((g) =>
      g.words.map((w) => [g.vowel, w.kana, w.lemma, w.surfaces.map(([s, n]) => `${s}:${n}`).join(" "), w.morae, w.freq, w.songs]),
    );
    const csv = tableCsv(["vowel", "kana", "lemma", "surfaces", "morae", "freq", "songs"], rows);
    const path = await saveCsv(csv, "univocalic.csv", onError);
    if (path) setNote(tr("已保存：{path}", { path }));
  };

  return (
    <>
      <section className="card">
        <div className="card-head">
          <h2>{tr("元音分布")}</h2>
          <div className="card-tools">
            <span className="muted small">
              {result ? tr("{count} 拍", { count: pyGrouped(result.vowelMorae) }) : ""}
            </span>
          </div>
        </div>
        <p className="muted small">
          {tr("歌词按发音拆成拍（モーラ），数每拍的元音。助词は读ワ、エイ / オウ 读长音（エーエン、トーキョー），长音拍算前一拍的元音；促音、拨音没有元音，单独列。")}
        </p>
        {busy && !result && <p className="muted small pad">{tr("计算中…")}</p>}
        {result && result.vowelMorae > 0 && (
          <>
            <div className={`coarse-bar${busy ? " stale" : ""}`} title={result.vowels.map((v) => `${ROW_LABEL[v.vowel]} ${pct(v.share)}`).join(" · ")}>
              {result.vowels.map((v, i) => (
                <span key={v.vowel} className={`coarse-seg c${i}`} style={{ flexGrow: v.share }}>
                  {KANA[v.vowel]} <b>{pct(v.share)}</b>
                </span>
              ))}
            </div>
            <p className="muted small phono-moraic">
              {tr("促音 {sokuon} 拍 · 拨音 {hatsuon} 拍 · 长音 {long} 拍", {
                sokuon: pyGrouped(result.sokuon),
                hatsuon: pyGrouped(result.hatsuon),
                long: pyGrouped(result.long),
              })}
              {" · "}
              {tr("拆了 {words} 个词", { words: pyGrouped(result.words) })}
              {result.skipped > 0 && (
                <span title={result.skippedExamples.map(([s, n]) => `${s} ×${n}`).join("\n")}>
                  {"，"}
                  {tr("{skipped} 个没有假名读音（多是英文，如 {examples}），没算", {
                    skipped: pyGrouped(result.skipped),
                    examples: result.skippedExamples.slice(0, 3).map(([s]) => s).join("、"),
                  })}
                </span>
              )}
            </p>
          </>
        )}
        {result && <ProvenanceLine p={result.provenance} />}
      </section>

      <section className="card">
        <div className="card-head">
          <h2>{tr("单元音词")}</h2>
          <div className="card-tools">
            <label className="ctl-label">
              {tr("至少")}
              <select value={minMorae} onChange={(e) => setMinMorae(Number(e.target.value))}>
                {MIN_MORAE.map((n) => (
                  <option key={n} value={n}>
                    {tr("{n} 拍", { n })}
                  </option>
                ))}
              </select>
            </label>
            <button type="button" disabled={!result} onClick={() => void exportCsv()}>{tr("导出 CSV")}</button>
          </div>
        </div>
        <p className="muted small">
          {tr("整个词只有一个元音（あなた、永遠 エーエン）。促音、拨音不影响，拍数下限也只数带元音的拍。按段数一数，就能看出只用一个段写歌词时手里有多少词。点词看它出现在哪些歌里。")}
          {note && <span className="export-note"> {note}</span>}
        </p>
        {busy && !result && <p className="muted small pad">{tr("计算中…")}</p>}
        {result && (
          <div className="card-filters">
            <div className="segmented phono-rows" role="radiogroup" aria-label={tr("元音段")}>
              {result.univocalic.map((g) => (
                <button key={g.vowel} type="button" role="radio" aria-checked={vowel === g.vowel} className={vowel === g.vowel ? "on" : ""} onClick={() => setVowel(g.vowel)}>
                  {ROW_LABEL[g.vowel]} <span className="muted">{pyGrouped(g.distinctWords)}</span>
                </button>
              ))}
            </div>
          </div>
        )}
        {group && (
          <>
            <p className="muted small">
              {tr("{words} 个词，一共出现 {tokens} 次；整行只有这个元音的行 {lines} 行", {
                words: pyGrouped(group.distinctWords),
                tokens: pyGrouped(group.tokens),
                lines: pyGrouped(group.lineCount),
              })}
            </p>
            {group.words.length > 0 ? (
              <div className={`table-wrap${busy ? " stale" : ""}`}>
                <table className="data">
                  <thead>
                    <tr>
                      <th>{tr("发音")}</th>
                      <th className="wrap-col">{tr("写法")}</th>
                      <th className="num">{tr("拍")}</th>
                      <th className="num">{tr("次数")}</th>
                      <th className="num">{tr("出现曲数")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {(showAll ? group.words : group.words.slice(0, PAGE_ROWS)).map((w) => (
                      <tr key={`${w.kana}\u{1f}${w.lemma}`}>
                        <td className="jp">{w.kana}</td>
                        <td className="jp wrap-col">
                          <button type="button" className="link-btn" onClick={() => onSearchWord(w.lemma)} title={tr("「{lemma}」出现在哪些歌里", { lemma: w.lemma })}>
                            {w.surfaces.map(([s]) => s).join(" / ")}
                          </button>
                        </td>
                        <td className="num">{w.morae}</td>
                        <td className="num">{pyGrouped(w.freq)}</td>
                        <td className="num">{pyGrouped(w.songs)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {group.words.length > PAGE_ROWS && (
                  <p className="table-more">
                    <button type="button" className="link-btn" onClick={() => setExpandedFor(showAll ? null : pageKey)}>
                      {showAll ? tr("收起") : tr("显示全部 {n} 个", { n: pyGrouped(group.words.length) })}
                    </button>
                  </p>
                )}
              </div>
            ) : (
              <p className="muted small pad">{tr("没有")}</p>
            )}
            {group.lines.length > 0 && (
              <>
                <h3 className="phono-subhead">{tr("单元音行")}</h3>
                <ul className="phono-lines">
                  {group.lines.map((l) => (
                    <li key={l.utteranceId}>
                      <span className="jp">{l.text}</span>
                      <span className="muted small jp">{l.kana}</span>
                    </li>
                  ))}
                </ul>
              </>
            )}
          </>
        )}
      </section>
    </>
  );
}
