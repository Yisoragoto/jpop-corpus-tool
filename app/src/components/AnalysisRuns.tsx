/**
 * 分析记录：把当前这份报告 / 词频表连同条件、范围、分词器、口径一起存下来，
 * 之后在现在的库上原样重跑，看结果变没变、为什么变。
 *
 * 数据见 `jp-corpus/src/analysis.rs`。
 */

import { useEffect, useState } from "react";

import type { StatsFilter } from "../api";
import {
  ANALYSIS_ARCHIVED,
  describeFilter,
  KIND_LABELS,
  metricLabel,
  qualityApi,
  tokenizerLabel,
  type AnalysisRun,
  type AnalysisSpec,
  type AnalysisSummary,
  type Comparison,
} from "../corpusQuality";
import { pyGrouped } from "../pyFormat";

interface Props {
  filter: StatsFilter;
  /** 词频表当前选的词性（空串是全部） */
  pos: string;
  frequencyLimit: number;
  onError: (message: string) => void;
}

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));
const when = (iso: string) => iso.replace("T", " ").replace(/Z$/, "");
const num = (v: number | null) =>
  v === null ? "—" : Number.isInteger(v) ? pyGrouped(v) : v.toFixed(Math.abs(v) < 1 ? 4 : 2);

export function AnalysisRuns({ filter, pos, frequencyLimit, onError }: Props) {
  const [runs, setRuns] = useState<AnalysisSummary[]>([]);
  const [busy, setBusy] = useState(false);
  const [label, setLabel] = useState("");
  const [comparison, setComparison] = useState<Comparison | null>(null);
  const [detail, setDetail] = useState<AnalysisRun | null>(null);

  const load = () => qualityApi.analyses(50).then(setRuns).catch((e) => onError(message(e)));
  useEffect(() => {
    const reload = () => qualityApi.analyses(50).then(setRuns).catch((e) => onError(message(e)));
    void reload();
    // 副歌效应、子语料对比卡上的「存档」存完通知这里
    window.addEventListener(ANALYSIS_ARCHIVED, reload);
    return () => window.removeEventListener(ANALYSIS_ARCHIVED, reload);
  }, [onError]);

  const busyWith = async (job: () => Promise<void>) => {
    setBusy(true);
    try {
      await job();
    } catch (err) {
      onError(message(err));
    } finally {
      setBusy(false);
    }
  };

  const save = (spec: AnalysisSpec) =>
    busyWith(async () => {
      const run = await qualityApi.runAnalysis(spec, filter, label);
      setLabel("");
      setDetail(run);
      setComparison(null);
      await load();
    });

  return (
    <section className="card analysis-runs">
      <div className="card-head">
        <h2>分析记录</h2>
        <div className="card-tools">
          <input type="text" className="label-input" placeholder="备注（可不填）" value={label} onChange={(e) => setLabel(e.target.value)} />
          <button className="ghost" disabled={busy} onClick={() => void save({ kind: "corpusReport" })}>
            存档报告
          </button>
          <button
            className="ghost"
            disabled={busy}
            onClick={() => void save({ kind: "frequency", pos: pos || null, limit: frequencyLimit })}
          >
            存档词频表
          </button>
        </div>
      </div>
      <p className="muted small">
        存档按上面的条件现算一次，连同筛选条件、纳入的每首歌和它的歌词版本、用过的分词器、统计口径和生成时间一起存下来。
        「重跑」在现在的库上用同样的条件再算一次，并说明和原来差在哪。
      </p>

      {runs.length === 0 && <p className="muted small">还没有存档。</p>}
      {runs.length > 0 && (
        <table className="data compact">
          <thead>
            <tr>
              <th className="num">#</th>
              <th>种类</th>
              <th>范围</th>
              <th className="num">曲数</th>
              <th>时间</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {runs.map((r) => (
              <tr key={r.id}>
                <td className="num muted">{r.id}</td>
                <td>
                  {KIND_LABELS[r.kind] ?? r.kind}
                  {r.label && <small className="muted"> {r.label}</small>}
                  {r.rerunOf !== null && <small className="muted"> ↻#{r.rerunOf}</small>}
                </td>
                <td className="small">{describeFilter(r.filter)}</td>
                <td className="num">{r.songCount}</td>
                <td className="muted small">{when(r.createdAt)}</td>
                <td className="row-actions">
                  <button
                    className="link-btn"
                    onClick={() =>
                      void busyWith(async () => {
                        setDetail(await qualityApi.analysis(r.id));
                        setComparison(null);
                      })
                    }
                  >
                    详情
                  </button>
                  <button
                    className="link-btn"
                    disabled={busy}
                    onClick={() =>
                      void busyWith(async () => {
                        setComparison(await qualityApi.rerun(r.id));
                        setDetail(null);
                        await load();
                      })
                    }
                  >
                    重跑对比
                  </button>
                  {r.rerunOf !== null && (
                    <button
                      className="link-btn"
                      onClick={() =>
                        void busyWith(async () => {
                          setComparison(await qualityApi.compare(r.rerunOf!, r.id));
                          setDetail(null);
                        })
                      }
                    >
                      和 #{r.rerunOf} 比
                    </button>
                  )}
                  <button
                    className="link-btn danger"
                    onClick={() =>
                      void busyWith(async () => {
                        await qualityApi.deleteAnalysis(r.id);
                        if (detail?.id === r.id) setDetail(null);
                        await load();
                      })
                    }
                  >
                    删除
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {detail && <RunDetail run={detail} onClose={() => setDetail(null)} />}
      {comparison && <ComparisonView data={comparison} onClose={() => setComparison(null)} />}
    </section>
  );
}

function RunDetail({ run, onClose }: { run: AnalysisRun; onClose: () => void }) {
  return (
    <div className="run-detail small">
      <div className="card-head">
        <h3>
          #{run.id} {KIND_LABELS[run.spec.kind]}
          {run.label && ` · ${run.label}`}
        </h3>
        <button className="link-btn" onClick={onClose}>
          收起
        </button>
      </div>
      <dl>
        <dt>生成时间</dt>
        <dd>
          {when(run.createdAt)} · 程序 {run.appVersion || "?"} · 库结构 {run.schemaVersion}
        </dd>
        <dt>条件</dt>
        <dd>{describeFilter(run.filter)}</dd>
        <dt>范围</dt>
        <dd>
          {run.scope.songCount} 首 / {pyGrouped(run.scope.lineCount)} 行 · 指纹 <code>{run.scopeHash}</code>
        </dd>
        <dt>分词器</dt>
        <dd>
          {run.tokenizers.runs.map((t) => (
            <div key={t.id}>
              {tokenizerLabel(t)}（{t.lineCount} 行）
            </div>
          ))}
          {run.tokenizers.unknownLines > 0 && <div className="muted">{run.tokenizers.unknownLines} 行不知道用的哪个分词器（老库导入的）</div>}
        </dd>
        <dt>口径</dt>
        <dd>
          <ul className="method-list">
            {Object.entries(run.method).map(([k, v]) => (
              <li key={k}>
                <code>{k}</code> {Array.isArray(v) ? v.join(", ") : String(v)}
              </li>
            ))}
          </ul>
        </dd>
        {run.snapshot && (
          <>
            <dt>库指纹</dt>
            <dd>
              <code>{run.snapshot.fingerprint}</code> · {run.snapshot.songs} 首 / {pyGrouped(run.snapshot.tokens)} 词 /{" "}
              {run.snapshot.corrections} 处校正
            </dd>
          </>
        )}
        <dt>结果指纹</dt>
        <dd>
          <code>{run.resultHash}</code>
        </dd>
      </dl>
    </div>
  );
}

function ComparisonView({ data, onClose }: { data: Comparison; onClose: () => void }) {
  const flags: [string, boolean][] = [
    ["条件", data.sameFilter],
    ["参数", data.sameSpec],
    ["口径", data.sameMethod],
    ["范围", data.sameScope],
    ["分词器", data.sameTokenizers],
    ["库结构", data.sameSchema],
    ...(data.sameCorpus === null ? [] : ([["库指纹", data.sameCorpus]] as [string, boolean][])),
  ];
  return (
    <div className="run-detail comparison small">
      <div className="card-head">
        <h3>
          #{data.before.id} → #{data.after.id}：{data.resultIdentical ? "结果完全一致" : "结果不同"}
        </h3>
        <button className="link-btn" onClick={onClose}>
          收起
        </button>
      </div>
      <p>
        {flags.map(([name, same]) => (
          <span key={name} className={`chip ${same ? "" : "on"}`}>
            {name}
            {same ? "相同" : "不同"}
          </span>
        ))}
      </p>
      {data.reasons.length > 0 && (
        <ul className="reasons">
          {data.reasons.map((r) => (
            <li key={r}>{r}</li>
          ))}
        </ul>
      )}
      {(data.scope.added.length > 0 || data.scope.removed.length > 0 || data.scope.lyricsChanged.length > 0) && (
        <p className="muted">
          {data.scope.added.length > 0 && `新纳入：${data.scope.added.slice(0, 20).join("、")}${data.scope.added.length > 20 ? "…" : ""}　`}
          {data.scope.removed.length > 0 && `少了：${data.scope.removed.slice(0, 20).join("、")}　`}
          {data.scope.lyricsChanged.length > 0 && `换了歌词：${data.scope.lyricsChanged.slice(0, 20).join("、")}`}
        </p>
      )}
      {data.metrics.length > 0 && (
        <table className="data compact">
          <thead>
            <tr>
              <th>指标</th>
              <th className="num">之前</th>
              <th className="num">之后</th>
            </tr>
          </thead>
          <tbody>
            {data.metrics.map((m) => (
              <tr key={m.name}>
                <td>{metricLabel(m.name)}</td>
                <td className="num">{num(m.before)}</td>
                <td className="num">{num(m.after)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {data.words.length > 0 && (
        <table className="data compact">
          <thead>
            <tr>
              <th>词元</th>
              <th>词性</th>
              <th className="num">名次</th>
              <th className="num">频次</th>
            </tr>
          </thead>
          <tbody>
            {data.words.slice(0, 50).map((w) => (
              <tr key={`${w.lemma}:${w.pos}`}>
                <td>{w.lemma}</td>
                <td className="muted">{w.pos}</td>
                <td className="num">
                  {w.beforeRank ?? "—"} → {w.afterRank ?? "—"}
                </td>
                <td className="num">
                  {w.beforeFreq ?? "—"} → {w.afterFreq ?? "—"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
