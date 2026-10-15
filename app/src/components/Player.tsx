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
 * 版面照 Spotify 的三栏：左边这首歌、中间走带、右边工具和音量，进度条单独一行贯穿整宽。
 * 按钮照 Apple Music 的底栏：只有粗实心的白色字形、没有底框，鼠标移上去才浮出一圈毛玻璃；
 * 倍速和变调合成一个「调音」按钮。封面虚化垫在玻璃底下，整条带上这首歌的颜色。
 */

import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

import { api, formatDuration, PITCH_STEPS, pitchLabel, RATE_STEPS, type PlaybackState } from "../api";
import { useCoverTint } from "../coverColor";
import { Cover } from "./Cover";
import { useSpectrum } from "../usePlayback";

/** 前进 / 后退一步的秒数。听写歌词时反复倒回来的那一小段 */
const STEP_SEC = 5;

interface Props {
  state: PlaybackState;
  title: string;
  artist: string;
  /** 正在放的这首的封面，`songs.cover_path`。没选歌时是 undefined。 */
  coverPath?: string | null | undefined;
  /** 引擎不可用时整条禁用，并说明原因 */
  disabled: boolean;
  /** 频谱条开不开（设置里可关）。关掉时组件不挂载，采样轮询也就跟着停了 */
  showSpectrum: boolean;
  /** 点左下角这首歌：进全屏歌词。没有选中曲目时传 null，卡片就不可点 */
  onOpenStage: (() => void) | null;
  onError: (message: string) => void;
}

export function Player({
  state,
  title,
  artist,
  coverPath,
  disabled,
  showSpectrum,
  onOpenStage,
  onError,
}: Props) {
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
  // 封面的颜色：播放钮的光晕、顶边那道高光都用它，换歌跟着换
  const tint = useCoverTint(coverPath);
  const [ambientBroken, setAmbientBroken] = useState(false);
  const [showRemaining, setShowRemaining] = useState(true);
  const lastVolume = useRef(1);
  useEffect(() => setAmbientBroken(false), [coverPath]);

  return (
    <footer
      className={`player ${disabled ? "off" : ""} ${playing ? "is-playing" : ""}`}
      style={{ ["--player-tint" as string]: tint ?? "rgba(255, 255, 255, .7)" }}
    >
      {/* 封面重度虚化垫在玻璃底下：整条播放条带上这首歌的颜色。单独一层裁切，弹出面板不受影响 */}
      <div className="player-ambient" aria-hidden="true">
        {coverPath && !ambientBroken && (
          <img key={coverPath} src={convertFileSrc(coverPath)} alt="" draggable={false} onError={() => setAmbientBroken(true)} />
        )}
      </div>
      {showSpectrum && <Spectrum enabled={!disabled} active={playing} />}

      <div className="seek-row">
        <span className="time">{scrubbing === null && idle ? "—:—" : formatDuration(position)}</span>
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
        {/* 右边默认显示剩余时长（照 Apple Music），点一下换成总长。拿不到时长的格式显示占位 */}
        <button
          className="time time-toggle"
          onClick={() => setShowRemaining((v) => !v)}
          title={showRemaining ? "显示总时长" : "显示剩余时长"}
          disabled={duration === null}
        >
          {duration === null ? "—:—" : showRemaining ? `-${formatDuration(Math.max(0, duration - position))}` : formatDuration(duration)}
        </button>
      </div>

      <div className="player-row">
        <div
          className={`player-now ${onOpenStage ? "clickable" : ""}`}
          role={onOpenStage ? "button" : undefined}
          tabIndex={onOpenStage ? 0 : undefined}
          title={onOpenStage ? "打开全屏歌词" : undefined}
          onClick={() => onOpenStage?.()}
          onKeyDown={(e) => {
            if (onOpenStage && (e.key === "Enter" || e.key === " ")) {
              e.preventDefault();
              onOpenStage();
            }
          }}
        >
          {/* key 换歌就重挂，封面和歌名一起滑进来 */}
          <div className="now-swap" key={state.songId ?? "none"}>
            <Cover path={coverPath} size={48} rounded={6} />
            <div className="now">
              <span className="now-title">{title || "—"}</span>
              <span className="now-artist">{artist}</span>
            </div>
          </div>
        </div>

        {/* 走带：照 Apple Music，只有图标没有底框；鼠标移上去才浮出一圈玻璃 */}
        <div className="transport">
          <button
            className="glyph-btn nudge-back"
            onClick={() => step(-STEP_SEC)}
            disabled={disabled || idle}
            title={`后退 ${STEP_SEC} 秒 (←)`}
            aria-label={`后退 ${STEP_SEC} 秒`}
          >
            <IconSkip back />
          </button>
          <button
            className={`glyph-btn play-btn ${playing ? "is-playing" : ""}`}
            onClick={() => void api.audioToggle()}
            // 等变调渲染时引擎还是空的，但播放键照样能按（记到渲染好之后）
            disabled={disabled || idle}
            title={playing ? "暂停 (空格)" : "播放 (空格)"}
            aria-label={playing ? "暂停" : "播放"}
          >
            {/* 两个图标叠着，靠 class 切换：播放 ↔ 暂停是一次旋转 + 缩放的交叉淡入，不是硬切 */}
            <span className="play-icon icon-play">
              <IconPlay />
            </span>
            <span className="play-icon icon-pause">
              <IconPause />
            </span>
          </button>
          <button
            className="glyph-btn nudge-forward"
            onClick={() => step(STEP_SEC)}
            disabled={disabled || idle}
            title={`前进 ${STEP_SEC} 秒 (→)`}
            aria-label={`前进 ${STEP_SEC} 秒`}
          >
            <IconSkip />
          </button>
        </div>

        <div className="player-tools">
          {state.pitchRendering && <span className="pitch-busy">生成 {pitchLabel(state.pitchSemitones)} 音频…</span>}
          <button
            className={`glyph-btn small ${state.loopRegion ? "on" : ""}`}
            onClick={() => void api.audioSetLoop()}
            disabled={disabled || !state.loopRegion}
            title={
              state.loopRegion
                ? `单句循环 ${formatDuration(state.loopRegion.startSec)}–${formatDuration(state.loopRegion.endSec)}，点击取消`
                : "单句循环：在歌词里选一句开启"
            }
            aria-label="单句循环"
          >
            <IconLoop />
          </button>
          <button
            className="glyph-btn small"
            onClick={() => onOpenStage?.()}
            disabled={!onOpenStage}
            title="全屏歌词"
            aria-label="全屏歌词"
          >
            <IconLyrics />
          </button>
          <TunePanel state={state} disabled={disabled} onError={onError} />
          <span className="volume-wrap" title="音量">
            <button
              className="glyph-btn tiny"
              onClick={() => {
                // 点小喇叭静音，再点回到静音前的音量
                if (state.volume > 0) {
                  lastVolume.current = state.volume;
                  void api.audioSetVolume(0);
                } else void api.audioSetVolume(lastVolume.current || 1);
              }}
              disabled={disabled}
              aria-label={state.volume === 0 ? "取消静音" : "静音"}
            >
              <IconVolume level={0} muted={state.volume === 0} />
            </button>
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
            <span className="volume-max" aria-hidden="true">
              <IconVolume level={2} muted={false} />
            </span>
          </span>
        </div>
      </div>
    </footer>
  );
}

