/**
 * 时间轴放大后：一位音乐人的「时间树」。竖着看（年份是树干上的节，专辑左右交替长出来，歌是小枝），
 * 或者横着看（年份在上，专辑挂在横向主干上，点一张往下长出它的歌）。横竖记住。
 *
 * 乐队解散了，树干在解散那里换成单飞成员的颜色接着长（只接解散之后的个人作品）。排法在 `timeline.ts` 的 `buildTree`。
 */

import { useEffect, useMemo, useRef, useState } from "react";

import { api, type PersonWork, type TimelineRelease } from "../api";
import { messageOf } from "../errors";
import { yearOf } from "../libraryIndex";
import { buildTree, type TreePart, type TreeRelease } from "../timeline";
import { prefersReducedMotion } from "../motion";
import { Avatar } from "./Avatar";
import { Cover } from "./Cover";
import type { DirectoryData } from "./LibraryBrowser";
import { tr } from "../i18n";

export type TreeOrient = "vertical" | "horizontal";

const SHOW_SONGS = 6;

interface Props {
  personId: number;
  data: DirectoryData;
  releases: TimelineRelease[];
  orient: TreeOrient;
  onOrient: (o: TreeOrient) => void;
  /** 进来时要定位的那张（总览里点的封面） */
  focus: string | null;
  playingSongId: string | null;
  onBack: () => void;
  onOpenPerson: (id: number) => void;
  onOpenAlbum: (personId: number, albumId: number) => void;
  onPlaySong: (songId: string) => void;
  onError: (message: string) => void;
}

/** 一张发行的稳定 key：专辑按 id，单曲按歌 */
export function releaseKey(r: TimelineRelease): string {
  return r.albumId !== null ? `a${r.albumId}` : `s${r.songId}`;
}

