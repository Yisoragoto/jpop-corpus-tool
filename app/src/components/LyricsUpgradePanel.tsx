import { useCallback, useEffect, useRef, useState } from "react";
import { api, LYRICS_SOURCE_LABELS, type LyricsUpgradeProgress } from "../api";
import { listen } from "../events";
import { tr, trBackend } from "../i18n";

const LABELS: Record<string, string> = {
  upgraded: tr("已补逐字"), lineFilled: tr("逐行回退"), protected: tr("保留人工修改"),
  complete: tr("逐字已完整"), kept: tr("保留现有逐字"), review: tr("待逐首复核"), notFound: tr("未找到"),
  failed: tr("获取失败"), cancelled: tr("已停止"), cachedNotFound: tr("近期查过没找到"),
};

/** Status lives in Rust, so changing pages never loses the running queue. */
export function LyricsUpgradePanel({ onChanged, onError, allowStart = true }: {
  onChanged: () => void; onError: (message: string) => void; allowStart?: boolean;
}) {
  const [job, setJob] = useState<LyricsUpgradeProgress | null>(null);
  const [busy, setBusy] = useState(false);
  const alive = useRef(false);
  const callbacks = useRef({ onChanged, onError }); callbacks.current = { onChanged, onError };
  const previous = useRef<LyricsUpgradeProgress | null>(null);
  const pending = useRef(false);
  const actionPending = useRef(false);
  const refresh = useCallback(async () => {
    if (pending.current) return;
    pending.current = true;
    try {
      const next = await api.lyricsUpgradeStatus();
      if (!alive.current) return;
      if (previous.current?.running && !next.running) callbacks.current.onChanged();
      previous.current = next;
      setJob(next);
    } catch (error) {
      if (alive.current) callbacks.current.onError(String(error));
    } finally { pending.current = false; }
  }, []);
  useEffect(() => {
    alive.current = true; void refresh();
    const off = listen<LyricsUpgradeProgress>("lyrics-upgrade://progress", () => void refresh());
    const timer = setInterval(() => void refresh(), 2000);
    return () => { alive.current = false; clearInterval(timer); void off.then(f => f()); };
  }, [refresh]);
  const action = async (cancel: boolean) => {
    if (actionPending.current) return;
    actionPending.current = true;
    setBusy(true);
    try { await (cancel ? api.lyricsUpgradeCancel() : api.lyricsUpgradeStart()); await refresh(); }
    catch (error) { callbacks.current.onError(String(error)); }
    finally { actionPending.current = false; if (alive.current) setBusy(false); }
  };
  if (!allowStart && (!job || job.total === 0)) return null;
  return <div className="lyrics-batch" aria-busy={busy}>
    <div className="lyrics-batch-heading">
      <div><strong>{allowStart ? tr("全库逐字歌词") : tr("歌词后台任务")}</strong><p>{tr("自动匹配并优先补逐字；保留人工修改，无法确认的留待复核。")}</p></div>
      {job?.running
        ? <button type="button" className="btn" disabled={busy || job.cancelled} onClick={() => void action(true)}>{job.cancelled ? tr("正在停止…") : tr("停止任务")}</button>
        : allowStart && <button type="button" className="primary" disabled={busy} onClick={() => void action(false)}>{busy ? tr("正在启动…") : tr("一键补全逐字歌词")}</button>}
    </div>
    {job && job.total > 0 && <>
      <div className="lyrics-batch-status" role="status">
        <span>{job.running ? (job.cancelled ? tr("正在停止…") : tr("正在处理")) : job.cancelled ? tr("已停止") : tr("处理完成")} · {job.done} / {job.total}</span>
        {job.running && <span className="ellipsis">{job.title}</span>}
      </div>
      <progress max={job.total} value={job.done} aria-label={tr("歌词处理进度")} />
      <div className="lyrics-batch-totals">
        <span>{tr("逐字 {n}", { n: job.upgraded })}</span><span>{tr("逐行回退 {n}", { n: job.lineFilled })}</span>
        <span>{tr("保留 / 待复核 {n}", { n: job.skipped })}</span><span>{tr("未找到 {n}", { n: job.notFound })}</span><span>{tr("失败 {n}", { n: job.failed })}</span>
      </div>
      {job.error && <p className="warn" role="alert">{trBackend(job.error)}</p>}
      {job.results.length > 0 && <details className="lyrics-batch-results"><summary>{tr("查看处理结果")}</summary>
        <div>{job.results.map(item => <div className="lyrics-batch-item" key={item.songId}>
          <strong title={item.title}>{item.title}</strong><span>{LABELS[item.status] ?? item.status}</span>
          <small>{LYRICS_SOURCE_LABELS[item.source as keyof typeof LYRICS_SOURCE_LABELS] ?? item.source}</small>
          {item.message && <p>{trBackend(item.message)}</p>}
        </div>)}</div>
      </details>}
    </>}
  </div>;
}
