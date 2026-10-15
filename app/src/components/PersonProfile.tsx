/**
 * 资料库中栏的「人」：档案头（照片、角色、所属乐队、数字）、成员卡片、按专辑排的作品。
 * 右栏的合作者也在这里（`PersonSide`）。
 *
 * 原来的人物页按「演唱 / 作曲 / 作词 / 编曲」四个列表分人，同一个人在每个列表里各出现一次；
 * 现在一个人只有一页，他做过哪些角色写在档案头上。
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

import { api, ROLE_LABELS, type Album, type Collaborator, type DirectoryEntry, type Membership, type PersonWork, type Track } from "../api";
import { messageOf } from "../errors";
import { rolesOf, sectionWorks, spanOf, type WorkSection } from "../libraryIndex";
import { prefersReducedMotion } from "../motion";
import { Avatar } from "./Avatar";
import { Cover } from "./Cover";
import { MembersEditor } from "./MembersEditor";
import { RelationGraph } from "./RelationGraph";
import { buildGraph } from "../graph";
import { ROLE_COLORS, type DirectoryData } from "./LibraryBrowser";

/** 没有成员关系时的空列表。用同一个引用：每次新建 [] 会让依赖它的 effect 每帧重跑 */
const NONE: Membership[] = [];

export interface AlbumGroup {
  key: string;
  albumId: number | null;
  title: string;
  year: string;
  coverPath: string;
  tracks: Track[];
}

/** 一首歌归到哪一组：有专辑 id 按 id，只有专辑名按名字，都没有算「单曲」 */
export function albumKey(t: Track): string {
  return t.albumId !== null ? `id:${t.albumId}` : t.album ? `t:${t.album}` : "single";
}

/** 作品按专辑分组，专辑按年份；没有专辑的歌各自归到「单曲」 */
export function groupByAlbum(tracks: Track[]): AlbumGroup[] {
  const groups = new Map<string, AlbumGroup>();
  for (const t of tracks) {
    const key = albumKey(t);
    let g = groups.get(key);
    if (g === undefined) {
      g = { key, albumId: t.albumId, title: t.album || "单曲", year: t.year, coverPath: t.coverPath, tracks: [] };
      groups.set(key, g);
    }
    // 同一首歌可能因为几个角色重复出现
    if (!g.tracks.some((x) => x.id === t.id)) g.tracks.push(t);
    if (t.year && (!g.year || t.year < g.year)) g.year = t.year;
    if (!g.coverPath && t.coverPath) g.coverPath = t.coverPath;
  }
  return [...groups.values()].sort((a, b) => (a.year || "9999").localeCompare(b.year || "9999") || a.title.localeCompare(b.title, "ja"));
}

interface ProfileProps {
  personId: number;
  data: DirectoryData | null;
  /** 要定位的专辑（从左栏「专辑」点进来的） */
  focusAlbumId: number | null;
  playingSongId: string | null;
  onOpenPerson: (id: number) => void;
  onOpenTrack: (track: Track, from: HTMLElement) => void;
  onPlayAll: (tracks: Track[]) => void;
  /** 改了成员：目录要重拉 */
  onMembersChanged: () => void;
  onError: (message: string) => void;
}

