/**
 * Analytics：语料总览 + 时间线 + 词频统计 + 语料报告。
 *
 * 图表全部用内联 SVG / CSS 画，不引图表库。
 * 三种图（柱、线、条）用一个库来画不划算，而且要求书第十五条第 3 点
 * 明确说了不要为了视觉效果引入大量依赖。
 *
 * 也刻意**不做成孤立的 dashboard**（第十五条第 8 点）：词频表里的每个词
 * 都能点开，直接跳到曲库页看它的例句，或者去检索页看 KWIC。数据是入口，不是终点。
 *
 * 词频统计和报告对应 PyQt 版的「统计」「报告」两页，共用一组筛选（歌手、只看日文）。
 * 数字的写法照 Python（千分位、正好一半取偶），和导出的 TXT 一致。
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";

import {
  api,
  formatLongDuration,
  POS_LABELS,
  type DurationBackfillReport,
  type FrequencyRow,
  type Overview,
  type StatsFilter,
  type StatsReport,
  type YearStats,
} from "../api";
import { PerformerPicker } from "../components/PerformerPicker";
import { Stat } from "../components/Stat";
import { pyFixed, pyGrouped } from "../pyFormat";
import type { LibraryActions } from "../useLibrary";

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

interface Props {
  overview: Overview | null;
  onOverviewChanged: () => void;
  actions: LibraryActions;
  onNavigate: () => void;
  /** 跳到检索页，按词元查这个词 */
  onSearchWord: (lemma: string) => void;
  onError: (message: string) => void;
}

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

