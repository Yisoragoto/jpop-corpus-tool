/**
 * 播放条。只订阅状态、只发命令，自己不持有任何播放状态。
 *
 * 唯一的本地状态是「进度条正在被拖动」——拖动期间不能被轮询回来的
 * 位置覆盖，否则手指还没松开滑块就跳回去了。
 *
 * **松手的监听常驻**，不随「正在拖」挂上去：effect 在渲染之后才跑，
 * 而在进度条上点一下（按下即改值、立刻松手）时，pointerup 往往早于
 * 那次 effect——第一下于是只改了显示、没发 seek，看起来就是
 * 「要点两次才跳得过去」。常驻监听没有这个时序窗口。
 *
 * 版面照 Spotify 的三栏：左边这首歌、中间走带和进度、右边工具和音量。
 * 控件按 Windows 11 的质感做（半透明填充 + 细描边 + 悬停变亮），
 * 窗口本身开了 Mica，所以这一条留了透明度，桌面的颜色会透上来。
 */

import { useEffect, useRef, useState } from "react";

import { api, formatDuration, PITCH_STEPS, pitchLabel, RATE_STEPS, type PlaybackState } from "../api";
import { Cover } from "./Cover";
import { Flyout } from "./Flyout";

/** 前进 / 后退一步的秒数。听写歌词时反复倒回来的那一小段 */
const STEP_SEC = 5;

interface Props {
  state: PlaybackState;
  spectrum: number[];
  title: string;
  artist: string;
  /** 正在放的这首的封面，`songs.cover_path`。没选歌时是 undefined。 */
  coverPath?: string | null | undefined;
  /** 引擎不可用时整条禁用，并说明原因 */
  disabled: boolean;
  onError: (message: string) => void;
}

export function Player({ state, spectrum, title, artist, coverPath, disabled, onError }: Props) {
  const [scrubbing, setScrubbing] = useState<number | null>(null);
  /** 和 scrubbing 同步的副本，给常驻的 pointerup 读——监听器只挂一次，闭包里拿不到最新的 state */
  const scrubRef = useRef<number | null>(null);
  const duration = state.durationSec;
  const position = scrubbing ?? state.positionSec;
  const playing = state.playState === "playing";
  const idle = state.playState === "empty" && !state.pitchRendering;

  // 换歌时丢掉拖动中的位置，否则新歌会从旧位置开始显示
  useEffect(() => {
    scrubRef.current = null;
    setScrubbing(null);
  }, [state.songId]);

  // 松手在哪都算数：只听滑块自己的 mouseup 的话，拖出滑块再松手就白拖了，
  // 进度条弹回原处，看着像「拖不动」。**只挂一次**，理由见文件头。
  useEffect(() => {
    const commit = () => {
      const target = scrubRef.current;
      if (target === null) return;
      scrubRef.current = null;
      // 等引擎真的跳过去再放开显示。先清空的话，下一拍轮询回来的还是
      // 跳之前的位置，进度条会先弹回去再跳过来，看着像「跳歪了」。
      void api
        .audioSeek(target)
        .catch(() => undefined)
        .finally(() => setScrubbing(null));
    };
    window.addEventListener("pointerup", commit);
    window.addEventListener("pointercancel", commit);
    return () => {
      window.removeEventListener("pointerup", commit);
      window.removeEventListener("pointercancel", commit);
    };
  }, []);

  const scrub = (value: number) => {
    scrubRef.current = value;
    setScrubbing(value);
  };

  // 变调失败（已退回原调）只提示一次
  const shownPitchError = useRef<number | null>(null);
  useEffect(() => {
    const error = state.pitchError;
    if (error === null || error.id === shownPitchError.current) return;
    shownPitchError.current = error.id;
    onError(error.message);
  }, [state.pitchError, onError]);

  const step = (delta: number) => {
    const target = Math.max(0, position + delta);
    void api.audioSeek(duration === null ? target : Math.min(target, duration));
  };
  const pct = duration && duration > 0 ? Math.min(100, (position / duration) * 100) : 0;

  return (
    <footer className={`player ${disabled ? "off" : ""}`}>
      <Spectrum bands={spectrum} active={playing} />

      <div className="seek-row">
        <span className="time">{formatDuration(position)}</span>
        <span className="seek-wrap" style={{ ["--played" as string]: `${pct}%` }}>
          <input
            className="seekbar"
            type="range"
            min={0}
            max={duration ?? 0}
            step={0.05}
            value={duration ? Math.min(position, duration) : 0}
            disabled={disabled || !duration}
            onChange={(e) => scrub(Number(e.target.value))}
            onKeyUp={() => {
              // 键盘调进度：方向键改完就跳。和鼠标那条路各走各的，
              // 因为键盘没有 pointerup。
              const target = scrubRef.current;
              if (target === null) return;
              scrubRef.current = null;
              void api
                .audioSeek(target)
                .catch(() => undefined)
                .finally(() => setScrubbing(null));
            }}
            aria-label="播放进度"
          />
        </span>
        {/* 拿不到时长的格式（某些流式 mp3）显示占位而不是 0:00 */}
        <span className="time">{duration === null ? "—:—" : formatDuration(duration)}</span>
      </div>

      <div className="player-row">
        <div className="player-now">
          <Cover path={coverPath} size={48} rounded={6} />
          <div className="now">
            <span className="now-title">{title || "—"}</span>
            <span className="now-artist">{artist}</span>
          </div>
        </div>

        <div className="player-center">
          <div className="transport">
            <button
              className="icon-btn"
              onClick={() => step(-STEP_SEC)}
              disabled={disabled || idle}
              title={`后退 ${STEP_SEC} 秒 (←)`}
            >
              <IconBack />
            </button>
            <button
              className="play-btn"
              onClick={() => void api.audioToggle()}
              // 等变调渲染时引擎还是空的，但播放键照样能按（记到渲染好之后）
              disabled={disabled || idle}
              title={playing ? "暂停 (空格)" : "播放 (空格)"}
            >
              {playing ? <IconPause /> : <IconPlay />}
            </button>
            <button
              className="icon-btn"
              onClick={() => step(STEP_SEC)}
              disabled={disabled || idle}
              title={`前进 ${STEP_SEC} 秒 (→)`}
            >
              <IconForward />
            </button>
            <button
              className={`icon-btn ${state.loopRegion ? "on" : ""}`}
              onClick={() => void api.audioSetLoop()}
              disabled={disabled || !state.loopRegion}
              title={
                state.loopRegion
                  ? `单句循环 ${formatDuration(state.loopRegion.startSec)}–${formatDuration(state.loopRegion.endSec)}，点击取消`
                  : "单句循环：在歌词里选一句开启"
              }
            >
              <IconLoop />
            </button>
          </div>

        </div>

        <div className="player-tools">
          {state.pitchRendering && <span className="pitch-busy">生成 {pitchLabel(state.pitchSemitones)} 音频…</span>}

          <Flyout
            caption="速度"
            label={`${state.rate.toFixed(2).replace(/0$/, "")}×`}
            title="倍速（不改变音高）"
            value={state.rate}
            active={state.rate !== 1}
            disabled={disabled}
            options={RATE_STEPS.map((r) => ({ value: r, label: `${r.toFixed(2).replace(/0$/, "")}×` }))}
            onPick={(r) => void api.audioSetRate(r)}
            note="不改变音高，走 WSOLA 时间伸缩"
          />

          <Flyout
            caption="调"
            label={pitchLabel(state.pitchSemitones)}
            title="音调（升降调，不改变速度）"
            value={state.pitchSemitones}
            active={state.pitchSemitones !== 0}
            disabled={disabled}
            options={PITCH_STEPS.map((n) => ({ value: n, label: pitchLabel(n) }))}
            onPick={(n) =>
              void api
                .audioSetPitch(n)
                .catch((err: unknown) => onError(err instanceof Error ? err.message : String(err)))
            }
            note="没用过的调要先生成几秒，之后有缓存"
          />

          <span className="volume-wrap" title="音量">
            <IconVolume muted={state.volume === 0} />
            <input
              className="volume"
              type="range"
              min={0}
              max={1}
              step={0.01}
              value={state.volume}
              disabled={disabled}
              onChange={(e) => void api.audioSetVolume(Number(e.target.value))}
              style={{ ["--played" as string]: `${state.volume * 100}%` }}
              aria-label="音量"
            />
          </span>
        </div>
      </div>
    </footer>
  );
}