/**
 * 调音：倍速和变调合成一个按钮（照 Apple Music 底栏那颗「均衡器」）。
 * 两样都不是默认值时按钮下面挂一个小字，一眼看得出现在不是原速原调。
 */
function TunePanel({
  state,
  disabled,
  onError,
}: {
  state: PlaybackState;
  disabled: boolean;
  onError: (message: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  const rateLabel = (r: number) => `${r.toFixed(2).replace(/0$/, "")}×`;
  const changed = state.rate !== 1 || state.pitchSemitones !== 0;
  const badge = [state.rate !== 1 && rateLabel(state.rate), state.pitchSemitones !== 0 && `${state.pitchSemitones > 0 ? "+" : ""}${state.pitchSemitones}`]
    .filter(Boolean)
    .join(" · ");
  const setPitch = (n: number) =>
    void api.audioSetPitch(n).catch((err: unknown) => onError(err instanceof Error ? err.message : String(err)));

  return (
    <div className="tune" ref={root}>
      <button
        className={`glyph-btn small ${changed ? "on" : ""} ${open ? "open" : ""}`}
        onClick={() => setOpen((v) => !v)}
        disabled={disabled}
        title="速度和音调"
        aria-label="速度和音调"
        aria-expanded={open}
      >
        <IconTune />
        {badge && <span className="tune-badge">{badge}</span>}
      </button>
      {open && (
        <div className="flyout-panel tune-panel" role="dialog" aria-label="速度和音调">
          <div className="tune-head">
            <b>速度</b>
            <span>不改变音高</span>
          </div>
          <div className="tune-chips">
            {RATE_STEPS.map((r) => (
              <button key={r} className={r === state.rate ? "on" : ""} onClick={() => r !== state.rate && void api.audioSetRate(r)}>
                {rateLabel(r)}
              </button>
            ))}
          </div>
          <div className="tune-head">
            <b>音调</b>
            <span>没用过的调要先生成几秒，之后有缓存</span>
          </div>
          <div className="tune-chips pitch">
            {PITCH_STEPS.map((n) => (
              <button key={n} className={n === state.pitchSemitones ? "on" : ""} onClick={() => n !== state.pitchSemitones && setPitch(n)}>
                {n === 0 ? "原调" : `${n > 0 ? "+" : ""}${n}`}
              </button>
            ))}
          </div>
          {changed && (
            <button
              className="tune-reset"
              onClick={() => {
                if (state.rate !== 1) void api.audioSetRate(1);
                if (state.pitchSemitones !== 0) setPitch(0);
              }}
            >
              恢复原速原调
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * 频谱条。没有数据时画一条静默的基线，而不是留空。
 *
 * 采样在这里拉，不在 App 上：它播放时每秒变 20 次，挂在顶层整棵树都要跟着重渲染。
 */
function Spectrum({ enabled, active }: { enabled: boolean; active: boolean }) {
  const bands = useSpectrum(enabled, active);
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
    <svg viewBox="0 0 24 24" width="26" height="26" fill="currentColor" aria-hidden="true">
      <path d="M7.5 4.6c0-.9 1-1.5 1.8-1L20 10.6c.8.5.8 1.7 0 2.2L9.3 19.9c-.8.5-1.8-.1-1.8-1z" />
    </svg>
  );
}

function IconPause() {
  return (
    <svg viewBox="0 0 24 24" width="26" height="26" fill="currentColor" aria-hidden="true">
      <rect x="5.5" y="4" width="4.6" height="16" rx="1.4" />
      <rect x="13.9" y="4" width="4.6" height="16" rx="1.4" />
    </svg>
  );
}

/** 前进 / 后退 5 秒：一道圆弧箭头里写着秒数（Apple 的 gobackward.5 那种），不是「上一首 / 下一首」 */
function IconSkip({ back = false }: { back?: boolean }) {
  return (
    <svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <g transform={back ? undefined : "translate(24 0) scale(-1 1)"}>
        <path d="M5.2 8.5A8 8 0 1 1 4 12.6" />
        <path d="M4.6 4v4.8h4.8" />
      </g>
      <text x="12" y="15.6" textAnchor="middle" fontSize="8.5" fontWeight="700" fill="currentColor" stroke="none" fontFamily="Segoe UI, sans-serif">
        {STEP_SEC}
      </text>
    </svg>
  );
}

function IconLoop() {
  return (
    <svg viewBox="0 0 24 24" width="19" height="19" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M4 11V9a3 3 0 0 1 3-3h12" />
      <path d="m16 3 3 3-3 3" />
      <path d="M20 13v2a3 3 0 0 1-3 3H5" />
      <path d="m8 21-3-3 3-3" />
      <path d="M11.5 10.5 13 9.5v5" strokeWidth="1.8" />
    </svg>
  );
}

/** 歌词：一个带引号的对话框 */
function IconLyrics() {
  return (
    <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinejoin="round" aria-hidden="true">
      <path d="M5 4h14a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2h-7l-4.5 3.5V17H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z" />
      <path d="M8.6 9.2h2.4v2.2c0 1-.6 1.7-1.6 2M13.2 9.2h2.4v2.2c0 1-.6 1.7-1.6 2" strokeLinecap="round" />
    </svg>
  );
}

/** 调音：三根推子 */
function IconTune() {
  return (
    <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" aria-hidden="true">
      <path d="M6 3v18M12 3v18M18 3v18" />
      <circle cx="6" cy="14" r="2.2" fill="currentColor" />
      <circle cx="12" cy="8" r="2.2" fill="currentColor" />
      <circle cx="18" cy="16" r="2.2" fill="currentColor" />
    </svg>
  );
}

function IconVolume({ level, muted }: { level: 0 | 2; muted: boolean }) {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M4 9.5v5h3.5L12 18V6L7.5 9.5H4z" fill="currentColor" />
      {muted ? (
        <path d="M16 9.5l4 5M20 9.5l-4 5" />
      ) : level === 2 ? (
        <>
          <path d="M15.5 9a4.2 4.2 0 0 1 0 6" />
          <path d="M18 6.5a7.6 7.6 0 0 1 0 11" />
        </>
      ) : null}
    </svg>
  );
}
