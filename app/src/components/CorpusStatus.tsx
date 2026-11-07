/**
 * 导航栏底部的状态点：有几首新歌、几首找不到音频、几首没分词。
 *
 * 原来这些提示散在顶部横幅、曲库左栏、导入页和设置里，每处一句。现在汇成一个点，
 * 点开是数量列表，每项跳到处理它的地方。什么都不缺时整个不显示。
 */

import { useEffect, useRef, useState } from "react";

import { command } from "../api";
import type { CorpusStatus as Counts } from "../generated/jp-api";
import { tr } from "../i18n";
import { Icon } from "./Icon";

interface Props {
  /** 变了就重新数（导入、移出、补分词之后） */
  version: number;
  /** 启动时查到的新歌（前端查的，不在后端的计数里） */
  newSongs: number;
  onNewSongs: () => void;
  onSources: () => void;
  onCorpusSettings: () => void;
}

export function CorpusStatus({ version, newSongs, onNewSongs, onSources, onCorpusSettings }: Props) {
  const [counts, setCounts] = useState<Counts | null>(null);
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let alive = true;
    // 数不出来不打扰人：这只是个提示
    command("corpus_status").then(
      (c) => alive && setCounts(c),
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, [version]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const items = [
    { key: "new", n: newSongs, label: tr("{n} 首新歌", { n: newSongs }), go: onNewSongs },
    { key: "audio", n: counts?.missingAudio ?? 0, label: tr("{n} 首找不到音频", { n: counts?.missingAudio ?? 0 }), go: onSources },
    { key: "tokens", n: counts?.untokenized ?? 0, label: tr("{n} 首没分词", { n: counts?.untokenized ?? 0 }), go: onCorpusSettings },
  ].filter((item) => item.n > 0);
  if (items.length === 0) return null;
  const total = items.reduce((sum, item) => sum + item.n, 0);

  return (
    <div className="corpus-status" ref={box}>
      <button
        className="nav-status"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        title={items.map((item) => item.label).join(" · ")}
      >
        <span className="status-dot-icon">
          <Icon name="status" />
          <i className="status-dot" aria-hidden="true" />
        </span>
        <span className="nav-label">{tr("待处理")}</span>
        <span className="status-count">{total}</span>
      </button>
      {open && (
        <div className="status-menu" role="menu">
          {items.map((item) => (
            <button
              key={item.key}
              role="menuitem"
              onClick={() => {
                setOpen(false);
                item.go();
              }}
            >
              {item.label}
              <Icon name="chevronRight" />
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
