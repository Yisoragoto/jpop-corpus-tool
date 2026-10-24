/**
 * 打轴：听着歌按键给每个字记时间，再在波形上拖、用键盘微调。
 *
 * - 「按住」模式：按下 = 这个字开始唱，松开 = 唱完（AMLL 打轴工具的做法，结尾和停顿都打得出来）；
 *   「点按」模式：每个字按一下，前一个字唱到这一下为止，一行最后一个字按住、松开记结尾；
 * - 按键时刻直接问一次播放引擎的位置，扣掉一半往返时间和「按键提前量」（人的反应 + 声卡缓冲，可调）；
 * - 慢放用播放条同一个倍速，记下来的是原曲时间；
 * - 选中的字可以在波形上拖边界，或者 `[` `]`（开始）`,` `.`（结尾）每次 10 毫秒地挪，按住 Shift 是 100 毫秒；
 *   连唱的两个字之间只有一条线，拖一下两边一起动；
 * - 存的是新的一版歌词（增强 LRC），旧版在「来源与版本」里能恢复；手动打的轴批量补歌词不会覆盖。
 */

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { api, type LyricLine, type PlaybackState } from "../api";
import { tr } from "../i18n";
import {
  clearLine,
  editLinesFrom,
  lineEnd,
  lineStart,
  lineStatus,
  mergeNext,
  nextCursor,
  nudge,
  prevCursor,
  setEdge,
  splitUnit,
  tapDown,
  tapUp,
  toSavePayload,
  unitText,
  type Cursor,
  type EditLine,
  type TapMode,
} from "../karaoke";
import { advanceClock, type ClockState } from "../useKaraoke";
import { REACTION_SEC, type WordTiming } from "../wordTiming";

interface Props {
  songId: string;
  lyrics: LyricLine[];
  wordTimings: WordTiming[];
  durationSec: number | null;
  playback: PlaybackState;
  onSaved: () => void;
  onError: (message: string) => void;
  onClose: () => void;
}

interface History {
  lines: EditLine[];
  cursor: Cursor;
  past: EditLine[][];
  future: EditLine[][];
  dirty: boolean;
}

/** 波形每秒几格 */
const PEAKS_PER_SEC = 100;
const HISTORY_LIMIT = 200;
const RATES = [0.5, 0.75, 1] as const;

const PREF_KEY = "timing-editor";
function loadPrefs(): { mode: TapMode; offsetMs: number } {
  const fallback = { mode: "hold" as TapMode, offsetMs: Math.round(REACTION_SEC * 1000) };
  try {
    const raw = JSON.parse(localStorage.getItem(PREF_KEY) ?? "{}") as { mode?: unknown; offsetMs?: unknown };
    return {
      mode: raw.mode === "tap" ? "tap" : "hold",
      offsetMs: typeof raw.offsetMs === "number" && Number.isFinite(raw.offsetMs) ? Math.min(400, Math.max(0, raw.offsetMs)) : fallback.offsetMs,
    };
  } catch {
    return fallback;
  }
}
function savePrefs(prefs: { mode: TapMode; offsetMs: number }) {
  try { localStorage.setItem(PREF_KEY, JSON.stringify(prefs)); } catch { /* 存不了就只在这次有效 */ }
}

function formatTime(sec: number | null): string {
  if (sec === null) return "—";
  const m = Math.floor(sec / 60);
  const s = sec - m * 60;
  return `${m}:${s.toFixed(3).padStart(6, "0")}`;
}

function firstCursor(lines: EditLine[]): Cursor {
  const line = lines.findIndex((l) => l.units.length > 0);
  return { line: Math.max(0, line), unit: 0 };
}

