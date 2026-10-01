import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

/**
 * 封面图。
 *
 * 封面是磁盘上的普通文件（`raw/covers/<歌手>/<曲目号>.jpg`），webview 不能
 * 直接用 `file://` 读——要走 Tauri 的 asset 协议，并且目录得在 scope 里
 * （见 `jp-app/src/lib.rs` 的 setup）。`convertFileSrc` 负责把本地路径
 * 转成那个协议的 URL。
 *
 * 没有封面、或者文件被手工删掉了，都退回占位块。**不显示裂图**：
 * 209 首里有 9 首本来就没匹配到封面，裂图会让人以为是程序坏了。
 */
export function Cover({
  path,
  size = 40,
  alt = "",
  rounded = 4,
}: {
  /** `songs.cover_path` 或 `albums.artwork_path`，空字符串表示没有 */
  path: string | null | undefined;
  size?: number;
  alt?: string;
  rounded?: number;
}) {
  const [broken, setBroken] = useState(false);

  // 换歌时要把上一张的失败状态清掉，否则一次读不到就永远是占位块
  useEffect(() => setBroken(false), [path]);

  const style = { width: size, height: size, borderRadius: rounded };

  if (!path || broken) {
    return <span className="cover cover-empty" style={style} aria-hidden="true" />;
  }

  return (
    <img
      className="cover"
      style={style}
      src={convertFileSrc(path)}
      alt={alt}
      width={size}
      height={size}
      loading="lazy"
      draggable={false}
      onError={() => setBroken(true)}
    />
  );
}
