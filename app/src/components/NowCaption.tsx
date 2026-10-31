/**
 * 「正在唱」：底部一行字幕，显示正在放的那一句。
 *
 * 词云下面原来就有这一行；现在每个页面都有。人在分析、词典、检索页点歌开始放，
 * 不用切去资料库也看得见唱到哪一句，最近在「词的去处」里打开的那个词会标亮。
 * 右边两个按钮：回到这一行（去资料库，滚到这句）、全屏歌词（退出后回到当前页面）。
 *
 * 资料库自己有歌词栏和词云字幕，那里不显示这一条（App 决定）。
 */

import { useMemo } from "react";

import type { LyricLine, Track } from "../api";
import { useFocusWord } from "../focusWord";
import { tr } from "../i18n";

interface Props {
  track: Track;
  line: LyricLine;
  onReveal: () => void;
  onFullscreen: () => void;
}

export function NowCaption({ track, line, onReveal, onFullscreen }: Props) {
  const focus = useFocusWord();
  const parts = useMemo(() => {
    if (line.tokens.length === 0 || focus.length === 0) return [{ text: line.text, hit: false }];
    return line.tokens.map((t) => ({ text: t.surface, hit: focus.includes(t.lemma) }));
  }, [line, focus]);
  return (
    <div className="now-caption" role="region" aria-label={tr("正在播放的这一句")}>
      <div className="now-caption-meta">
        <strong>{track.title}</strong>
        <small>{track.artist}</small>
      </div>
      <p className="now-caption-line" lang="ja" key={line.utteranceId}>
        {parts.map((p, i) => (p.hit ? <mark key={i}>{p.text}</mark> : <span key={i}>{p.text}</span>))}
      </p>
      <div className="now-caption-actions">
        <button className="link-btn" onClick={onReveal} title={tr("去资料库，滚到这一句")}>{tr("回到这一行")}</button>
        <button className="icon-btn" onClick={onFullscreen} title={tr("全屏歌词（退出后回到这里）")} aria-label={tr("全屏歌词")}>
          <svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
            <path d="M2 6V2h4M14 6V2h-4M2 10v4h4M14 10v4h-4" />
          </svg>
        </button>
      </div>
    </div>
  );
}