export function TimingEditor(props: Props) {
  const { songId, lyrics, wordTimings, durationSec, playback, onError } = props;
  const [state, setState] = useState<History>(() => {
    const lines = editLinesFrom(lyrics, wordTimings);
    return { lines, cursor: firstCursor(lines), past: [], future: [], dirty: false };
  });
  const stateRef = useRef(state);
  stateRef.current = state;

  // 歌词或时间轴重读了（刚存完、或者时间轴晚一步到）：没有没存的改动就跟着换
  useEffect(() => {
    setState((prev) => {
      if (prev.dirty) return prev;
      const lines = editLinesFrom(lyrics, wordTimings);
      const line = Math.min(prev.cursor.line, Math.max(0, lines.length - 1));
      const unit = Math.min(prev.cursor.unit, Math.max(0, (lines[line]?.units.length ?? 1) - 1));
      return { lines, cursor: { line, unit }, past: [], future: [], dirty: false };
    });
  }, [lyrics, wordTimings]);

  const [prefs, setPrefs] = useState(loadPrefs);
  const prefsRef = useRef(prefs);
  prefsRef.current = prefs;
  useEffect(() => savePrefs(prefs), [prefs]);

  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [confirmLeave, setConfirmLeave] = useState(false);
  const [peaks, setPeaks] = useState<number[] | null>(null);
  useEffect(() => {
    let alive = true;
    api.audioPeaks(songId, PEAKS_PER_SEC).then((p) => alive && setPeaks(p), () => alive && setPeaks([]));
    return () => { alive = false; };
  }, [songId]);

  /** 改一下：`record` 时进撤销历史 */
  const apply = useCallback((change: (s: History) => { lines: EditLine[]; cursor?: Cursor }, record = true) => {
    setState((prev) => {
      const next = change(prev);
      const past = record ? [...prev.past, prev.lines].slice(-HISTORY_LIMIT) : prev.past;
      return { lines: next.lines, cursor: next.cursor ?? prev.cursor, past, future: record ? [] : prev.future, dirty: true };
    });
    setNotice(null);
  }, []);

  const undo = useCallback(() => setState((prev) => {
    const back = prev.past[prev.past.length - 1];
    if (back === undefined) return prev;
    return { ...prev, lines: back, past: prev.past.slice(0, -1), future: [prev.lines, ...prev.future], dirty: true };
  }), []);
  const redo = useCallback(() => setState((prev) => {
    const forward = prev.future[0];
    if (forward === undefined) return prev;
    return { ...prev, lines: forward, past: [...prev.past, prev.lines], future: prev.future.slice(1), dirty: true };
  }), []);

  const select = useCallback((cursor: Cursor) => setState((prev) => ({ ...prev, cursor })), []);

  // ── 按键时刻的播放位置 ──
  // 问一次引擎：位置是返回前某一刻采的，按往返时间的一半往回推到按键那一刻，再扣掉提前量
  const tapQueue = useRef<Promise<void>>(Promise.resolve());
  const position = useCallback(async () => {
    const t0 = performance.now();
    const s = await api.audioState();
    const t1 = performance.now();
    if (s.playState !== "playing") return s.positionSec;
    const rate = s.rate > 0 ? s.rate : 1;
    return s.positionSec - ((t1 - t0) / 2000) * rate - (prefsRef.current.offsetMs / 1000) * rate;
  }, []);
  const tap = useCallback((kind: "down" | "up") => {
    tapQueue.current = tapQueue.current.then(async () => {
      const sec = await position();
      const mode = prefsRef.current.mode;
      if (kind === "down") apply((s) => tapDown(s.lines, s.cursor, sec, mode));
      // 松开和按下算同一步撤销
      else apply((s) => tapUp(s.lines, s.cursor, sec, mode), false);
    }).catch((e) => onError(String(e)));
  }, [apply, position, onError]);

  const lines = state.lines;
  const cursor = state.cursor;
  const current = lines[cursor.line];
  const unit = current?.units[cursor.unit];

  const playFromLine = useCallback(async (index: number) => {
    const all = stateRef.current.lines;
    const line = all[index];
    if (line === undefined) return;
    const prevEnd = all.slice(0, index).map(lineEnd).filter((t): t is number => t !== null).pop() ?? null;
    const start = lineStart(line) ?? prevEnd ?? 0;
    try {
      await api.audioSeek(Math.max(0, start - 1.5));
      await api.audioPlay();
    } catch (e) {
      onError(String(e));
    }
  }, [onError]);

  const save = useCallback(async () => {
    const all = stateRef.current.lines;
    const broken = all.map((_, i) => i).filter((i) => lineStatus(all, i, durationSec).status === "error");
    if (broken.length > 0) {
      setNotice(tr("第 {lines} 行的时间倒退了，改好再存", { lines: broken.map((i) => i + 1).join("、") }));
      return;
    }
    setSaving(true);
    try {
      await api.lyricsSaveTiming(songId, toSavePayload(all));
      setState((prev) => ({ ...prev, dirty: false }));
      setNotice(tr("已保存为新的一版歌词（旧版在「来源与版本」里能恢复）"));
      props.onSaved();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }, [durationSec, songId, onError, props]);

  const leave = useCallback(() => {
    if (stateRef.current.dirty) setConfirmLeave(true);
    else props.onClose();
  }, [props]);

  // ── 键盘 ──
  useEffect(() => {
    const step = (e: KeyboardEvent) => (e.shiftKey ? 0.1 : 0.01);
    const onDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement) return;
      const ctrl = e.ctrlKey || e.metaKey;
      const at = stateRef.current.cursor;
      let handled = true;
      if (e.code === "Space") {
        // 刚点过的按钮还带着焦点：空格别去「按」它
        if (document.activeElement instanceof HTMLButtonElement) document.activeElement.blur();
        if (!e.repeat) tap("down");
      }
      else if (ctrl && e.code === "KeyZ") { if (e.shiftKey) redo(); else undo(); }
      else if (ctrl && e.code === "KeyY") redo();
      else if (ctrl && e.code === "KeyS") void save();
      else if (ctrl) handled = false;
      else if (e.code === "Enter" || e.code === "KeyP") void api.audioToggle();
      else if (e.code === "KeyR") { select({ line: at.line, unit: 0 }); void playFromLine(at.line); }
      else if (e.code === "ArrowRight") { const n = nextCursor(stateRef.current.lines, at); if (n) select(n); }
      else if (e.code === "ArrowLeft") { const n = prevCursor(stateRef.current.lines, at); if (n) select(n); }
      else if (e.code === "ArrowDown" || e.code === "ArrowUp") {
        const all = stateRef.current.lines;
        const dir = e.code === "ArrowDown" ? 1 : -1;
        for (let i = at.line + dir; i >= 0 && i < all.length; i += dir) {
          if ((all[i]?.units.length ?? 0) > 0) { select({ line: i, unit: 0 }); break; }
        }
      }
      else if (e.code === "BracketLeft") apply((s) => ({ lines: nudge(s.lines, s.cursor, "start", -step(e)) }));
      else if (e.code === "BracketRight") apply((s) => ({ lines: nudge(s.lines, s.cursor, "start", step(e)) }));
      else if (e.code === "Comma") apply((s) => ({ lines: nudge(s.lines, s.cursor, "end", -step(e)) }));
      else if (e.code === "Period") apply((s) => ({ lines: nudge(s.lines, s.cursor, "end", step(e)) }));
      else if (e.code === "Escape") leave();
      else handled = false;
      if (handled) e.preventDefault();
    };
    const onUp = (e: KeyboardEvent) => {
      if (e.code !== "Space") return;
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      e.preventDefault();
      tap("up");
    };
    document.addEventListener("keydown", onDown);
    document.addEventListener("keyup", onUp);
    return () => {
      document.removeEventListener("keydown", onDown);
      document.removeEventListener("keyup", onUp);
    };
  }, [apply, leave, playFromLine, redo, save, select, tap, undo]);

  // 选中的行滚到看得见的地方
  const listRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    listRef.current?.querySelector(`[data-line="${cursor.line}"]`)?.scrollIntoView({ block: "nearest" });
  }, [cursor.line]);

  const statuses = useMemo(() => lines.map((_, i) => lineStatus(lines, i, durationSec)), [lines, durationSec]);
  const doneCount = statuses.filter((s) => s.status === "done").length;
  const withUnits = lines.filter((l) => l.units.length > 0).length;

  const STATUS_LABEL: Record<string, string> = {
    done: tr("逐字"),
    partial: tr("未完成"),
    empty: tr("未打"),
    error: tr("时间倒退"),
  };
  const REASON_LABEL: Record<string, string> = {
    overlapNext: tr("结尾晚于下一行的开始，播放时这一行整行高亮"),
    pastEnd: tr("结尾超出了音频长度"),
    back: tr("后面的字比前面的字先唱，存不了"),
  };

  return (
    <div className="te">
      <div className="te-toolbar">
        <div className="te-group" role="radiogroup" aria-label={tr("打点方式")}>
          <button className={`ktv-btn ${prefs.mode === "hold" ? "on" : ""}`} onClick={() => setPrefs({ ...prefs, mode: "hold" })}
            title={tr("按下空格 = 开始唱，松开 = 唱完")}>{tr("按住")}</button>
          <button className={`ktv-btn ${prefs.mode === "tap" ? "on" : ""}`} onClick={() => setPrefs({ ...prefs, mode: "tap" })}
            title={tr("每个字按一下空格；一行最后一个字按住、松开记结尾")}>{tr("点按")}</button>
        </div>
        <div className="te-group" aria-label={tr("播放速度")}>
          {RATES.map((r) => (
            <button key={r} className={`ktv-btn ${Math.abs(playback.rate - r) < 1e-3 ? "on" : ""}`} onClick={() => void api.audioSetRate(r)}>
              {r}×
            </button>
          ))}
        </div>
        <label className="te-offset" title={tr("按键比听到的晚多少：人的反应加上声卡缓冲。打出来整体偏晚就调大")}>
          {tr("按键提前量")}
          <input type="number" min={0} max={400} step={10} value={prefs.offsetMs}
            onChange={(e) => setPrefs({ ...prefs, offsetMs: Math.min(400, Math.max(0, Number(e.target.value) || 0)) })} />
          ms
        </label>
        <span className="te-progress">{tr("{done} / {total} 行已逐字", { done: doneCount, total: withUnits })}</span>
        <div className="te-group te-right">
          <button className="ktv-btn" onClick={undo} disabled={state.past.length === 0} title="Ctrl+Z">{tr("撤销")}</button>
          <button className="ktv-btn" onClick={redo} disabled={state.future.length === 0} title="Ctrl+Y">{tr("重做")}</button>
          <button className="ktv-btn on" onClick={() => void save()} disabled={saving || !state.dirty} title="Ctrl+S">
            {saving ? tr("保存中…") : tr("保存")}
          </button>
        </div>
      </div>

      {confirmLeave && (
        <div className="te-banner">
          {tr("有没保存的改动。")}
          <button className="ktv-btn on" onClick={() => void save().then(() => { if (!stateRef.current.dirty) props.onClose(); })}>{tr("保存并返回")}</button>
          <button className="ktv-btn" onClick={() => { setConfirmLeave(false); props.onClose(); }}>{tr("放弃改动")}</button>
          <button className="ktv-btn" onClick={() => setConfirmLeave(false)}>{tr("继续打轴")}</button>
        </div>
      )}
      {notice !== null && <div className="te-banner te-notice">{notice}</div>}

      <div className="te-lines" ref={listRef}>
        {lines.map((line, li) => {
          const st = statuses[li]!;
          return (
            <div key={li} data-line={li} className={`te-line ${li === cursor.line ? "on" : ""} te-${st.status}`}>
              <button className="te-line-head" onClick={() => { select({ line: li, unit: 0 }); void playFromLine(li); }}
                title={tr("从这一行前面一点开始放（R）")}>
                <span className="te-line-no">{li + 1}</span>
                <span className="te-line-time">{formatTime(lineStart(line))}</span>
              </button>
              <div className="te-units">
                {line.units.map((u, ui) => (
                  <button
                    key={`${u.charStart}-${u.charEnd}`}
                    className={`te-unit ${u.startSec === null ? "untimed" : u.endSec === null ? "open" : "timed"} ${li === cursor.line && ui === cursor.unit ? "sel" : ""}`}
                    onClick={() => select({ line: li, unit: ui })}
                    title={`${formatTime(u.startSec)} → ${formatTime(u.endSec)}`}
                  >
                    {unitText(line, u)}
                  </button>
                ))}
              </div>
              <span className="te-status" title={st.reason ? REASON_LABEL[st.reason] : undefined}>
                {STATUS_LABEL[st.status]}{st.reason && st.reason !== "back" ? " ⚠" : ""}
              </span>
            </div>
          );
        })}
      </div>

      <div className="te-detail">
        {current !== undefined && (
          <Waveform
            peaks={peaks}
            lines={lines}
            lineIndex={cursor.line}
            selected={cursor.unit}
            playback={playback}
            songId={songId}
            onSelect={(ui) => select({ line: cursor.line, unit: ui })}
            onDragStart={() => apply((s) => ({ lines: s.lines }))}
            onDrag={(ui, edge, sec) => apply((s) => ({ lines: setEdge(s.lines, { line: s.cursor.line, unit: ui }, edge, sec) }), false)}
          />
        )}
        <div className="te-unit-tools">
          <span className="te-unit-name">{current && unit ? `「${unitText(current, unit)}」` : "—"}</span>
          <EdgeControl label={tr("开始")} value={unit?.startSec ?? null}
            onNudge={(d) => apply((s) => ({ lines: nudge(s.lines, s.cursor, "start", d) }))}
            onSet={(v) => apply((s) => ({ lines: setEdge(s.lines, s.cursor, "start", v) }))} />
          <EdgeControl label={tr("结尾")} value={unit?.endSec ?? null}
            onNudge={(d) => apply((s) => ({ lines: nudge(s.lines, s.cursor, "end", d) }))}
            onSet={(v) => apply((s) => ({ lines: setEdge(s.lines, s.cursor, "end", v) }))} />
          <button className="ktv-btn" onClick={() => apply((s) => ({ lines: splitUnit(s.lines, s.cursor) }))}
            disabled={!unit || unit.charEnd - unit.charStart < 2}>{tr("拆开")}</button>
          <button className="ktv-btn" onClick={() => apply((s) => ({ lines: mergeNext(s.lines, s.cursor) }))}
            disabled={!current || cursor.unit + 1 >= current.units.length}>{tr("和后一个合并")}</button>
          <button className="ktv-btn" onClick={() => apply((s) => ({ lines: clearLine(s.lines, s.cursor.line), cursor: { line: s.cursor.line, unit: 0 } }))}
            disabled={!current}>{tr("清空这一行")}</button>
        </div>
        <p className="te-keys">
          {tr("空格 打点 · Enter 播放/暂停 · R 从本行重放 · ←→ 选字 · ↑↓ 选行 · [ ] 开始 ±10ms · , . 结尾 ±10ms（Shift ±100ms）· Ctrl+Z 撤销 · Ctrl+S 保存")}
        </p>
      </div>
    </div>
  );
}

