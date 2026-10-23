/**
 * 时间轴：总览是每位演唱者一条泳道，发行按日期落在上面，**画的是封面**（专辑方、单曲圆，越大歌越多）；
 * 发行多的年份自动撑宽（`warpYears`），同一行的封面一张挨一张排开（`packRow`），不叠、不合并。乐队解散处一道竖杠，一条虚线接到单飞的成员那一行。
 *
 * 点名字（或者某张封面）放大成这位音乐人的时间树（`TimelineTree`，横竖可选）。
 * 放大了谁、横还是竖都记在模块里：去档案、去放歌再回来，还是刚才那棵树。
 */

import { useEffect, useMemo, useRef, useState } from "react";

import { api, type TimelineRelease } from "../api";
import { messageOf } from "../errors";
import { buildTimeline, packRow, warpYears } from "../timeline";
import { releaseKey, TimelineTree, type TreeOrient } from "./TimelineTree";
import { Avatar } from "./Avatar";
import { Cover } from "./Cover";
import type { DirectoryData } from "./LibraryBrowser";
import { tr } from "../i18n";

const ROW = 64;
const HEAD = 30;
const PAD = 40;
/** 同一行两张封面之间至少留这么宽 */
const GAP = 6;
const ZOOM_KEY = "jp.library.timelineZoom";

function readZoom(): number {
  try {
    const v = Number(localStorage.getItem(ZOOM_KEY));
    return v >= 30 && v <= 240 ? v : 90;
  } catch {
    return 90;
  }
}

const ORIENT_KEY = "jp.library.treeOrient";

function readOrient(): TreeOrient {
  try {
    return localStorage.getItem(ORIENT_KEY) === "horizontal" ? "horizontal" : "vertical";
  } catch {
    return "vertical";
  }
}

/** 放大了谁、定位到哪张：跟着模块走，回到时间轴时还在 */
const memory: { zoom: number | null; focus: string | null } = { zoom: null, focus: null };

/** 从某个人的档案打开：直接放大成他的时间树 */
export function focusTimelineOn(personId: number) {
  memory.zoom = personId;
  memory.focus = null;
}

/** 封面边长：专辑按歌数，单曲小一号 */
function coverSize(r: TimelineRelease): number {
  return r.albumId === null ? 24 : Math.min(44, 26 + r.tracks * 1.6);
}

interface Props {
  data: DirectoryData | null;
  version: number;
  playingSongId: string | null;
  onOpenPerson: (id: number) => void;
  onOpenAlbum: (personId: number, albumId: number) => void;
  /** 从头播放这首，人留在时间轴里 */
  onPlaySong: (songId: string) => void;
  onError: (message: string) => void;
}

