/**
 * KWIC 分布图（AntConc 的 Plot）：每首歌一条，命中的行在这首歌里的位置画成刻度。
 *
 * 一定**不去重**地再查一遍：去重会把副歌的几次出现折成一条，而分布图要看的恰恰是
 * 「这个词是散在全曲，还是挤在反复唱的那几行」。位置按行算（第几行 / 一共几行），
 * 不按时间——有些歌没有时间轴。点一个刻度跳到那一行接着放。
 */

import { useEffect, useMemo, useState } from "react";

import { api, type KwicHit, type KwicQuery } from "../api";
import { tr } from "../i18n";

interface Props {
  query: KwicQuery;
  onOpen: (hit: KwicHit) => void;
  onError: (message: string) => void;
}

interface Row {
  songId: string;
  label: string;
  lines: number;
  hits: KwicHit[];
}

export function KwicPlot({ query, onOpen, onError }: Props) {
  const [hits, setHits] = useState<KwicHit[] | null>(null);
  const key = JSON.stringify(query);

  useEffect(() => {
    let cancelled = false;
    setHits(null);
    api
      .kwic({ ...query, dedup: false })
      .then((h) => !cancelled && setHits(h))
      .catch((e) => !cancelled && onError(e instanceof Error ? e.message : String(e)));
    return () => {
      cancelled = true;
    };
    // key 就是 query 的内容
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, onError]);

  const rows = useMemo(() => {
    const bySong = new Map<string, Row>();
    for (const h of hits ?? []) {
      let row = bySong.get(h.songId);
      if (!row) {
        row = { songId: h.songId, label: `${h.artist} · ${h.title}`, lines: h.songLines, hits: [] };
        bySong.set(h.songId, row);
      }
      row.hits.push(h);
    }
    return [...bySong.values()].sort((a, b) => b.hits.length - a.hits.length || a.label.localeCompare(b.label, "ja"));
  }, [hits]);

  if (!hits) return <p className="muted pad">{tr("计算中…")}</p>;
  if (hits.length === 0) return <p className="muted pad">{tr("没有命中")}</p>;

  return (
    <div className="kwic-plot">
      <p className="muted small">
        {tr("{hits} 处 · {songs} 首（不去重）。每条是一首歌，从左到右是第一行到最后一行；点刻度从那一行放。", {
          hits: hits.length,
          songs: rows.length,
        })}
        {query.limit && hits.length >= query.limit ? tr("（已截断到 {limit} 条）", { limit: query.limit }) : ""}
      </p>
      {rows.map((row) => (
        <div className="plot-row" key={row.songId}>
          <span className="plot-label" title={row.label}>
            {row.label}
          </span>
          <span className="plot-count" title={tr("{hits} 处 / {lines} 行", { hits: row.hits.length, lines: row.lines })}>
            {row.hits.length}
          </span>
          <span className="plot-track">
            {row.hits.map((h, i) => (
              <button
                key={`${h.utteranceId}-${i}`}
                className="plot-tick"
                style={{ left: `${((h.linePos + 0.5) / Math.max(1, h.songLines)) * 100}%` }}
                title={`${h.left}【${h.keyword}】${h.right}`}
                aria-label={tr("第 {line} 行", { line: h.linePos + 1 })}
                onClick={() => onOpen(h)}
              />
            ))}
          </span>
        </div>
      ))}
    </div>
  );
}
