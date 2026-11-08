/**
 * 循环区间的调整栏（#64）：时间轴开始晚了、结束早了，循环时听不全，在这里挪一挪。
 *
 * 起点、终点各有 −0.5 / −0.1 / +0.1 / +0.5 秒，改了马上生效；改起点会从新起点重放。
 * 键盘：`[` `]` 挪起点，Shift+`[` `]` 挪终点，按着 Alt 一下 0.5 秒（见 useHotkeys.ts）。
 * 调过的按行记住，试听和制卡截的那一段也用调过的。
 */

import { tr } from "../i18n";
import {
  COARSE_STEP_MS,
  FINE_STEP_MS,
  extendLineLoop,
  nudgeLineLoop,
  resetLineLoop,
  stopLineLoop,
  type LoopRegion,
} from "../lineLoop";

interface Props {
  region: LoopRegion;
  /** 这首歌有时间戳的行，按时间顺序：往前 / 往后加一行用 */
  timedIds: readonly number[];
  onError: (message: string) => void;
}

/** 0:12.3 —— 微调的粒度是 0.1 秒，显示也到 0.1 秒 */
export function formatLoopTime(sec: number): string {
  const tenths = Math.round(Math.max(0, sec) * 10);
  const m = Math.floor(tenths / 600);
  const s = (tenths % 600) / 10;
  return `${m}:${s.toFixed(1).padStart(4, "0")}`;
}

/** +0.3 / −0.2：调过多少。没调过是空 */
function formatDelta(ms: number): string {
  if (ms === 0) return "";
  return `${ms > 0 ? "+" : "−"}${(Math.abs(ms) / 1000).toFixed(1)}`;
}

const STEPS = [-COARSE_STEP_MS, -FINE_STEP_MS, FINE_STEP_MS, COARSE_STEP_MS];

function stepLabel(ms: number): string {
  return `${ms > 0 ? "+" : "−"}${Math.abs(ms) / 1000}`;
}

export function LoopAdjustBar({ region, timedIds, onError }: Props) {
  const a = timedIds.indexOf(region.firstUtteranceId);
  const b = timedIds.indexOf(region.lastUtteranceId);
  const lines = a >= 0 && b >= 0 ? b - a + 1 : 1;
  const adjusted = region.startDeltaMs !== 0 || region.endDeltaMs !== 0;

  const run = (job: Promise<boolean>) =>
    void job.then((ok) => {
      if (!ok) onError(tr("这一段调不了了，可能歌词刚换过"));
    }, (e: unknown) => onError(String(e)));

  const edge = (which: "start" | "end") => {
    const sec = which === "start" ? region.startSec : region.endSec;
    const delta = which === "start" ? region.startDeltaMs : region.endDeltaMs;
    const keys = which === "start" ? "[ ]" : "Shift+[ ]";
    return (
      <div className="loop-edge" role="group" aria-label={which === "start" ? tr("起点") : tr("终点")}>
        <span className="loop-edge-label">{which === "start" ? tr("起点") : tr("终点")}</span>
        {STEPS.slice(0, 2).map((ms) => (
          <button key={ms} className="loop-step" onClick={() => run(nudgeLineLoop(which === "start" ? { startMs: ms } : { endMs: ms }))}
            title={tr("提前 {sec} 秒（{keys}）", { sec: Math.abs(ms) / 1000, keys })}>
            {stepLabel(ms)}
          </button>
        ))}
        <span className="loop-time">
          {formatLoopTime(sec)}
          {delta !== 0 && <small>{formatDelta(delta)}</small>}
        </span>
        {STEPS.slice(2).map((ms) => (
          <button key={ms} className="loop-step" onClick={() => run(nudgeLineLoop(which === "start" ? { startMs: ms } : { endMs: ms }))}
            title={tr("推后 {sec} 秒（{keys}）", { sec: ms / 1000, keys })}>
            {stepLabel(ms)}
          </button>
        ))}
      </div>
    );
  };

  return (
    <div className="loop-adjust" aria-label={tr("循环区间")}>
      <span className="loop-adjust-title">
        {lines > 1 ? tr("循环 {n} 行", { n: lines }) : tr("循环这一句")}
      </span>
      {edge("start")}
      {edge("end")}
      <span className="loop-adjust-actions">
        <button className="link-btn" disabled={a <= 0} onClick={() => run(extendLineLoop(timedIds, "before"))}>
          {tr("往前加一行")}
        </button>
        <button className="link-btn" disabled={b < 0 || b >= timedIds.length - 1} onClick={() => run(extendLineLoop(timedIds, "after"))}>
          {tr("往后加一行")}
        </button>
        <button className="link-btn" disabled={!adjusted} onClick={() => run(resetLineLoop())} title={tr("回到按时间轴算的起止")}>
          {tr("重置")}
        </button>
        <button className="link-btn" onClick={() => void stopLineLoop()} title="Ctrl+L">
          {tr("取消循环")}
        </button>
      </span>
    </div>
  );
}
