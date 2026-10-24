/**
 * 卡拉 OK 模式：背景是这首歌的封面，底下两行 KTV 式逐字歌词（上下交替，唱完一行换下下行），
 * 入句前倒数，前奏显示歌名和词曲。右上角「打轴」进打轴页，可以自己打、自己调。
 *
 * - 逐字点亮只用已验证的真实时间轴（和普通歌词同一份 `charTimeline`）；没有逐字时间的行到点整行亮起，
 *   不按字数均摊；
 * - 动画每帧直接改 DOM（`paintKaraoke`），和全屏歌词一样不走 React 状态，只有换行、倒数、前奏这种
 *   低频变化才 setState；
 * - 以后的 MV / 视频背景替换的是 `.ktv-bg` 这一层，歌词层不变。
 */

import { Fragment, useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

import { api, ROLE_LABELS, type Credit, type LyricLine, type PlaybackState, type Track } from "../api";
import { holdFullscreen, isFullscreenHeld, releaseFullscreen } from "../fullscreen";
import { tr } from "../i18n";
import { ktvLayout, type KtvLayout } from "../karaoke";
import type { Segment } from "../lyricSegments";
import { advanceClock, paintKaraoke, type ClockState } from "../useKaraoke";
import type { CharTimeline, WordTiming } from "../wordTiming";
import { useAppSettings } from "../settings";
import { AmllBackground } from "./AmllBackground";
import { usePlayerHeight } from "./LyricsStage";
import { TimingEditor } from "./TimingEditor";

interface Props {
  track: Track;
  coverPath: string | null | undefined;
  credits: Credit[];
  lyrics: LyricLine[];
  /** 每行已验证的逐字时间轴（null：这一行没有完整的真实单位） */
  timelines: (CharTimeline | null)[];
  /** 每行的振假名拆分；null 时只显示原文 */
  segments: (Segment[] | null)[];
  wordTimings: WordTiming[];
  playback: PlaybackState;
  style: CSSProperties;
  onSaved: () => void;
  onError: (message: string) => void;
  onClose: () => void;
}

const EMPTY_LAYOUT: KtvLayout = { slots: [null, null], active: null, countdown: 0, intro: false };

function sameLayout(a: KtvLayout, b: KtvLayout): boolean {
  return a.slots[0] === b.slots[0] && a.slots[1] === b.slots[1] && a.active === b.active && a.countdown === b.countdown && a.intro === b.intro;
}

export function KaraokeStage(props: Props) {
  const { track, lyrics, timelines, playback, onClose } = props;
  const [editing, setEditing] = useState(false);
  const [bgBroken, setBgBroken] = useState(false);
  useEffect(() => setBgBroken(false), [props.coverPath]);
  // 背景用 AMLL 的流动封面（设置里可关，WebGL 起不来时退回虚化封面）
  const [amllBgFailed, setAmllBgFailed] = useState(false);
  const flowingBg = useAppSettings().amllBackground && !amllBgFailed;
  const playerHeight = usePlayerHeight();
  const [fullscreen, setFullscreen] = useState(() => isFullscreenHeld("karaoke"));
  useEffect(() => () => releaseFullscreen("karaoke"), []);

  const thisSong = playback.songId === track.id;
  // 打开时这首歌还没在放：直接从头放（点「卡拉 OK」就是要唱这一首）
  const loaded = useRef(false);
  useEffect(() => {
    if (loaded.current) return;
    loaded.current = true;
    if (!thisSong) api.audioLoad(track.id, { positionSec: 0, autoplay: true }).catch((e) => props.onError(String(e)));
    // 只在打开时做一次
  }, []);

  const ktvLines = useMemo(
    () => lyrics.map((line, i) => {
      const t = timelines[i];
      return { start: t?.starts[0] ?? line.timeSec, end: t ? t.ends[t.ends.length - 1] ?? null : null };
    }),
    [lyrics, timelines],
  );
  const timedCount = ktvLines.filter((l) => l.start !== null).length;

  const [layout, setLayout] = useState<KtvLayout>(EMPTY_LAYOUT);
  const slotRefs = [useRef<HTMLDivElement>(null), useRef<HTMLDivElement>(null)];
  const polled = useRef({ pos: playback.positionSec, at: performance.now() });
  useEffect(() => {
    polled.current = { pos: thisSong ? playback.positionSec : 0, at: performance.now() };
  }, [playback.positionSec, thisSong]);

  const playing = thisSong && playback.playState === "playing";
  useEffect(() => {
    if (editing) return;
    let frame = 0;
    let clock: ClockState | null = null;
    // 从当前已经显示的布局比起：不然每次重启都会把同样的布局再 set 一遍，渲染、重启，没完没了
    let last = layout;
    const paint = () => {
      clock = advanceClock(clock, polled.current, performance.now(), playing, playback.rate);
      const next = ktvLayout(ktvLines, clock.sec);
      if (!sameLayout(next, last)) { last = next; setLayout(next); }
      next.slots.forEach((index, slot) => {
        const box = slotRefs[slot]!.current;
        if (box === null || index === null || Number(box.dataset.idx) !== index) return;
        const chars = box.querySelectorAll<HTMLElement>(".kc");
        const timeline = timelines[index] ?? null;
        paintKaraoke(timeline, clock!.sec, chars);
        // 没有逐字时间的行：到点整行亮起，不估算每个字
        const start = ktvLines[index]?.start ?? null;
        box.classList.toggle("ktv-line-on", timeline === null && start !== null && clock!.sec >= start);
      });
      if (playing) frame = requestAnimationFrame(paint);
    };
    frame = requestAnimationFrame(paint);
    return () => cancelAnimationFrame(frame);
    // 暂停时拖进度条也要重画一帧
  }, [editing, playing, playback.rate, ktvLines, timelines, layout, playing ? 0 : playback.positionSec]);

  // 键盘：空格播放/暂停，Esc 退出（打轴页自己管键盘）
  useEffect(() => {
    if (editing) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (e.key === "Escape") { e.preventDefault(); onClose(); }
      else if (e.key === " ") {
        e.preventDefault();
        if (document.activeElement instanceof HTMLButtonElement) document.activeElement.blur();
        if (!e.repeat) void api.audioToggle();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [editing, onClose]);

  const renderLine = (index: number): ReactNode => {
    const line = lyrics[index];
    if (line === undefined) return null;
    let c = 0;
    const chars = (text: string) => [...text].map((ch) => {
      const at = c++;
      return <span key={at} className="kc" data-c={at}>{ch}</span>;
    });
    const segs = props.segments[index];
    if (segs === null || segs === undefined) return chars(line.text);
    return segs.map((segment, k) => {
      if (segment.kind === "gap") return <Fragment key={k}>{chars(segment.text)}</Fragment>;
      const token = line.tokens[segment.index];
      if (segment.pieces.length === 0) return <Fragment key={k}>{chars(token?.surface ?? "")}</Fragment>;
      return (
        <Fragment key={k}>
          {segment.pieces.map((piece, p) => piece.reading === undefined
            ? <Fragment key={p}>{chars(piece.text)}</Fragment>
            : <ruby key={p}>{chars(piece.text)}<rt>{piece.reading}</rt></ruby>)}
        </Fragment>
      );
    });
  };

  const roles = ["lyricist", "composer", "arranger"]
    .map((role) => ({ role, names: props.credits.filter((c) => c.role === role).map((c) => c.name) }))
    .filter((r) => r.names.length > 0);

  const toggleFullscreen = () => {
    if (isFullscreenHeld("karaoke")) releaseFullscreen("karaoke");
    else holdFullscreen("karaoke");
    setFullscreen(isFullscreenHeld("karaoke"));
  };

  return (
    <div
      className={`ktv ${editing ? "ktv-editing" : ""} ${layout.intro && !editing ? "ktv-intro-on" : ""}`}
      style={{ ...props.style, ["--player-h" as string]: `${playerHeight}px` }}
      role="dialog"
      aria-label={tr("卡拉 OK")}
    >
      <div className="ktv-bg" aria-hidden="true">
        {props.coverPath && !bgBroken && (
          <>
            {flowingBg ? (
              <AmllBackground coverPath={props.coverPath} playing={playing} onFail={() => setAmllBgFailed(true)} />
            ) : (
              <img className="ktv-bg-fill" src={convertFileSrc(props.coverPath)} alt="" draggable={false} onError={() => setBgBroken(true)} />
            )}
            <img className="ktv-bg-cover" src={convertFileSrc(props.coverPath)} alt="" draggable={false} />
          </>
        )}
      </div>

      <header className="ktv-head">
        <div className="ktv-title">
          <strong>{track.title}</strong>
          <span>{track.artist}</span>
        </div>
        <button className={`ktv-btn ${editing ? "on" : ""}`} onClick={() => setEditing((v) => !v)} disabled={lyrics.length === 0}
          title={tr("自己打逐字时间轴、手动微调")}>
          {editing ? tr("回到卡拉 OK") : tr("打轴")}
        </button>
        <button className={`ktv-btn ${fullscreen ? "on" : ""}`} onClick={toggleFullscreen} title={tr("窗口全屏")} aria-label={tr("窗口全屏")}>⛶</button>
        <button className="ktv-btn" onClick={onClose} title={tr("退出卡拉 OK（Esc）")} aria-label={tr("退出卡拉 OK")}>✕</button>
      </header>

      {editing ? (
        <TimingEditor
          songId={track.id}
          lyrics={lyrics}
          wordTimings={props.wordTimings}
          durationSec={track.durationSec}
          playback={playback}
          onSaved={props.onSaved}
          onError={props.onError}
          onClose={() => setEditing(false)}
        />
      ) : (
        <>
          {layout.intro && (
            <div className="ktv-intro">
              <h1>{track.title}</h1>
              <p className="ktv-intro-artist">{track.artist}</p>
              {roles.map((r) => (
                <p key={r.role} className="ktv-intro-credit">
                  <span>{ROLE_LABELS[r.role] ?? r.role}</span>
                  {r.names.join(" / ")}
                </p>
              ))}
            </div>
          )}
          {lyrics.length === 0 && <p className="ktv-empty">{tr("这首歌还没有歌词")}</p>}
          {lyrics.length > 0 && timedCount === 0 && (
            <div className="ktv-empty">
              <p>{tr("这首歌的歌词没有时间轴，卡拉 OK 跟不上")}</p>
              <button className="ktv-btn on" onClick={() => setEditing(true)}>{tr("自己打轴")}</button>
            </div>
          )}
          <div className="ktv-lyrics" lang="ja">
            {layout.slots.map((index, slot) => (
              <div
                key={slot}
                ref={slotRefs[slot]}
                data-idx={index ?? ""}
                className={`ktv-row ktv-row-${slot === 0 ? "top" : "bottom"} ${index !== null && index === layout.active ? "ktv-row-active" : ""}`}
              >
                {index !== null && layout.countdown > 0 && index === layout.active && (
                  <span className="ktv-countdown" aria-label={tr("倒数 {n}", { n: layout.countdown })}>
                    {"●".repeat(layout.countdown)}
                  </span>
                )}
                {/* key 跟着行走：换行时整行重新渲染，上一行点亮的类不会留在新字上 */}
                {index !== null && <span key={index} className="ktv-text">{renderLine(index)}</span>}
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