function EdgeControl({ label, value, onNudge, onSet }: { label: string; value: number | null; onNudge: (delta: number) => void; onSet: (sec: number) => void }) {
  const [draft, setDraft] = useState<string | null>(null);
  const commit = () => {
    if (draft === null) return;
    const match = /^(?:(\d+):)?(\d+(?:\.\d+)?)$/.exec(draft.trim());
    if (match) onSet(Number(match[1] ?? 0) * 60 + Number(match[2]));
    setDraft(null);
  };
  return (
    <span className="te-edge">
      <span className="te-edge-label">{label}</span>
      <button className="ktv-btn" onClick={() => onNudge(-0.1)} disabled={value === null}>−100</button>
      <button className="ktv-btn" onClick={() => onNudge(-0.01)} disabled={value === null}>−10</button>
      <input
        className="te-edge-input"
        value={draft ?? formatTime(value)}
        disabled={value === null}
        onFocus={(e) => { setDraft(formatTime(value)); e.currentTarget.select(); }}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); if (e.key === "Escape") { setDraft(null); e.currentTarget.blur(); } }}
        aria-label={label}
      />
      <button className="ktv-btn" onClick={() => onNudge(0.01)} disabled={value === null}>+10</button>
      <button className="ktv-btn" onClick={() => onNudge(0.1)} disabled={value === null}>+100</button>
    </span>
  );
}

