/**
 * 全屏歌词。对应 Python 版曲库的「展开歌词播放器」（`LyricsPlayerView`）：
 *
 * - 铺满整个窗口，**播放条盖在它上面**：播放条是半透明的，底下这张虚化封面会透上来，
 *   这样底部那一条和上面才是一体的（倍速、音调、进度都在播放条里，不再做一份）；
 * - 当前行放大加粗、停在屏幕中间偏上，换行时平滑滚过去；滚轮可以自己翻，下一次换行再回到当前行；
 * - 点一行跳到那一句；点词在右边查词（和曲库页右栏同一套内容），可以制卡；
 * - 右上角同样有振假名开关和「显示」面板；Esc 退出（「显示」面板开着时先收起面板）。
 *
 * 行的内容（振假名、可点的词）由曲库页渲染后传进来，和普通歌词视图用同一份代码。
 *
 * 底色取自封面（`coverColor`），照 Spotify 的全屏歌词。设置里可以改成**整张封面虚化铺满**
 * （`stageCoverBlur`，默认开）；没有封面、或者封面读不出来，都退回纯色。
 * **整块是不透明的**：外壳为了云母留了透明度，全屏这一层要是跟着透，下面的页面会整个透上来。
 */

import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

import type { LyricLine } from "../api";
import { useCoverColor } from "../coverColor";
import type { LyricsDisplay } from "../lyricsDisplay";
import { useAppSettings } from "../settings";
import { Cover } from "./Cover";
import { LyricsTools } from "./LyricsTools";
import { ColumnSplitter, usePaneWidth } from "./ColumnSplitter";

interface Props {
  title: string;
  meta: string;
  coverPath: string | null | undefined;
  lines: LyricLine[];
  currentLine: number;
  renderText: (index: number) => ReactNode;
  onSeekLine: (index: number) => void;
  canSeek: boolean;
  display: LyricsDisplay;
  withFurigana: boolean;
  style: CSSProperties;
  /** 右侧查词内容；null 时不显示侧栏 */
  side: ReactNode | null;
  onCloseSide: () => void;
  onError: (message: string) => void;
  onClose: () => void;
}

/** 当前行停在可视高度的这个位置（Python 版是 0.48） */
const ANCHOR = 0.45;

/** 右侧那一栏可拖的范围。再窄查词的释义表就排不开，再宽歌词那边就不够看了 */
const SIDE_BOUNDS = { min: 260, max: 720, restMin: 420 };

export function LyricsStage(props: Props) {
  const { lines, currentLine, onClose } = props;
  const scroller = useRef<HTMLDivElement>(null);
  const firstScroll = useRef(true);
  const playerHeight = usePlayerHeight();
  const coverColor = useCoverColor(props.coverPath);
  const settings = useAppSettings();
  // 右边那一栏可以拖，并且记住（和曲库页的两道缝同一套）
  const side = usePaneWidth("stage-side", 340, SIDE_BOUNDS);
  // 封面读不出来时退回纯色。换歌要把失败状态清掉，否则一次读不到就再也不显示了
  const [bgBroken, setBgBroken] = useState(false);
  useEffect(() => setBgBroken(false), [props.coverPath]);
  const blurCover = settings.stageCoverBlur && !bgBroken && Boolean(props.coverPath);

  // Esc 退出；「显示」面板开着时那一下留给面板
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && document.querySelector(".stage .lyrics-display-panel") === null) onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  // 当前行居中：刚打开时直接跳过去，之后平滑滚
  useLayoutEffect(() => {
    const box = scroller.current;
    if (box === null) return;
    const line = currentLine >= 0 ? box.querySelector<HTMLElement>(`[data-idx="${currentLine}"]`) : null;
    const top = line === null ? 0 : line.offsetTop + line.offsetHeight / 2 - box.clientHeight * ANCHOR;
    box.scrollTo({ top, behavior: firstScroll.current ? "auto" : "smooth" });
    firstScroll.current = false;
  }, [currentLine, lines, props.display.fontSize, props.withFurigana]);

  return (
    <div
      className={`stage ${props.withFurigana ? "with-furigana" : ""} ${blurCover ? "with-cover-bg" : ""}`}
      style={{
        ...props.style,
        // 铺到窗口底边（播放条盖在上面），内容再按播放条的高度让开
        bottom: 0,
        ["--player-h" as string]: `${playerHeight}px`,
        ["--stage-color" as string]: coverColor ?? "#1a1a20",
      }}
      role="dialog"
      aria-label="全屏歌词"
    >
      {blurCover && (
        <div className="stage-bg" aria-hidden="true">
          <img
            src={convertFileSrc(props.coverPath as string)}
            alt=""
            style={{ filter: `blur(${settings.stageBlurRadius}px)` }}
            onError={() => setBgBroken(true)}
            draggable={false}
          />
        </div>
      )}
      <header className="stage-head">
        <div className="stage-title">
          <h1>{props.title}</h1>
          <p>{props.meta}</p>
        </div>
        <LyricsTools display={props.display} onError={props.onError} />
        <button className="stage-close" onClick={onClose} title="退出全屏（Esc）" aria-label="退出全屏">
          ✕
        </button>
      </header>
      <div className="stage-body" ref={side.container}>
        <div className="stage-lyrics" ref={scroller}>
          {lines.length === 0 && <p className="muted stage-empty">这首歌还没有歌词</p>}
          {lines.map((line, idx) => (
            <p
              key={line.utteranceId}
              data-idx={idx}
              className={`stage-line ${idx === currentLine ? "current" : ""} ${props.canSeek && line.timeSec !== null ? "seekable" : ""}`}
              onClick={() => props.onSeekLine(idx)}
              title={props.canSeek && line.timeSec !== null ? "跳到这一句" : undefined}
            >
              {props.renderText(idx)}
            </p>
          ))}
        </div>
        <ColumnSplitter
          edge="right"
          label="查词栏宽度"
          onDrag={side.drag}
          onNudge={side.nudge}
          onReset={side.reset}
        />
        <aside className="stage-side" style={side.style}>
          {props.side === null ? (
            /* 没在查词时放这首歌本身：封面、歌名、歌手和署名 */
            <div className="stage-card">
              <Cover path={props.coverPath} size={300} alt="" rounded={10} />
              <h2>{props.title}</h2>
              <p>{props.meta}</p>
            </div>
          ) : (
            <>
              <button className="stage-side-close" onClick={props.onCloseSide} title="收起" aria-label="收起查词">
                ✕
              </button>
              {props.side}
            </>
          )}
        </aside>
      </div>
    </div>
  );
}

/** 底部播放条的高度：全屏歌词盖到它上面为止 */
function usePlayerHeight(): number {
  const [height, setHeight] = useState(() => document.querySelector("footer.player")?.getBoundingClientRect().height ?? 0);
  useEffect(() => {
    const footer = document.querySelector("footer.player");
    if (footer === null) return;
    const observer = new ResizeObserver(() => setHeight(footer.getBoundingClientRect().height));
    observer.observe(footer);
    return () => observer.disconnect();
  }, []);
  return height;
}