export function PersonProfile({
  personId,
  data,
  focusAlbumId,
  playingSongId,
  onOpenPerson,
  onOpenTrack,
  onPlayAll,
  onMembersChanged,
  onError,
}: ProfileProps) {
  const [works, setWorks] = useState<PersonWork[] | null>(null);
  // 目录里没有的人（只在成员表里、一首署名都没有）也要能看：按 id 补一份
  const [fallback, setFallback] = useState<{ id: number; name: string; imagePath: string } | null>(null);
  const entry: DirectoryEntry | undefined = data?.dir.byId.get(personId);

  useEffect(() => {
    let alive = true;
    setWorks(null);
    api
      .personWorks(personId)
      .then((w) => alive && setWorks(w))
      .catch((err: unknown) => {
        if (!alive) return;
        setWorks([]);
        onError(`读不出作品：${messageOf(err)}`);
      });
    return () => {
      alive = false;
    };
  }, [personId, onError]);

  const members = data?.dir.membersOf.get(personId) ?? NONE;
  const bands = data?.dir.bandsOf.get(personId) ?? NONE;
  useEffect(() => {
    if (entry !== undefined || data === null) return setFallback(null);
    const asMember = bands[0];
    if (asMember !== undefined) {
      setFallback({ id: personId, name: asMember.memberName, imagePath: asMember.memberImage });
      return;
    }
    let alive = true;
    api
      .personById(personId)
      .then((p) => alive && p && setFallback({ id: p.id, name: p.name, imagePath: p.imagePath }))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [entry, data, personId, bands]);

  const sections = useMemo(() => sectionWorks(personId, works ?? [], bands), [personId, works, bands]);
  const albumCount = useMemo(() => groupByAlbum(sections.find((x) => !x.collapsed)?.works ?? []).length, [sections]);
  // 哪些区是展开的（收起的那些点了才展开）。换人时重置
  const flashed = useRef("");
  const [opened, setOpened] = useState<Set<string>>(new Set());
  useEffect(() => setOpened(new Set()), [personId]);
  const isOpen = (key: string, collapsed: boolean) => !collapsed || opened.has(key);
  const toggleSection = (key: string) =>
    setOpened((cur) => {
      const next = new Set(cur);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  // 要定位的那张专辑：左栏「专辑」点进来的，或者发行轴上点的。滚到那一张，闪一下；
  // 在收起的区里就先展开那一区。nonce 让同一张点第二次也再闪
  const [focus, setFocus] = useState<{ key: string; nonce: number } | null>(null);
  useEffect(() => {
    setFocus(focusAlbumId === null ? null : { key: `id:${focusAlbumId}`, nonce: Date.now() });
  }, [personId, focusAlbumId]);
  useEffect(() => {
    if (focus === null || works === null) return;
    const token = `${focus.key}:${focus.nonce}`;
    if (flashed.current === token) return;
    const holder = sections.find((x) => x.collapsed && x.works.some((w) => albumKey(w) === focus.key));
    if (holder !== undefined && !opened.has(holder.key)) {
      setOpened((cur) => new Set(cur).add(holder.key));
      return; // 展开之后这个 effect 会再跑一次
    }
    const el = document.querySelector<HTMLElement>(`.profile-album[data-key="${CSS.escape(focus.key)}"]`);
    if (el === null) return;
    el.scrollIntoView({ block: "center", behavior: prefersReducedMotion() ? "auto" : "smooth" });
    el.classList.remove("flash");
    void el.offsetWidth; // 重新触发动画
    el.classList.add("flash");
    flashed.current = token;
  }, [focus, works, sections, opened]);

  const name = entry?.name ?? fallback?.name ?? "";
  const image = entry?.imagePath || fallback?.imagePath || "";
  const roles = entry ? rolesOf(entry) : [];
  const isBand = members.length > 0 || entry?.artistType === "Group";
  const years = yearSpan(entry);

  if (name === "") return <p className="muted pad">加载中…</p>;

  return (
    <div className="profile" key={personId}>
      <header className="profile-hero">
        <div className="profile-hero-bg" style={image ? { backgroundImage: `url("${cssUrl(image)}")` } : undefined} aria-hidden="true" />
        <span className="profile-photo">
          <Avatar path={image} name={name} size={104} />
        </span>
        <div className="profile-hero-text">
          <h1>{name}</h1>
          <div className="role-tags">
            {isBand && <span className="role-tag band">乐队</span>}
            {roles.map((r) => (
              <span key={r} className="role-tag" style={{ ["--role" as string]: ROLE_COLORS[r] }}>
                {ROLE_LABELS[r] ?? r}
              </span>
            ))}
          </div>
          {bands.map((b) => (
            <button key={b.bandId} className="belong" onClick={() => onOpenPerson(b.bandId)}>
              <Avatar path={b.bandImage} name={b.bandName} size={20} />
              {b.leftAt ? `曾属 ${b.bandName} · ${spanOf(b.joined, b.leftAt)}` : `${b.bandName} 成员${b.part ? ` · ${b.part}` : ""}`}
            </button>
          ))}
          <div className="profile-stats">
            {entry && entry.performer > 0 && <Fact value={entry.performer} label={bands.length > 0 ? "个人演唱" : "演唱"} />}
            {entry && entry.songs - entry.performer > 0 && <Fact value={entry.songs - entry.performer} label="幕后署名" />}
            {works !== null && albumCount > 0 && <Fact value={albumCount} label="专辑 / 单曲" />}
            {years && <Fact value={years} label={entry?.ended ? "已结束活动" : "有作品的年份"} />}
          </div>
        </div>
        {works !== null && works.length > 0 && (
          <button className="play-all" onClick={() => onPlayAll(sections[0]?.works ?? works)}>
            <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
              <path d="M7 5.5v13a1 1 0 0 0 1.5.86l11-6.5a1 1 0 0 0 0-1.72l-11-6.5A1 1 0 0 0 7 5.5Z" fill="currentColor" />
            </svg>
            播放
          </button>
        )}
      </header>

      {works !== null && works.length > 0 && (
        <ReleaseAxis sections={sections} onPick={(key) => setFocus({ key, nonce: Date.now() })} ended={entry?.ended ?? ""} />
      )}

      {(isBand || (entry !== undefined && entry.performer > 0 && entry.artistType !== "Person" && bands.length === 0)) && (
        <MembersEditor
          bandId={personId}
          members={members}
          data={data}
          onOpen={onOpenPerson}
          onChanged={onMembersChanged}
          onError={onError}
        />
      )}

      <section className="profile-works">
        {works === null && <p className="muted">加载作品…</p>}
        {works !== null && works.length === 0 && <p className="muted">没有作品记录</p>}
        {sections.map((sec) => {
          const open = isOpen(sec.key, sec.collapsed);
          return (
            <div className="work-section" key={sec.key}>
              {sec.title && (
                <h2 className="section-label section-head">
                  {sec.title}
                  <span className="section-count">{sec.works.length} 首</span>
                  {sec.collapsed && (
                    <button className="section-toggle" onClick={() => toggleSection(sec.key)} aria-expanded={open}>
                      {open ? "收起" : "展开"}
                    </button>
                  )}
                </h2>
              )}
              {sec.note && <p className="section-note">{sec.note}</p>}
              {open &&
                groupByAlbum(sec.works).map((g, i) => (
                  <div className="profile-album" key={g.key} data-key={g.key} style={{ ["--n" as string]: Math.min(i, 12) }}>
                    <Cover path={g.coverPath} size={72} rounded={8} />
                    <div className="profile-album-body">
                      <h3>
                        {g.title}
                        <small>
                          {g.year && `${g.year.slice(0, 4)} · `}
                          {g.tracks.length} 首
                        </small>
                      </h3>
                      <div className="song-chips">
                        {g.tracks.map((t) => (
                          <button
                            key={t.id}
                            className={playingSongId === t.id ? "now" : ""}
                            onClick={(e) => onOpenTrack(t, e.currentTarget)}
                            title={`${t.title} · ${t.artist}`}
                          >
                            {playingSongId === t.id && <span className="now-mark">♪</span>}
                            {t.title}
                          </button>
                        ))}
                      </div>
                    </div>
                  </div>
                ))}
            </div>
          );
        })}
      </section>
    </div>
  );
}

/** 「2014–2019」；只有一年就写一年 */
function yearSpan(entry: DirectoryEntry | undefined): string {
  if (!entry?.firstYear) return "";
  const from = entry.firstYear.slice(0, 4);
  const to = (entry.ended || entry.lastYear).slice(0, 4);
  return from === to || !to ? from : `${from}–${to}`;
}

/**
 * 发行轴：每张专辑一个点，横着按年份排；点越大歌越多。自己的作品实心，乐队时期的空心，合作 / 幕后是小一号的绿色。
 * 悬停看名字，点一下滚到下面那一张。乐队解散了，那一年画一道竖线。
 */
function ReleaseAxis({ sections, onPick, ended }: { sections: WorkSection[]; onPick: (key: string) => void; ended: string }) {
  const dots = sections.flatMap((sec) =>
    groupByAlbum(sec.works)
      .filter((g) => /^\d{4}/.test(g.year))
      .map((g) => ({ g, kind: sec.key === "own" ? "own" : sec.key === "credits" ? "credit" : "band", year: Number(g.year.slice(0, 4)) })),
  );
  if (dots.length === 0) return null;
  const endYear = /^\d{4}/.test(ended) ? Number(ended.slice(0, 4)) : null;
  const years = dots.map((d) => d.year).concat(endYear ?? []);
  const lo = Math.min(...years) - 1;
  const hi = Math.max(...years) + 1;
  const x = (y: number) => ((y - lo) / (hi - lo)) * 100;
  const step = hi - lo > 24 ? 5 : hi - lo > 12 ? 2 : 1;
  const ticks: number[] = [];
  for (let y = lo; y <= hi; y++) if (y % step === 0) ticks.push(y);
  // 同一年的几张错开一点，别叠成一个点
  const seen = new Map<number, number>();
  return (
    <section className="release-axis" aria-label="发行时间轴">
      <div className="axis-line" />
      {ticks.map((y) => (
        <span key={y} className="axis-year" style={{ left: `${x(y)}%` }}>
          {y}
        </span>
      ))}
      {endYear !== null && <span className="axis-end" style={{ left: `${x(endYear)}%` }} title={`${endYear} 解散`} />}
      {dots.map(({ g, kind, year }, i) => {
        const k = seen.get(year) ?? 0;
        seen.set(year, k + 1);
        const size = Math.min(26, 10 + g.tracks.length * 2) * (kind === "credit" ? 0.75 : 1);
        return (
          <button
            key={`${kind}-${g.key}`}
            className={`axis-dot ${kind} ${x(year) < 18 ? "edge-l" : x(year) > 82 ? "edge-r" : ""}`}
            style={{ left: `calc(${x(year)}% + ${k * 9}px)`, width: size, height: size, ["--n" as string]: i }}
            onClick={() => onPick(g.key)}
            aria-label={`${g.title} · ${year}`}
          >
            <span className="axis-label">
              {g.title} · {year}
            </span>
          </button>
        );
      })}
    </section>
  );
}

function Fact({ value, label }: { value: number | string; label: string }) {
  return (
    <div className="fact">
      <b>{value}</b>
      <small>{label}</small>
    </div>
  );
}

/** 档案头背后那层虚化的照片：本地文件要走 Tauri 的 asset 协议 */
function cssUrl(path: string): string {
  return convertFileSrc(path).replace(/"/g, "%22");
}

/** 右栏：合作者 */
export function PersonSide({
  personId,
  data,
  onOpenPerson,
  onOpenGraph,
  onError,
}: {
  personId: number;
  data: DirectoryData | null;
  onOpenPerson: (id: number) => void;
  /** 「全局 ↗」：在整张关系网里看这个人 */
  onOpenGraph: () => void;
  onError: (message: string) => void;
}) {
  // 这个人和他的直接合作者、乐队、成员
  const local = useMemo(
    () => (data === null ? null : buildGraph(data.entries, data.edges, data.memberships, { focus: personId, depth: 1 })),
    [data, personId],
  );
  const [peers, setPeers] = useState<Collaborator[] | null>(null);
  useEffect(() => {
    let alive = true;
    setPeers(null);
    api
      .collaborators(personId, 40)
      .then((p) => alive && setPeers(p))
      .catch((err: unknown) => {
        if (!alive) return;
        setPeers([]);
        onError(`读不出合作者：${messageOf(err)}`);
      });
    return () => {
      alive = false;
    };
  }, [personId, onError]);

  const bandmates = useMemo(() => {
    const out = new Set<number>();
    const dir = data?.dir;
    if (!dir) return out;
    for (const m of dir.membersOf.get(personId) ?? []) out.add(m.memberId);
    for (const b of dir.bandsOf.get(personId) ?? []) {
      out.add(b.bandId);
      for (const m of dir.membersOf.get(b.bandId) ?? []) out.add(m.memberId);
    }
    return out;
  }, [data, personId]);

  const max = Math.max(1, ...(peers ?? []).map((p) => p.sharedTracks));
  return (
    <div className="person-side" key={personId}>
      {local !== null && local.nodes.length > 1 && (
        <>
          <div className="side-head-row">
            <h3 className="side-head">关系网</h3>
            <button className="link-btn" onClick={onOpenGraph}>
              全局 ↗
            </button>
          </div>
          <RelationGraph
            className="local-graph"
            graph={local}
            focusId={personId}
            compact
            onSelect={(id) => id !== personId && onOpenPerson(id)}
          />
        </>
      )}
      <h3 className="side-head">合作者</h3>
      <p className="muted small">同一首歌里一起署名的人</p>
      {peers === null && <p className="muted">加载中…</p>}
      {peers !== null && peers.length === 0 && <p className="muted">没有合作者。多数歌只有一位歌手，关系的多少取决于数据本身。</p>}
      {peers?.map((p, i) => (
        <button key={p.personId} className="peer" style={{ ["--n" as string]: Math.min(i, 14) }} onClick={() => onOpenPerson(p.personId)}>
          <Avatar path={p.imagePath} name={p.name} size={30} />
          <span className="peer-text">
            <b>{p.name}</b>
            <span className="peer-bar">
              <i style={{ width: `${(p.sharedTracks / max) * 100}%`, background: ROLE_COLORS[p.roles[0] ?? ""] ?? "var(--accent)" }} />
            </span>
            <small>
              {bandmates.has(p.personId) && "同队 · "}
              {p.sharedTracks} 首共同 · {p.roles.map((r) => ROLE_LABELS[r] ?? r).join(" / ")}
            </small>
          </span>
        </button>
      ))}
    </div>
  );
}

/** 对不上人的专辑：直接列这张专辑里的歌 */
export function AlbumView({
  album,
  title,
  playingSongId,
  onOpenTrack,
  onPlayAll,
  onError,
}: {
  album: Album | null;
  title: string;
  playingSongId: string | null;
  onOpenTrack: (track: Track, from: HTMLElement) => void;
  onPlayAll: (tracks: Track[]) => void;
  onError: (message: string) => void;
}) {
  const [tracks, setTracks] = useState<Track[] | null>(null);
  const id = album?.id ?? null;
  useEffect(() => {
    if (id === null) return;
    let alive = true;
    setTracks(null);
    api
      .albumTracks(id)
      .then((t) => alive && setTracks(t))
      .catch((err: unknown) => {
        if (!alive) return;
        setTracks([]);
        onError(`读不出专辑：${messageOf(err)}`);
      });
    return () => {
      alive = false;
    };
  }, [id, onError]);

  return (
    <div className="profile album-page" key={id ?? title}>
      <header className="profile-hero">
        <div className="profile-hero-bg" style={album?.artworkPath ? { backgroundImage: `url("${cssUrl(album.artworkPath)}")` } : undefined} aria-hidden="true" />
        <span className="profile-photo square">
          <Cover path={album?.artworkPath} size={104} rounded={10} />
        </span>
        <div className="profile-hero-text">
          <h1>{album?.title || title}</h1>
          <p className="muted">
            {[album?.albumArtist, album?.year?.slice(0, 4)].filter(Boolean).join(" · ")}
          </p>
          <div className="profile-stats">
            {album && <Fact value={album.trackCount} label="首" />}
          </div>
        </div>
        {tracks !== null && tracks.length > 0 && (
          <button className="play-all" onClick={() => onPlayAll(tracks)}>
            ▶ 播放
          </button>
        )}
      </header>
      <section className="album-tracklist">
        {tracks === null && <p className="muted">加载中…</p>}
        {tracks?.map((t, i) => (
          <button
            key={t.id}
            className={`album-track ${playingSongId === t.id ? "now" : ""}`}
            style={{ ["--n" as string]: Math.min(i, 16) }}
            onClick={(e) => onOpenTrack(t, e.currentTarget)}
          >
            <span className="album-track-no">{playingSongId === t.id ? "♪" : i + 1}</span>
            <span className="album-track-title">{t.title}</span>
            <span className="muted small">{t.artist}</span>
          </button>
        ))}
      </section>
    </div>
  );
}