export function TimelineTree({
  personId,
  data,
  releases,
  orient,
  onOrient,
  focus,
  playingSongId,
  onBack,
  onOpenPerson,
  onOpenAlbum,
  onPlaySong,
  onError,
}: Props) {
  const { dir } = data;
  const entry = dir.byId.get(personId);
  // 要拿歌名的人：自己，加上解散后接上来的前成员
  const people = useMemo(() => {
    const ids = [personId];
    for (const m of dir.membersOf.get(personId) ?? []) if (m.leftAt) ids.push(m.memberId);
    return ids;
  }, [dir, personId]);
  const [works, setWorks] = useState<Map<number, PersonWork[]> | null>(null);
  useEffect(() => {
    let alive = true;
    setWorks(null);
    Promise.all(people.map((id) => api.personWorks(id).then((w) => [id, w] as const)))
      .then((pairs) => alive && setWorks(new Map(pairs)))
      .catch((err: unknown) => {
        if (!alive) return;
        setWorks(new Map());
        onError(tr("读不出作品：{err}", { err: messageOf(err) }));
      });
    return () => {
      alive = false;
    };
  }, [people, onError]);

  const parts = useMemo(() => (works === null ? null : buildTree(personId, dir, releases, works)), [personId, dir, releases, works]);
  const total = parts?.reduce((n, p) => n + p.items.length, 0) ?? 0;
  const isBand = (dir.membersOf.get(personId)?.length ?? 0) > 0 || entry?.artistType === "Group";
  const first = parts?.[0]?.items[0]?.release.date ?? "";
  const scroller = useRef<HTMLDivElement>(null);

  // 从总览点了某张封面进来：滚到那一张，闪一下
  useEffect(() => {
    if (focus === null || parts === null) return;
    const el = scroller.current?.querySelector<HTMLElement>(`[data-rel="${CSS.escape(focus)}"]`);
    if (!el) return;
    el.scrollIntoView({ block: "center", inline: "center", behavior: prefersReducedMotion() ? "auto" : "smooth" });
    el.classList.add("flash");
  }, [focus, parts, orient]);

  return (
    <div className="tree-view">
      <div className="timeline-bar">
        <div className="tree-crumb">
          <button className="link-btn" onClick={onBack}>{tr("← 全部音乐人")}</button>
          <span>/</span>
          <b>{entry?.name ?? ""}</b>
        </div>
        <div className="segmented seg-slide tree-orient" style={{ ["--seg-i" as string]: orient === "vertical" ? 0 : 1, ["--seg-n" as string]: 2 }}>
          <span className="seg-thumb" aria-hidden="true" />
          <button className={orient === "vertical" ? "on" : ""} onClick={() => onOrient("vertical")}>{tr("竖向")}</button>
          <button className={orient === "horizontal" ? "on" : ""} onClick={() => onOrient("horizontal")}>{tr("横向")}</button>
        </div>
        <div className="timeline-legend muted small" aria-hidden="true">
          <span><i className="tl-node" />{isBand ? tr("乐队时期") : tr("发行")}</span>
          {parts !== null && parts.length > 1 && <span><i className="tl-node solo" />{tr("解散后单飞")}</span>}
        </div>
      </div>

      <div className={`tree-scroll ${orient}`} ref={scroller} key={orient}>
        <header className="tree-hero">
          <Avatar path={entry?.imagePath} name={entry?.name ?? ""} size={64} />
          <div>
            <h2>{entry?.name}</h2>
            <p className="muted">
              {[isBand ? tr("乐队") : tr("个人"), first && `${yearOf(first)}–${entry?.ended ? yearOf(entry.ended) : tr("至今")}`, tr("{total} 次发行", { total })]
                .filter(Boolean)
                .join(" · ")}
              {parts !== null && parts.length > 1 && tr(" · 之后 {map} 单飞", { map: parts.slice(1).map((p) => p.name).join(tr("、")) })}
            </p>
          </div>
          <button className="btn" onClick={() => onOpenPerson(personId)}>{tr("打开档案")}</button>
        </header>
        {parts === null && <p className="muted pad">{tr("加载中…")}</p>}
        {parts !== null && total === 0 && <p className="muted pad">{tr("没有带日期的发行。刮削补上发行日期之后这里就有了")}</p>}
        {parts !== null && total > 0 && orient === "vertical" && (
          <VerticalTree parts={parts} playing={playingSongId} onOpenAlbum={onOpenAlbum} onPlaySong={onPlaySong} />
        )}
        {parts !== null && total > 0 && orient === "horizontal" && (
          <HorizontalTree parts={parts} focus={focus} playing={playingSongId} onOpenAlbum={onOpenAlbum} onPlaySong={onPlaySong} />
        )}
      </div>
    </div>
  );
}

function SongTwig({ song, n, playing, onPlay }: { song: PersonWork; n: number; playing: boolean; onPlay: () => void }) {
  return (
    <button className={`twig ${playing ? "now" : ""}`} onClick={onPlay} title={tr("从头播放")}>
      <em>{playing ? "♪" : n}</em>
      <span lang="ja">{song.title}</span>
      <span className="twig-play">▶</span>
    </button>
  );
}