/** 频谱条。没有数据时画一条静默的基线，而不是留空。 */
function Spectrum({ bands, active }: { bands: number[]; active: boolean }) {
  const values = bands.length ? bands : new Array(48).fill(0);
  return (
    <div className={`spectrum ${active ? "on" : ""}`} aria-hidden="true">
      {values.map((v, i) => (
        <span key={i} style={{ height: `${Math.max(2, v * 100)}%` }} />
      ))}
    </div>
  );
}

/* 图标都用内联 SVG：不引图标库，线宽和 Windows 的 Segoe Fluent 对齐 */

function IconPlay() {
  return (
    <svg viewBox="0 0 24 24" width="15" height="15" fill="currentColor" aria-hidden="true">
      <path d="M8 5.2v13.6L19 12 8 5.2z" />
    </svg>
  );
}

function IconPause() {
  return (
    <svg viewBox="0 0 24 24" width="15" height="15" fill="currentColor" aria-hidden="true">
      <path d="M7 5h3.2v14H7zM13.8 5H17v14h-3.2z" />
    </svg>
  );
}

function IconBack() {
  return (
    <svg viewBox="0 0 24 24" width="17" height="17" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M11 6 5 12l6 6" />
      <path d="M19 6l-6 6 6 6" />
    </svg>
  );
}

function IconForward() {
  return (
    <svg viewBox="0 0 24 24" width="17" height="17" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M13 6l6 6-6 6" />
      <path d="M5 6l6 6-6 6" />
    </svg>
  );
}

function IconLoop() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M4 12a8 8 0 0 1 8-8h5" />
      <path d="M14 1.5 17.5 4 14 6.5" />
      <path d="M20 12a8 8 0 0 1-8 8H7" />
      <path d="M10 22.5 6.5 20 10 17.5" />
    </svg>
  );
}

function IconVolume({ muted }: { muted: boolean }) {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M4 9.5v5h3.5L12 18V6L7.5 9.5H4z" />
      {muted ? <path d="M16 9.5l4 5M20 9.5l-4 5" /> : <path d="M15.5 9a4.2 4.2 0 0 1 0 6" />}
    </svg>
  );
}
