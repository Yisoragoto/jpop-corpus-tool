import type { ReactNode } from "react";
import { tr } from "../i18n";
import { validLyricTime } from "../wordTiming";

/** 点击真实单位时保留查词，并阻止外层「跳到行首」再次覆盖单位定位。 */
export function LyricUnit({ children, startSec, canSeek, onSeek, onLookup, charIndex, className }: {
  children: ReactNode;
  startSec: number | null;
  canSeek: boolean;
  onSeek: (sec: number) => void;
  onLookup?: (() => void) | undefined;
  charIndex: number;
  className: string;
}) {
  const seekable = canSeek && validLyricTime(startSec);
  const activate = () => {
    if (seekable) onSeek(startSec!);
    onLookup?.();
  };
  return <span className={className} data-c={charIndex}
    role={seekable && !onLookup ? "button" : undefined}
    tabIndex={seekable && !onLookup ? 0 : undefined}
    title={seekable ? onLookup ? tr("跳到真实歌词单位起点并查词") : tr("跳到真实歌词单位起点") : undefined}
    onClick={(event) => {
      if (seekable || onLookup) event.stopPropagation();
      activate();
    }}
    onKeyDown={(event) => {
      if (!onLookup && seekable && (event.key === "Enter" || event.key === " ")) {
        event.preventDefault(); event.stopPropagation(); activate();
      }
    }}>{children}</span>;
}