/** 读 CSS token 的颜色给 canvas 用 */
function token(el: Element, name: string, fallback: string): string {
  return getComputedStyle(el).getPropertyValue(name).trim() || fallback;
}

/**
 * 选中那一行的波形：前后各多留一点，字块画在上面。拖字块的边缘改时间，点字块选中，点空白处从那里放。
 */
function Waveform(props: {
  peaks: number[] | null;
  lines: EditLine[];
  lineIndex: number;
  selected: number;
  playback: PlaybackState;
  songId: string;
  onSelect: (unit: number) => void;
  onDragStart: () => void;
  onDrag: (unit: number, edge: "start" | "end", sec: number) => void;
}) {
  const { peaks, lines, lineIndex, selected, playback } = props;
  const canvas = useRef<HTMLCanvasElement>(null);
  const [width, setWidth] = useState(800);
  const HEIGHT = 120;
  useEffect(() => {
    const el = canvas.current?.parentElement;
    if (!el) return;
    const observer = new ResizeObserver(() => setWidth(Math.max(200, el.clientWidth)));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const line = lines[lineIndex]!;
  // 显示的时间窗：这一行打过的范围，前后各留一点；没打过就用原来的行首或上一行结尾
  const range = useMemo(() => {
    const prevEnd = lines.slice(0, lineIndex).map(lineEnd).filter((t): t is number => t !== null).pop() ?? null;
    const nextStart = lines.slice(lineIndex + 1).map(lineStart).find((t) => t !== null) ?? null;
    const start = lineStart(line) ?? prevEnd ?? 0;
    const end = Math.max(lineEnd(line) ?? start, nextStart ?? start + 4, start + 3);
    return { from: Math.max(0, start - 1), to: end + 1 };
  }, [lines, lineIndex, line]);
  const span = range.to - range.from;
  const x = useCallback((sec: number) => ((sec - range.from) / span) * width, [range.from, span, width]);
  const sec = useCallback((px: number) => range.from + (px / width) * span, [range.from, span, width]);

  const polled = useRef({ pos: playback.positionSec, at: performance.now() });
  useEffect(() => {
    polled.current = { pos: playback.songId === props.songId ? playback.positionSec : -1, at: performance.now() };
  }, [playback.positionSec, playback.songId, props.songId]);
  const playing = playback.songId === props.songId && playback.playState === "playing";

  useEffect(() => {
    const el = canvas.current;
    if (el === null) return;
    const ctx = el.getContext("2d");
    if (ctx === null) return;
    const dpr = window.devicePixelRatio || 1;
    el.width = Math.round(width * dpr);
    el.height = Math.round(HEIGHT * dpr);
    const colors = {
      wave: token(el, "--te-wave", "#888"),
      unit: token(el, "--te-unit", "#4cc2ff33"),
      sel: token(el, "--te-unit-sel", "#4cc2ff66"),
      edge: token(el, "--accent", "#4cc2ff"),
      text: token(el, "--fg", "#fff"),
      head: token(el, "--danger", "#f66"),
    };
    let frame = 0;
    let clock: ClockState | null = null;
    const draw = () => {
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, width, HEIGHT);
      // 波形
      if (peaks && peaks.length > 0) {
        ctx.fillStyle = colors.wave;
        const mid = HEIGHT / 2;
        for (let px = 0; px < width; px += 2) {
          const a = Math.floor(sec(px) * PEAKS_PER_SEC);
          const b = Math.max(a + 1, Math.floor(sec(px + 2) * PEAKS_PER_SEC));
          let peak = 0;
          for (let i = Math.max(0, a); i < Math.min(peaks.length, b); i += 1) peak = Math.max(peak, peaks[i]!);
          const h = (peak / 255) * (HEIGHT - 20);
          ctx.fillRect(px, mid - h / 2, 1.5, Math.max(1, h));
        }
      }
      // 字块
      ctx.font = "14px sans-serif";
      ctx.textBaseline = "top";
      line.units.forEach((u, ui) => {
        if (u.startSec === null) return;
        const x0 = x(u.startSec);
        const x1 = u.endSec === null ? x0 + 3 : x(u.endSec);
        ctx.fillStyle = ui === selected ? colors.sel : colors.unit;
        ctx.fillRect(x0, 0, Math.max(2, x1 - x0), HEIGHT);
        ctx.fillStyle = colors.edge;
        ctx.fillRect(x0, 0, 1.5, HEIGHT);
        if (u.endSec !== null) ctx.fillRect(x1 - 1.5, 0, 1.5, HEIGHT);
        ctx.fillStyle = colors.text;
        ctx.fillText(unitText(line, u), x0 + 4, 4);
      });
      // 播放头
      clock = advanceClock(clock, polled.current, performance.now(), playing, playback.rate);
      if (polled.current.pos >= 0 && clock.sec >= range.from && clock.sec <= range.to) {
        ctx.fillStyle = colors.head;
        ctx.fillRect(x(clock.sec) - 1, 0, 2, HEIGHT);
      }
      if (playing) frame = requestAnimationFrame(draw);
    };
    frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
    // 暂停时位置变了也重画
  }, [peaks, line, selected, width, x, sec, range, playing, playback.rate, playing ? 0 : playback.positionSec]);

  const drag = useRef<{ unit: number; edge: "start" | "end" } | null>(null);
  const hit = (px: number): { unit: number; edge: "start" | "end" } | { unit: number; edge: null } | null => {
    let best: { unit: number; edge: "start" | "end"; d: number } | null = null;
    line.units.forEach((u, ui) => {
      for (const edge of ["start", "end"] as const) {
        const t = edge === "start" ? u.startSec : u.endSec;
        if (t === null) continue;
        const d = Math.abs(x(t) - px);
        // 贴着的两条边界一样近时，往右拖拿后一个字的开始，往左拿前一个字的结尾（setEdge 会两边一起挪）
        if (d <= 6 && (best === null || d < best.d)) best = { unit: ui, edge, d };
      }
    });
    if (best !== null) return { unit: (best as { unit: number }).unit, edge: (best as { edge: "start" | "end" }).edge };
    const t = sec(px);
    const inside = line.units.findIndex((u) => u.startSec !== null && t >= u.startSec && t <= (u.endSec ?? u.startSec));
    return inside >= 0 ? { unit: inside, edge: null } : null;
  };

  return (
    <div className="te-wave">
      <canvas
        ref={canvas}
        style={{ width: `${width}px`, height: `${HEIGHT}px` }}
        onPointerDown={(e) => {
          const px = e.nativeEvent.offsetX;
          const h = hit(px);
          if (h !== null && h.edge !== null) {
            drag.current = { unit: h.unit, edge: h.edge };
            props.onSelect(h.unit);
            props.onDragStart();
            e.currentTarget.setPointerCapture(e.pointerId);
          } else if (h !== null) {
            props.onSelect(h.unit);
          } else {
            void api.audioSeek(Math.max(0, sec(px)));
          }
        }}
        onPointerMove={(e) => {
          const d = drag.current;
          if (d !== null) props.onDrag(d.unit, d.edge, sec(e.nativeEvent.offsetX));
          else {
            const h = hit(e.nativeEvent.offsetX);
            e.currentTarget.style.cursor = h !== null && h.edge !== null ? "ew-resize" : h !== null ? "pointer" : "text";
          }
        }}
        onPointerUp={() => { drag.current = null; }}
        aria-label={tr("这一行的波形和每个字的时间")}
      />
      {peaks === null && <span className="te-wave-loading">{tr("正在读取波形…")}</span>}
    </div>
  );
}
