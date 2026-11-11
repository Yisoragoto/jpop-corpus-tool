/**
 * 分析页「音系」下半：押韵检索、相邻行押韵。
 *
 * 日语歌词押韵看元音：歌词先按发音拆成拍，每拍只留元音（长音拍算前一拍的元音），
 * ン、ッ 默认跳过。读音拆不了的词（英文）是断点，韵不跨过它。定义和基线的算法见
 * `jp-corpus/src/phonology.rs` 的「押韵」一节。
 */

import { useEffect, useState } from "react";

import { api, type RhymePosition, type RhymeResult, type RhymeStatsResult, type StatsFilter } from "../api";
import { toSubcorpus } from "../corpusQuality";
import { ProvenanceLine } from "./CorpusCompare";
import { tr } from "../i18n";

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));
const scopeOf = (filter: StatsFilter) =>
  ({ subcorpus: toSubcorpus(filter), unit: filter.distinctLines ? "distinctLines" : "tokens", jpOnly: filter.jpOnly }) as const;
const pct = (share: number) => `${(share * 100).toFixed(1)}%`;
const TAILS = [2, 3, 4] as const;

/** 解析后的模式 aiu → a·i·u；n、q 写成 ン、ッ 好认；两种读法都算的 (ou) 写成 o/u */
function showPattern(p: string) {
  const one = (c: string) => (c === "n" ? "ン" : c === "q" ? "ッ" : c === "." ? "＿" : c);
  return (p.match(/\([a-z]+\)|./g) ?? []).map((t) => (t.startsWith("(") ? Array.from(t.slice(1, -1), one).join("/") : one(t))).join("·");
}

/** 按码点下标切出高亮的那一段 */
function Highlighted({ text, start, end }: { text: string; start: number | null; end: number | null }) {
  if (start === null || end === null) return <>{text}</>;
  const chars = Array.from(text);
  return (
    <>
      {chars.slice(0, start).join("")}
      <mark>{chars.slice(start, end).join("")}</mark>
      {chars.slice(end).join("")}
    </>
  );
}