/* ── 竖：年份是树干上的节，专辑左右交替 ── */
function VerticalTree({
  parts,
  playing,
  onOpenAlbum,
  onPlaySong,
}: {
  parts: TreePart[];
  playing: string | null;
  onOpenAlbum: (personId: number, albumId: number) => void;
  onPlaySong: (songId: string) => void;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  let side = 0;
  let delay = 0;
  return (
    <div className="vtree">
      {parts.map((part) => {
        let lastYear = "";
        return (
          <section key={part.personId} className={`vtree-part ${part.after ? "solo" : ""}`}>
            {part.after && (
              <div className="split-card">
                <b>{tr("{ended} 年 {bandName} 解散", { ended: yearOf(part.after.ended), bandName: part.after.bandName })}</b>
                <span className="split-who">
                  <Avatar path="" name={part.name} size={22} />{tr("{name} 的个人作品从这里接着长", { name: part.name })}</span>
              </div>
            )}
            {part.items.map((item) => {
              const y = yearOf(item.release.date);
              const node = y !== lastYear;
              lastYear = y;
              const s = side++ % 2 ? "r" : "l";
              const key = releaseKey(item.release);
              const open = expanded.has(key);
              const shown = open ? item.songs : item.songs.slice(0, SHOW_SONGS);
              return (
                <div key={key}>
                  {node && (
                    <div className="year-node">
                      <span>{y}</span>
                    </div>
                  )}
                  <article className={`vtree-alb ${s}`} data-rel={key} style={{ ["--d" as string]: `${(delay += 60)}ms` }}>
                    <AlbumCover item={item} size={item.release.albumId === null ? 56 : 84} onOpenAlbum={onOpenAlbum} />
                    <div className="vtree-body">
                      <h3 lang="ja">{item.release.title}</h3>
                      <p className="vtree-meta">
                        {item.release.date} · {item.release.albumId === null ? tr("单曲") : tr("{tracks} 首", { tracks: item.release.tracks })}
                        {part.after && ` · ${part.name}`}
                      </p>
                      <div className="twigs">
                        {shown.map((song, k) => (
                          <SongTwig key={song.id} song={song} n={k + 1} playing={playing === song.id} onPlay={() => onPlaySong(song.id)} />
                        ))}
                        {item.songs.length > SHOW_SONGS && (
                          <button
                            className="link-btn twig-more"
                            onClick={() =>
                              setExpanded((cur) => {
                                const next = new Set(cur);
                                if (next.has(key)) next.delete(key);
                                else next.add(key);
                                return next;
                              })
                            }
                          >
                            {open ? tr("收起") : tr("还有 {v} 首", { v: item.songs.length - SHOW_SONGS })}
                          </button>
                        )}
                      </div>
                    </div>
                  </article>
                </div>
              );
            })}
          </section>
        );
      })}
    </div>
  );
}

function AlbumCover({ item, size, onOpenAlbum }: { item: TreeRelease; size: number; onOpenAlbum: (personId: number, albumId: number) => void }) {
  const albumId = item.release.albumId;
  return albumId === null ? (
    <span className="tree-cover single">
      <Cover path={item.release.coverPath} size={size} rounded={size} />
    </span>
  ) : (
    <button className="tree-cover" onClick={() => onOpenAlbum(item.personId, albumId)} title={tr("在档案里看这张专辑")}>
      <Cover path={item.release.coverPath} size={size} rounded={8} />
    </button>
  );
}

/* ── 横：年份在上，专辑挂在主干上，点一张往下长出它的歌 ── */
const PER_YEAR = 150;
const PAD = 70;
const NODE_W = 128;
const OPEN_W = 200;

function HorizontalTree({
  parts,
  focus,
  playing,
  onOpenAlbum,
  onPlaySong,
}: {
  parts: TreePart[];
  focus: string | null;
  playing: string | null;
  onOpenAlbum: (personId: number, albumId: number) => void;
  onPlaySong: (songId: string) => void;
}) {
  const items = useMemo(() => parts.flatMap((p) => p.items.map((item) => ({ item, part: p }))), [parts]);
  // 默认展开定位的那张；没有就是第一张有歌的
  const [open, setOpen] = useState<Set<string>>(() => {
    if (focus) return new Set([focus]);
    const first = items.find((i) => i.item.songs.length > 0);
    return new Set(first ? [releaseKey(first.item.release)] : []);
  });
  const ats = items.map((i) => i.item.release.at);
  const lo = Math.floor(Math.min(...ats));
  const hi = Math.ceil(Math.max(...ats)) + 1;
  const x = (t: number) => PAD + (t - lo) * PER_YEAR;
  // 挨得太近的往右让开（展开的那张让得更多），再斜着连回主干上的真实日期
  const nodes: { key: string; tx: number; cx: number; item: TreeRelease; part: TreePart; isOpen: boolean }[] = [];
  let prev = -Infinity;
  let prevOpen = false;
  for (const { item, part } of items) {
    const key = releaseKey(item.release);
    const tx = x(item.release.at);
    const cx = Math.max(tx, prev + (prevOpen ? OPEN_W : NODE_W));
    const isOpen = open.has(key);
    nodes.push({ key, tx, cx, item, part, isOpen });
    prev = cx;
    prevOpen = isOpen;
  }
  const width = Math.max(x(hi), (nodes[nodes.length - 1]?.cx ?? 0) + OPEN_W) + PAD;
  const trunkY = 40;
  const nodeY = 76;
  const longest = Math.max(0, ...nodes.filter((n) => n.isOpen).map((n) => n.item.songs.length));
  const height = nodeY + 190 + longest * 30 + 40;
  const split = parts[1]?.after ? x(parts[1].items[0]?.release.at ?? hi) - 30 : null;
  const years: number[] = [];
  for (let y = lo; y <= hi; y++) years.push(y);
  const toggle = (key: string) =>
    setOpen((cur) => {
      const next = new Set(cur);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  return (
    <div className="htree" style={{ width, height }}>
      <div className="htree-years">
        {years.map((y) => (
          <span key={y} style={{ left: x(y) }}>
            {y}
          </span>
        ))}
      </div>
      <svg className="htree-lines" width={width} height={height}>
        <path d={`M${PAD - 30},${trunkY} L${split ?? width - PAD},${trunkY}`} className="ht-trunk" />
        {split !== null && <path d={`M${split},${trunkY} L${width - PAD},${trunkY}`} className="ht-trunk solo" />}
        {split !== null && <circle cx={split} cy={trunkY} r={6} className="ht-split" />}
        {nodes.map((n) => {
          const solo = n.part.after !== null;
          const stem = n.cx - 52;
          const sy = nodeY + 172;
          const last = sy + (n.item.songs.length - 1) * 30 + 13;
          return (
            <g key={n.key} className={solo ? "solo" : ""}>
              <circle cx={n.tx} cy={trunkY} r={4} className="ht-dot" />
              <path d={`M${n.tx},${trunkY} C${n.tx},${trunkY + 22} ${n.cx},${nodeY - 22} ${n.cx},${nodeY}`} className="ht-branch" />
              {n.isOpen && n.item.songs.length > 0 && (
                <>
                  <path
                    d={`M${n.cx},${nodeY + (n.item.release.albumId === null ? 118 : 150)} C${n.cx},${sy - 6} ${stem},${sy - 14} ${stem},${sy} L${stem},${last}`}
                    className="ht-branch"
                  />
                  {n.item.songs.map((_, j) => {
                    const yy = sy + j * 30 + 13;
                    return <path key={j} d={`M${stem},${yy - 9} Q${stem},${yy} ${stem + 11},${yy}`} className="ht-branch" />;
                  })}
                </>
              )}
            </g>
          );
        })}
      </svg>
      {nodes.map((n, k) => (
        <div
          key={n.key}
          className={`htree-node ${n.isOpen ? "open" : ""}`}
          data-rel={n.key}
          style={{ left: n.cx, top: nodeY, ["--d" as string]: `${k * 50}ms` }}
        >
          <button className={`htree-cover ${n.item.release.albumId === null ? "single" : ""}`} onClick={() => toggle(n.key)} title={n.isOpen ? tr("收起这张的歌") : tr("展开这张的歌")} aria-expanded={n.isOpen}>
            <Cover path={n.item.release.coverPath} size={n.item.release.albumId === null ? 60 : 84} rounded={n.item.release.albumId === null ? 60 : 8} />
          </button>
          <b lang="ja">{n.item.release.title}</b>
          <small>
            {n.item.release.date.slice(0, 7)}
            {n.part.after && ` · ${n.part.name}`}
          </small>
          {n.item.release.albumId !== null && (
            <button className="link-btn htree-open" onClick={() => onOpenAlbum(n.item.personId, n.item.release.albumId!)}>{tr("档案 →")}</button>
          )}
        </div>
      ))}
      {nodes
        .filter((n) => n.isOpen)
        .map((n) => (
          <div key={`s-${n.key}`} className="htree-songs" style={{ left: n.cx - 40, top: nodeY + 172 }}>
            {n.item.songs.map((song, j) => (
              <button
                key={song.id}
                className={`htree-song ${playing === song.id ? "now" : ""}`}
                style={{ ["--d" as string]: `${j * 35}ms` }}
                onClick={() => onPlaySong(song.id)}
                title={tr("从头播放")}
                lang="ja"
              >
                {playing === song.id ? "♪ " : ""}
                {song.title}
              </button>
            ))}
          </div>
        ))}
    </div>
  );
}
