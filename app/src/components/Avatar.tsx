/** Artist portraits use stable, centered identity marks when no image is available. */

import { avatarTone, monogram } from "../avatarIdentity";
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
      <span className={`avatar avatar-text avatar-tone-${avatarTone(name)}`} style={{ ...style, fontSize: Math.round(size * (monogram(name).length > 2 ? 0.27 : 0.34)) }} aria-hidden="true">
        <span className="avatar-monogram">{monogram(name)}</span>
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