export function TimelineView({ data, version, playingSongId, onOpenPerson, onOpenAlbum, onPlaySong, onError }: Props) {
  const [zoom, setZoomState] = useState(memory.zoom);
  const [focus, setFocus] = useState(memory.focus);
  const [orient, setOrientState] = useState<TreeOrient>(readOrient);
  const setZoom = (id: number | null, rel: string | null = null) => {
    memory.zoom = id;
    memory.focus = rel;
    setZoomState(id);
    setFocus(rel);
  };
  const setOrient = (o: TreeOrient) => {
    setOrientState(o);
    try {
      localStorage.setItem(ORIENT_KEY, o);
    } catch {
      // 只在这次会话里生效
    }
  };
  const [releases, setReleases] = useState<TimelineRelease[] | null>(null);
  const [query, setQuery] = useState("");
  const [perYear, setPerYear] = useState(readZoom);
  const [tip, setTip] = useState<{ r: TimelineRelease; x: number; y: number } | null>(null);
  const scroller = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let alive = true;
    api
      .timelineReleases()
      .then((r) => alive && setReleases(r))
      .catch((err: unknown) => {
        if (!alive) return;
        setReleases([]);
        onError(tr("读不出时间轴：{err}", { err: messageOf(err) }));
      });
    return () => {
      alive = false;
    };
  }, [version, onError]);

  useEffect(() => {
    try {
      localStorage.setItem(ZOOM_KEY, String(perYear));
    } catch {
      // 只在这次会话里生效
    }
  }, [perYear]);

  const layout = useMemo(
    () => (data === null || releases === null ? null : buildTimeline(data.dir, releases, query)),
    [data, releases, query],
  );

  // 第一次出来时滚到最近那几年
  const scrolled = useRef(false);
  useEffect(() => {
    if (layout === null || scrolled.current || scroller.current === null) return;
    scrolled.current = true;
    scroller.current.scrollLeft = scroller.current.scrollWidth;
  }, [layout]);

  if (layout === null || data === null || releases === null) return <p className="muted pad">{tr("加载中…")}</p>;

  if (zoom !== null) {
    return (
      <TimelineTree
        key={zoom}
        personId={zoom}
        data={data}
        releases={releases}
        orient={orient}
        onOrient={setOrient}
        focus={focus}
        playingSongId={playingSongId}
        onBack={() => setZoom(null)}
        onOpenPerson={onOpenPerson}
        onOpenAlbum={onOpenAlbum}
        onPlaySong={onPlaySong}
        onError={onError}
      />
    );
  }

  const { rows, connectors, min, max } = layout;
  const axis = warpYears(
    rows.map((r) => r.releases.map((rel) => ({ at: rel.at, size: coverSize(rel) }))),
    min,
    max,
    perYear,
    GAP,
  );
  const width = axis.width + PAD * 2;
  const x = (at: number) => PAD + axis.x(at);
  const y = (i: number) => HEAD + i * ROW + ROW / 2;
  const years: number[] = [];
  const step = perYear < 45 ? 5 : perYear < 70 ? 2 : 1;
  for (let yv = min; yv <= max; yv++) if (yv % step === 0) years.push(yv);
  const now = new Date().getFullYear() + new Date().getMonth() / 12;

  return (
    <div className="timeline-view">
      <div className="timeline-bar">
        <input className="search" placeholder={tr("筛选音乐人")} value={query} onChange={(e) => setQuery(e.target.value)} />
        <label className="graph-slider timeline-zoom">
          <span>{tr("缩放")}</span>
          <input type="range" min={30} max={240} value={perYear} onChange={(e) => setPerYear(Number(e.target.value))} />
        </label>
        <div className="timeline-legend muted small" aria-hidden="true">
          <span><i className="tl-cover" />{tr("专辑")}</span>
          <span><i className="tl-dot" />{tr("单曲（越大歌越多）")}</span>
          <span><i className="tl-end" />{tr("解散")}</span>
          <span><i className="tl-link" />{tr("之后单飞")}</span>
        </div>
      </div>

      {rows.length === 0 ? (
        <p className="muted pad">{query ? tr("没有匹配的音乐人") : tr("还没有带年份的歌。刮削补上发行日期之后这里就有了")}</p>
      ) : (
        <div className="timeline-scroll" ref={scroller} onScroll={() => tip && setTip(null)}>
          <div className="timeline-grid" style={{ height: HEAD + rows.length * ROW + 16 }}>
            <div className="timeline-names">
              <div className="timeline-corner" style={{ height: HEAD }} />
              {rows.map((r, i) => (
                <button
                  key={r.id}
                  className={`timeline-name ${r.under !== null ? "under" : ""}`}
                  style={{ height: ROW, ["--n" as string]: Math.min(i, 20) }}
                  onClick={() => setZoom(r.id)}
                  title={tr("放大看 {name} 的时间树", { name: r.name })}
                >
                  <Avatar path={r.imagePath} name={r.name} size={r.under !== null ? 26 : 32} />
                  <span className="timeline-name-text">
                    <b>{r.name}</b>
                    <small>
                      {r.isBand ? tr("乐队") : tr("个人")}{" "}{tr("· {length} 次发行", { length: r.releases.length })}</small>
                  </span>
                  <span className="timeline-zoom-in">{tr("放大 →")}</span>
                </button>
              ))}
            </div>
            <div className="timeline-plot" style={{ width }}>
              <div className="timeline-years" style={{ height: HEAD }}>
                {years.map((yv) => (
                  <span key={yv} style={{ left: x(yv) }}>
                    {yv}
                  </span>
                ))}
              </div>
              <svg width={width} height={HEAD + rows.length * ROW + 16} className="timeline-svg">
                {years.map((yv) => (
                  <line key={yv} x1={x(yv)} x2={x(yv)} y1={HEAD - 4} y2={HEAD + rows.length * ROW} className="tl-grid" />
                ))}
                {now <= max + 1 && <line x1={x(now)} x2={x(now)} y1={HEAD - 4} y2={HEAD + rows.length * ROW} className="tl-now" />}
                {rows.map((r, i) => (
                  <g key={r.id} className="tl-row" style={{ ["--n" as string]: Math.min(i, 20) }}>
                    {i % 2 === 1 && <rect x={0} y={HEAD + i * ROW} width={width} height={ROW} className="tl-stripe" />}
                    <line x1={x(r.start)} x2={x(r.end)} y1={y(i)} y2={y(i)} className={`tl-lane ${r.isBand ? "band" : ""}`} />
                    {r.endedAt !== null && (
                      <line x1={x(r.endedAt)} x2={x(r.endedAt)} y1={y(i) - 9} y2={y(i) + 9} className="tl-ended" />
                    )}
                  </g>
                ))}
                {connectors.map((c, k) => {
                  const x1 = x(c.fromAt);
                  const y1 = y(c.fromRow);
                  const x2 = x(c.toAt);
                  const y2 = y(c.toRow);
                  const mid = (x1 + x2) / 2;
                  return (
                    <path
                      key={k}
                      d={`M${x1},${y1} C${mid},${y1} ${mid},${y2} ${x2},${y2}`}
                      className="tl-connector"
                      style={{ ["--n" as string]: k }}
                    />
                  );
                })}
              </svg>
              {rows.map((r, i) =>
                packRow(r.releases.map((rel) => ({ x: x(rel.at), size: coverSize(rel) })), GAP).map((cx, index) => {
                  const rel = r.releases[index]!;
                  const size = coverSize(rel);
                  return (
                    <button
                      key={`${r.id}-${releaseKey(rel)}`}
                      className={`tl-cover-btn ${rel.albumId === null ? "single" : ""}`}
                      style={{
                        left: cx,
                        top: y(i),
                        ["--d" as string]: `${Math.min(i, 20) * 40 + Math.min(index, 14) * 45 + 300}ms`,
                      }}
                      onMouseEnter={(e) => {
                        const box = (e.currentTarget.closest(".timeline-view") as HTMLElement).getBoundingClientRect();
                        const dot = e.currentTarget.getBoundingClientRect();
                        setTip({ r: rel, x: dot.left - box.left + dot.width / 2, y: dot.top - box.top });
                      }}
                      onMouseLeave={() => setTip(null)}
                      onClick={() => setZoom(r.id, releaseKey(rel))}
                      aria-label={`${rel.title}${tr("，")}${rel.date}`}
                    >
                      <Cover path={rel.coverPath} size={size} rounded={rel.albumId === null ? size : 5} />
                    </button>
                  );
                }),
              )}
            </div>
          </div>
        </div>
      )}

      {tip !== null && (
        <div className="timeline-tip glass" style={{ left: tip.x, top: tip.y }}>
          <Cover path={tip.r.coverPath} size={40} rounded={6} />
          <span>
            <b>{tip.r.title}</b>
            <small>{tr("{date} · {tracks} 首", { date: tip.r.date, tracks: tip.r.tracks })}</small>
          </span>
        </div>
      )}
    </div>
  );
}
