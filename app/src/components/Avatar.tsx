/**
 * 人的头像。有照片就是照片，没有就画名字的第一个字。
 *
 * 为什么不直接用空的占位圆：一列人下来全是一样的灰圆，看着像坏了；
 * 而且作词作曲这些角色**本来就没有照片**（刮削只查演唱者），
 * 那不是故障，是数据范围。写个首字出来，至少每个人长得不一样。
 */

import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

/** 名字的第一个「字」。英文名取首字母大写，日文中文取第一个字 */
export function initial(name: string): string {
  const trimmed = name.trim();
  if (trimmed === "") return "?";
  const first = [...trimmed][0] ?? "?";
  return /[a-z]/.test(first) ? first.toUpperCase() : first;
}

export function Avatar({
  path,
  name,
  size = 36,
}: {
  path: string | null | undefined;
  name: string;
  size?: number;
}) {
  const [broken, setBroken] = useState(false);
  useEffect(() => setBroken(false), [path]);

  const style = { width: size, height: size, borderRadius: "50%" } as const;
  if (!path || broken) {
    return (
      <span className="avatar avatar-text" style={{ ...style, fontSize: Math.round(size * 0.42) }} aria-hidden="true">
        {initial(name)}
      </span>
    );
  }
  return (
    <img
      className="avatar"
      style={style}
      src={convertFileSrc(path)}
      alt=""
      width={size}
      height={size}
      loading="lazy"
      draggable={false}
      onError={() => setBroken(true)}
    />
  );
}