export function RhymeCard({
  filter,
  onOpenLine,
  onError,
}: {
  filter: StatsFilter;
  /** 点一行：打开这首歌、滚到这一行 */
  onOpenLine: (songId: string, utteranceId: number | null) => void;
  onError: (message: string) => void;
}) {
  const filterKey = JSON.stringify(filter);

  // ── 检索 ──
  const [draft, setDraft] = useState("");
  const [pattern, setPattern] = useState("");
  const [position, setPosition] = useState<RhymePosition>("end");
  const [strict, setStrict] = useState(false);
  const [result, setResult] = useState<RhymeResult | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!pattern.trim()) {
      setResult(null);
      return;
    }
    let cancelled = false;
    setBusy(true);
    api
      .rhyme({ scope: scopeOf(filter), pattern, position, strict })
      .then((r) => !cancelled && setResult(r))
      .catch((e) => !cancelled && onError(message(e)))
      .finally(() => !cancelled && setBusy(false));
    return () => {
      cancelled = true;
    };
    // filterKey 就是 filter 的内容
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filterKey, pattern, position, strict, onError]);

  // ── 相邻行押韵 ──
  const [statsSide, setStatsSide] = useState<RhymePosition>("end");
  const [tail, setTail] = useState<number>(3);
  const [statsStrict, setStatsStrict] = useState(false);
  const [stats, setStats] = useState<RhymeStatsResult | null>(null);
  const [statsBusy, setStatsBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setStatsBusy(true);
    api
      .rhymeStats({ scope: scopeOf(filter), position: statsSide, strict: statsStrict, tail })
      .then((r) => !cancelled && setStats(r))
      .catch((e) => !cancelled && onError(message(e)))
      .finally(() => !cancelled && setStatsBusy(false));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filterKey, statsSide, statsStrict, tail, onError]);

  const atTail = stats?.lengths.find((l) => l.length === tail) ?? null;

  return (
    <>
      <section className="card">
        <div className="card-head">
          <h2>{tr("押韵检索")}</h2>
          <div className="card-tools">
            <span className="muted small">
              {result && result.pattern ? tr("{lines} 行 · {songs} 首", { lines: result.lines.toLocaleString(), songs: result.songs.toLocaleString() }) : ""}
            </span>
          </div>
        </div>
        <p className="muted small">
          {tr("按元音找行：输入元音（a i u e o，n 是ン、q 是ッ，. 是任意一个音）或假名，假名按发音转（えいえん 就是 e-e-e）。默认只比元音，ン、ッ 跳过；英文词是断点。")}
        </p>
        <div className="rhyme-controls">
          <input
            type="search"
            className="search rhyme-input"
            placeholder={tr("如 aiu、ないよ、kimi（回车）")}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && setPattern(draft)}
            onBlur={() => setPattern(draft)}
          />
          <div className="segmented" role="tablist">
            {(
              [
                ["end", tr("行尾（脚韵）")],
                ["start", tr("行首（头韵）")],
                ["any", tr("任意位置")],
              ] as const
            ).map(([k, label]) => (
              <button key={k} role="tab" aria-selected={position === k} className={position === k ? "on" : ""} onClick={() => setPosition(k)}>
                {label}
              </button>
            ))}
          </div>
          <label className="check">
            <input type="checkbox" checked={strict} onChange={(e) => setStrict(e.target.checked)} />
            {tr("ン、ッ 也要对上")}
          </label>
        </div>
        {result && !result.pattern && <p className="muted small pad">{tr("输入里没有能比的元音")}</p>}
        {result && result.pattern && (
          <>
            <p className="muted small">
              {tr("模式：{pattern}", { pattern: showPattern(result.pattern) })}
            </p>
            {result.carriers.length > 0 && (
              <div className="rhyme-carriers">
                {result.carriers.slice(0, 24).map((c) => (
                  <span key={c.text} className="chip jp" title={tr("{lines} 行，{songs} 首", { lines: c.lines, songs: c.songs })}>
                    {c.text} <span className="muted">{c.lines}</span>
                  </span>
                ))}
              </div>
            )}
            {result.hits.length > 0 ? (
              <ul className={`rhyme-hits${busy ? " stale" : ""}`}>
                {result.hits.map((h) => (
                  <li key={h.utteranceId}>
                    <button className="link-btn rhyme-line jp" onClick={() => onOpenLine(h.songId, h.utteranceId)} title={tr("打开这首歌，滚到这一行")}>
                      <Highlighted text={h.text} start={h.charStart} end={h.charEnd} />
                    </button>
                    <span className="muted small jp">{h.kana}</span>
                    <span className="muted small rhyme-song">
                      {h.title}
                      {h.artist ? ` — ${h.artist}` : ""}
                    </span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="muted small pad">{tr("没有")}</p>
            )}
            {result.lines > result.hits.length && (
              <p className="muted small">{tr("只列前 {n} 行", { n: result.hits.length })}</p>
            )}
            <ProvenanceLine p={result.provenance} />
          </>
        )}
      </section>

      <section className="card">
        <div className="card-head">
          <h2>{tr("相邻行押韵")}</h2>
          <div className="card-tools">
            <div className="segmented small-seg">
              <button className={statsSide === "end" ? "on" : ""} onClick={() => setStatsSide("end")}>{tr("行尾")}</button>
              <button className={statsSide === "start" ? "on" : ""} onClick={() => setStatsSide("start")}>{tr("行首")}</button>
            </div>
            <label className="muted small">
              {tr("韵脚")}{" "}
              <select value={tail} onChange={(e) => setTail(Number(e.target.value))}>
                {TAILS.map((n) => (
                  <option key={n} value={n}>
                    {tr("{n} 个音", { n })}
                  </option>
                ))}
              </select>
            </label>
            <label className="check small">
              <input type="checkbox" checked={statsStrict} onChange={(e) => setStatsStrict(e.target.checked)} />
              {tr("含ン、ッ")}
            </label>
          </div>
        </div>
        <p className="muted small">
          {tr("同一首歌里挨着的两行，结尾（或开头）几个元音一样就算押韵；两行文本相同的不算。和同一首歌里随便挑两行比：倍数大于 1，说明挨着的行押韵比碰巧多。")}
        </p>
        {stats && (
          <div className={`table-wrap${statsBusy ? " stale" : ""}`}>
            <table className="data">
              <thead>
                <tr>
                  <th className="num">{tr("音数")}</th>
                  <th className="num">{tr("相邻行")}</th>
                  <th className="num">{tr("押韵")}</th>
                  <th className="num">{tr("随便两行")}</th>
                  <th className="num">{tr("倍数")}</th>
                </tr>
              </thead>
              <tbody>
                {stats.lengths.map((l) => (
                  <tr key={l.length} className={l.length === tail ? "on" : ""}>
                    <td className="num">{l.length}</td>
                    <td className="num">{l.adjacent.toLocaleString()}</td>
                    <td className="num">
                      {pct(l.adjacentRate)} <span className="muted small">({l.adjacentRhymed.toLocaleString()})</span>
                    </td>
                    <td className="num">{pct(l.baselineRate)}</td>
                    <td className="num">{l.lift === null ? "—" : `×${l.lift.toFixed(2)}`}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        {stats && atTail && atTail.adjacentRhymed > 0 && (
          <p className="muted small">
            {tr("押 {n} 个音的 {pairs} 对里，{same} 对是同一个词收尾（如都以「ないで」结尾），算重复不算押韵的话要扣掉。", {
              n: tail,
              pairs: atTail.adjacentRhymed.toLocaleString(),
              same: stats.sameWord.toLocaleString(),
            })}
          </p>
        )}
        {stats && stats.tails.length > 0 && (
          <>
            <h3 className="phono-subhead">{tr("常见韵脚")}</h3>
            <div className={`table-wrap${statsBusy ? " stale" : ""}`}>
              <table className="data">
                <thead>
                  <tr>
                    <th>{tr("韵脚")}</th>
                    <th className="num">{tr("对数")}</th>
                    <th className="num">{tr("出现曲数")}</th>
                    <th className="num">{tr("同词")}</th>
                    <th>{tr("例子")}</th>
                  </tr>
                </thead>
                <tbody>
                  {stats.tails.slice(0, 20).map((t) => (
                    <tr key={t.tail}>
                      <td>
                        <button
                          className="link-btn"
                          title={tr("检索押这个韵的行")}
                          onClick={() => {
                            setDraft(t.tail);
                            setPattern(t.tail);
                            setPosition(statsSide === "start" ? "start" : "end");
                            setStrict(statsStrict);
                          }}
                        >
                          {showPattern(t.tail)}
                        </button>
                      </td>
                      <td className="num">{t.pairs.toLocaleString()}</td>
                      <td className="num">{t.songs.toLocaleString()}</td>
                      <td className="num">{t.sameWord.toLocaleString()}</td>
                      <td className="jp small">
                        <button className="link-btn rhyme-example" onClick={() => onOpenLine(t.example.songId, null)}>
                          {t.example.first} / {t.example.second}
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </>
        )}
        {stats && stats.songs.length > 0 && (
          <>
            <h3 className="phono-subhead">{tr("押韵最密的歌")}</h3>
            <div className={`table-wrap${statsBusy ? " stale" : ""}`}>
              <table className="data">
                <thead>
                  <tr>
                    <th>{tr("歌曲")}</th>
                    <th className="num">{tr("相邻行")}</th>
                    <th className="num">{tr("押韵")}</th>
                    <th className="num">{tr("比例")}</th>
                  </tr>
                </thead>
                <tbody>
                  {stats.songs.slice(0, 20).map((s) => (
                    <tr key={s.songId}>
                      <td>
                        <button className="link-btn" onClick={() => onOpenLine(s.songId, null)}>
                          {s.title}
                        </button>{" "}
                        <span className="muted small">{s.artist}</span>
                      </td>
                      <td className="num">{s.pairs.toLocaleString()}</td>
                      <td className="num">{s.rhymed.toLocaleString()}</td>
                      <td className="num">{pct(s.rate)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </>
        )}
        {stats && <ProvenanceLine p={stats.provenance} />}
      </section>
    </>
  );
}