export function AnalyticsPage({
  overview,
  onOverviewChanged,
  actions,
  onNavigate,
  onSearchWord,
  onError,
}: Props) {
  const [timeline, setTimeline] = useState<YearStats[]>([]);
  const [backfill, setBackfill] = useState<DurationBackfillReport | null>(null);
  const [backfilling, setBackfilling] = useState(false);

  const [filter, setFilter] = useState<StatsFilter>({ performerIds: [], jpOnly: false });
  const [pos, setPos] = useState("");
  const [rows, setRows] = useState<FrequencyRow[] | null>(null);
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
    const id = ++reportRequest.current;
    setLoadingReport(true);
    setExportNote("");
    api
      .statsReport(filter)
      .then((result) => id === reportRequest.current && setReport(result))
      .catch((e) => id === reportRequest.current && onError(message(e)))
      .finally(() => id === reportRequest.current && setLoadingReport(false));
  }, [filter, onError]);

  const maxTracks = useMemo(
    () => Math.max(1, ...timeline.map((y) => y.trackCount)),
    [timeline],
  );

  const openWord = async (lemma: string) => {
    await actions.openWord(lemma);
    onNavigate();
  };

  const exportReport = async () => {
    if (!report) return;
    try {
      const path = await save({
        title: "保存统计报告",
        defaultPath: "corpus_report.txt",
        filters: [{ name: "文本文件", extensions: ["txt"] }],
      });
      if (!path) return;
      await api.exportText(path, report.text);
      setExportNote(`已保存：${path}`);
    } catch (err) {
      onError(message(err));
    }
  };

  const filterDesc =
    (report?.performers.length ? report.performers.join("、") : "全部歌手") + (report?.jpOnly ? " / 仅日文" : "");

  return (
    <div className="page analytics">
      {overview && (
        <section className="card">
          <h2>语料概览</h2>
          <div className="overview-grid">
            <Stat label="曲目" value={overview.tracks} align="start" />
            <Stat label="专辑" value={overview.albums} align="start" />
            <Stat label="人物" value={overview.people} align="start" />
            <Stat label="演唱" value={overview.performers} align="start" />
            <Stat label="作曲" value={overview.composers} align="start" />
            <Stat label="作词" value={overview.lyricists} align="start" />
            <Stat label="编曲" value={overview.arrangers} align="start" />
            <Stat label="歌词行" value={overview.lyricLines} align="start" />
            <Stat label="词次" value={overview.tokens} align="start" />
            <Stat label="词汇量" value={overview.vocabulary} align="start" />
            <Stat
              label="总时长"
              value={formatLongDuration(overview.totalDurationSec)}
              align="start"
            />
          </div>

          {overview.totalDurationSec === 0 && !backfill && (
            <div className="maintenance">
              <p className="muted small">
                总时长为空：<code>songs.duration_sec</code> 还没回填。
                回填会逐个读音频文件的时长写回库，幂等，可随时重跑。
              </p>
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
                {backfilling ? "回填中…" : "回填时长"}
              </button>
            </div>
          )}

          {backfill && (
            <div className="maintenance">
              <p className="small">
                扫描 {backfill.scanned} 首：写入 {backfill.written}
                {backfill.missing > 0 && `，文件缺失 ${backfill.missing}`}
                {backfill.unknown > 0 && `，无时长 ${backfill.unknown}`}
                {backfill.failed > 0 && `，失败 ${backfill.failed}`}
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
      )}

      <section className="card">
        <h2>时间线</h2>
        <p className="muted small">按年份的作品数与词汇量。年份为空的曲目不计入。</p>
        {timeline.length === 0 && <p className="muted">没有带年份的曲目</p>}
        {timeline.length > 0 && (
          <div className="timeline">
            {timeline.map((y) => (
              <div className="year" key={y.year} title={`${y.trackCount} 首 · ${y.vocabulary} 词`}>
                <div className="year-bars">
                  <span
                    className="bar tracks"
                    style={{ height: `${(y.trackCount / maxTracks) * 100}%` }}
                  />
                </div>
                <span className="year-label">{y.year.slice(2)}</span>
              </div>
            ))}
          </div>
        )}
      </section>

      <div className="stats-filters">
        <span className="muted small">筛选</span>
        <PerformerPicker
          selected={filter.performerIds}
          onChange={(performerIds) => setFilter((f) => ({ ...f, performerIds }))}
          onError={onError}
        />
        <label className="check" title="只统计含假名或汉字的词元（去掉英文、数字）">
          <input
            type="checkbox"
            checked={filter.jpOnly}
            onChange={(e) => setFilter((f) => ({ ...f, jpOnly: e.target.checked }))}
          />
          仅日文
        </label>
        <span className="muted small">词频表和报告都按这里筛选</span>
      </div>

      <section className="card">
        <div className="card-head">
          <h2>词频统计</h2>
          <div className="card-tools">
            <span className="muted small">
              {loadingRows ? "读取中…" : rows ? `${rows.length} 个词` : ""}
            </span>
            <select className="rate" value={pos} onChange={(e) => setPos(e.target.value)}>
              {POS_FILTERS.map((p) => (
                <option key={p} value={p}>
                  {p === "" ? "全部词性" : `${POS_LABELS[p] ?? p} ${p}`}
                </option>
              ))}
            </select>
          </div>
        </div>
        <p className="muted small">
          词元 × 词性按出现次数排，前 {TABLE_LIMIT} 个，不含标点和符号。JLPT 等级来自本地缓存。
          点词元看例句，点「检索」到检索页看 KWIC。
        </p>
        {rows && rows.length > 0 && (
          <div className={`table-wrap${loadingRows ? " stale" : ""}`}>
            <table className="data">
              <thead>
                <tr>
                  <th className="num">#</th>
                  <th>词元</th>
                  <th>词性</th>
                  <th className="mid">JLPT</th>
                  <th className="num">出现次数</th>
                  <th className="num">出现曲数</th>
                  <th>表层形例</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {rows.map((w, i) => (
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
                    <td title={w.surfaces.join("、")}>
                      <span className="surfaces">{w.surfaces.slice(0, 5).join("、")}</span>
                    </td>
                    <td className="row-actions">
                      <button className="link-btn" onClick={() => onSearchWord(w.lemma)}>
                        检索
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        {rows && rows.length === 0 && !loadingRows && <p className="muted">没有数据</p>}
      </section>

      <section className="card report">
        <div className="card-head">
          <h2>语料报告</h2>
          <div className="card-tools">
            <span className="muted small">
              {loadingReport
                ? "计算中…"
                : report
                  ? `${pyGrouped(report.report.songCount)} 曲 / ${pyGrouped(report.report.tokenCount)} tokens`
                  : ""}
            </span>
            <button className="ghost" disabled={!report || loadingReport} onClick={() => void exportReport()}>
              导出 TXT
            </button>
          </div>
        </div>
        <p className="muted small">
          TTR、STTR、Hapax、词性分布、高频词覆盖率。导出的 TXT 和 PyQt 版格式相同。
          {exportNote && <span className="export-note"> {exportNote}</span>}
        </p>
        {report && <ReportBody data={report} filterDesc={filterDesc} stale={loadingReport} onWord={openWord} />}
      </section>
    </div>
  );
}

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
      <p className="small">筛选：{filterDesc}</p>

      <div className="report-cards">
        <ReportCard label="曲数" value={pyGrouped(r.songCount)} />
        <ReportCard label="歌词行数" value={pyGrouped(r.utteranceCount)} />
        <ReportCard label="Token 数" value={pyGrouped(r.tokenCount)} note="实词 token，去除标点" />
        <ReportCard label="Type 数" value={pyGrouped(r.typeCount)} note="唯一 lemma 数" />
      </div>

      <h3>词汇多样性</h3>
      <div className="report-cards">
        <ReportCard label="TTR" value={pyFixed(r.ttr, 4)} note="Types / Tokens" />
        <ReportCard
          label="STTR"
          value={r.sttr === null ? "—" : pyFixed(r.sttr, 4)}
          note={r.sttrChunk ? `标准化 TTR / ${r.sttrChunk} tokens` : "标准化 TTR（词次不足 1000）"}
        />
        <ReportCard
          label="Hapax 比率"
          value={`${pyFixed(r.hapaxRatio * 100, 1)}%`}
          note={`${pyGrouped(r.hapaxCount)} 个词只出现 1 次`}
        />
        <ReportCard label="平均每曲词汇量" value={pyFixed(r.avgTypesPerSong, 0)} note="Types / Song" />
        <ReportCard label="平均每行 token" value={pyFixed(r.avgTokensPerLine, 1)} note="Tokens / Line" />
      </div>

      <h3>词性分布</h3>
      <table className="data">
        <thead>
          <tr>
            <th>词性</th>
            <th>和名</th>
            <th className="num">Token 数</th>
            <th className="num">比率</th>
            <th className="num">Type 数</th>
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

      <h3>高频词覆盖</h3>
      <div className="report-cards">
        {r.coverage.map((c) => (
          <ReportCard key={c.n} label={`Top ${pyGrouped(c.n)}`} value={`${pyFixed(c.pct, 1)}%`}>
            <Bar value={c.pct} max={100} />
          </ReportCard>
        ))}
      </div>

      <h3>高频词元 Top 20</h3>
      <table className="data">
        <thead>
          <tr>
            <th className="num">排名</th>
            <th>词元</th>
            <th className="num">出现次数</th>
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
