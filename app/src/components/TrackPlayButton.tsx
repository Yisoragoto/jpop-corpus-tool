/**
 * 曲目行上单独的那颗「播放」：点它才换正在放的歌，点行本身只是打开歌词看。
 *
 * 和行是兄弟、不是嵌在行里（按钮里不能套按钮），用 CSS 叠在封面或序号上。
 * 正在放的那首显示音符，再点一下是暂停 / 继续。
 */

import { api } from "../api";
import { tr } from "../i18n";
import { Icon } from "./Icon";

export function TrackPlayButton({
  title,
  playing,
  current,
  onPlay,
  className = "",
}: {
  title: string;
  /** 这首正在放（在走） */
  playing: boolean;
  /** 播放条里是这首（放着或暂停着） */
  current: boolean;
  onPlay: () => void;
  className?: string;
}) {
  return (
    <button
      type="button"
      className={`track-play ${current ? "current" : ""} ${className}`.trim()}
      onClick={(e) => {
        e.stopPropagation();
        if (current) void api.audioToggle();
        else onPlay();
      }}
      title={playing ? tr("暂停") : tr("播放「{title}」", { title })}
      aria-label={playing ? tr("暂停「{title}」", { title }) : tr("播放「{title}」", { title })}
    >
      <Icon name={playing ? "pauseFilled" : "playFilled"} size={18} />
    </button>
  );
}
