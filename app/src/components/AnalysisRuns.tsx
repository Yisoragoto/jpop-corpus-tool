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
import { tr } from "../i18n";

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
        <h2>{tr("分析记录")}</h2>
        <div className="card-tools">
          <input type="text" className="label-input" placeholder={tr("备注（可不填）")} value={label} onChange={(e) => setLabel(e.target.value)} />
          <button className="ghost" disabled={busy} onClick={() => void save({ kind: "corpusReport" })}>{tr("存档报告")}</button>
          <button
            className="ghost"
            disabled={busy}
            onClick={() => void save({ kind: "frequency", pos: pos || null, limit: frequencyLimit })}
          >{tr("存档词频表")}</button>
        </div>
      </div>
      <p className="muted small">{tr("存档按上面的条件现算一次，连同筛选条件、纳入的每首歌和它的歌词版本、用过的分词器、统计口径和生成时间一起存下来。 「重跑」在现在的库上用同样的条件再算一次，并说明和原来差在哪。")}</p>

      {runs.length === 0 && <p className="muted small">{tr("还没有存档。")}</p>}
      {runs.length > 0 && (
        <table className="data compact">
          <thead>
            <tr>
              <th className="num">#</th>
              <th>{tr("种类")}</th>
              <th>{tr("范围")}</th>
              <th className="num">{tr("曲数")}</th>
              <th>{tr("时间")}</th>
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
                  >{tr("详情")}</button>
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
                  >{tr("重跑对比")}</button>
                  {r.rerunOf !== null && (
                    <button
                      className="link-btn"
                      onClick={() =>
                        void busyWith(async () => {
                          setComparison(await qualityApi.compare(r.rerunOf!, r.id));
                          setDetail(null);
                        })
                      }
                    >{tr("和 #{rerunOf} 比", { rerunOf: r.rerunOf })}</button>
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
                  >{tr("删除")}</button>
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
        <button className="link-btn" onClick={onClose}>{tr("收起")}</button>
      </div>
      <dl>
        <dt>{tr("生成时间")}</dt>
        <dd>{tr("{createdAt} · 程序", { createdAt: when(run.createdAt) })}{" "}{run.appVersion || "?"}{" "}{tr("· 库结构 {schemaVersion}", { schemaVersion: run.schemaVersion })}</dd>
        <dt>{tr("条件")}</dt>
        <dd>{describeFilter(run.filter)}</dd>
        <dt>{tr("范围")}</dt>
        <dd>{tr("{songCount} 首 / {lineCount} 行 · 指纹", { songCount: run.scope.songCount, lineCount: pyGrouped(run.scope.lineCount) })}{" "}<code>{run.scopeHash}</code>
        </dd>
        <dt>{tr("分词器")}</dt>
        <dd>
          {run.tokenizers.runs.map((t) => (
            <div key={t.id}>{tr("{t}（{lineCount} 行）", { t: tokenizerLabel(t), lineCount: t.lineCount })}</div>
          ))}
          {run.tokenizers.unknownLines > 0 && <div className="muted">{tr("{unknownLines} 行不知道用的哪个分词器（老库导入的）", { unknownLines: run.tokenizers.unknownLines })}</div>}
        </dd>
        <dt>{tr("口径")}</dt>
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
            <dt>{tr("库指纹")}</dt>
            <dd>
              <code>{run.snapshot.fingerprint}</code>{" "}{tr("· {songs} 首 / {tokens} 词 /{v}{corrections} 处校正", { songs: run.snapshot.songs, tokens: pyGrouped(run.snapshot.tokens), v: " ", corrections: run.snapshot.corrections })}</dd>
          </>
        )}
        <dt>{tr("结果指纹")}</dt>
        <dd>
          <code>{run.resultHash}</code>
        </dd>
      </dl>
    </div>
  );
}

function ComparisonView({ data, onClose }: { data: Comparison; onClose: () => void }) {
  const flags: [string, boolean][] = [
    [tr("条件"), data.sameFilter],
    [tr("参数"), data.sameSpec],
    [tr("口径"), data.sameMethod],
    [tr("范围"), data.sameScope],
    [tr("分词器"), data.sameTokenizers],
    [tr("库结构"), data.sameSchema],
    ...(data.sameCorpus === null ? [] : ([[tr("库指纹"), data.sameCorpus]] as [string, boolean][])),
  ];
  return (
    <div className="run-detail comparison small">
      <div className="card-head">
        <h3>
          #{data.before.id} → #{data.after.id}{tr("：")}{data.resultIdentical ? tr("结果完全一致") : tr("结果不同")}
        </h3>
        <button className="link-btn" onClick={onClose}>{tr("收起")}</button>
      </div>
      <p>
        {flags.map(([name, same]) => (
          <span key={name} className={`chip ${same ? "" : "on"}`}>
            {name}
            {same ? tr("相同") : tr("不同")}
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
          {data.scope.added.length > 0 && tr("新纳入：{slice}{v}　", { slice: data.scope.added.slice(0, 20).join(tr("、")), v: data.scope.added.length > 20 ? "…" : "" })}
          {data.scope.removed.length > 0 && tr("少了：{slice}　", { slice: data.scope.removed.slice(0, 20).join(tr("、")) })}
          {data.scope.lyricsChanged.length > 0 && tr("换了歌词：{slice}", { slice: data.scope.lyricsChanged.slice(0, 20).join(tr("、")) })}
        </p>
      )}
      {data.metrics.length > 0 && (
        <table className="data compact">
          <thead>
            <tr>
              <th>{tr("指标")}</th>
              <th className="num">{tr("之前")}</th>
              <th className="num">{tr("之后")}</th>
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
              <th>{tr("词元")}</th>
              <th>{tr("词性")}</th>
              <th className="num">{tr("名次")}</th>
              <th className="num">{tr("频次")}</th>
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
