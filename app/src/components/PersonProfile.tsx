/**
 * 资料库里的「人」：档案头（照片、角色、所属乐队、数字）和下面四页（概览、作品、成员与合作、时间轴）。
 * 档案占满中栏和右栏，合作者、小关系网都在「成员与合作」里。
 *
 * 原来的人物页按「演唱 / 作曲 / 作词 / 编曲」四个列表分人，同一个人在每个列表里各出现一次；
 * 现在一个人只有一页，他做过哪些角色写在档案头上。
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { api, formatDuration, ROLE_LABELS, type Album, type Collaborator, type DirectoryEntry, type Membership, type PersonWork, type Track } from "../api";
import { FIELD_SOURCE_LABELS, percent, qualityApi, type FieldProvenance } from "../corpusQuality";
import { messageOf } from "../errors";
import { rolesOf, sectionWorks, spanOf, type WorkSection } from "../libraryIndex";
import { prefersReducedMotion } from "../motion";
import { Avatar } from "./Avatar";
import { Cover } from "./Cover";
import { MembersEditor } from "./MembersEditor";
import { RelationGraph } from "./RelationGraph";
import { buildGraph } from "../graph";
import { buildAxisColumns } from "../releaseAxis";
import { ROLE_COLORS, type DirectoryData } from "./LibraryBrowser";
import { invalidatePerformers } from "./PerformerPicker";
import { tr } from "../i18n";

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
      g = { key, albumId: t.albumId, title: t.album || tr("单曲"), year: t.year, coverPath: t.coverPath, tracks: [] };
      groups.set(key, g);
    }
    // 同一首歌可能因为几个角色重复出现
    if (!g.tracks.some((x) => x.id === t.id)) g.tracks.push(t);
    if (t.year && (!g.year || t.year < g.year)) g.year = t.year;
    if (!g.coverPath && t.coverPath) g.coverPath = t.coverPath;
  }
  return [...groups.values()].sort((a, b) => (a.year || "9999").localeCompare(b.year || "9999") || a.title.localeCompare(b.title, "ja"));
}

const RELEASE_SUFFIX = /\s*[-–—]\s*(single|ep)\s*$/i;
const titleKey = (s: string) => s.normalize("NFKC").toLowerCase().replace(/\s+/g, "");

/** 「修羅 - Single」→「修羅」：单曲区里不再重复写「Single」 */
export function releaseTitle(title: string): string {
  return title.replace(RELEASE_SUFFIX, "") || title;
}

/**
 * 概览页：单曲和 EP 一栏、专辑一栏。库里没有发行类型，按这几条认单曲：
 * 没有专辑的歌（每首单独一张）；标题以「- Single」「- EP」结尾；不超过 3 首、且和其中一首歌同名
 * （「あぶく」「茜」这种单曲发行）。其余算专辑。两栏都是最新在前。
 */
export function splitReleases(groups: AlbumGroup[]): { singles: AlbumGroup[]; albums: AlbumGroup[] } {
  const singles: AlbumGroup[] = [];
  const albums: AlbumGroup[] = [];
  for (const g of groups) {
    if (g.key === "single") {
      for (const t of g.tracks) singles.push({ ...g, title: t.title, year: t.year || g.year, coverPath: t.coverPath || g.coverPath, tracks: [t] });
      continue;
    }
    const bare = titleKey(releaseTitle(g.title));
    const single = RELEASE_SUFFIX.test(g.title) || (g.tracks.length <= 3 && g.tracks.some((t) => titleKey(t.title) === bare));
    (single ? singles : albums).push(g);
  }
  const newest = (a: AlbumGroup, b: AlbumGroup) => (b.year || "").localeCompare(a.year || "");
  return { singles: singles.sort(newest), albums: albums.sort(newest) };
}

/** 档案下面的几页 */
type ProfileTab = "overview" | "works" | "people" | "timeline";

/** 每个人上次看的是哪一页：去别处再按「返回」回来，还在那一页 */
const tabMemory = new Map<number, ProfileTab>();

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
  /** 看这个人的词云 */
  onOpenCloud: () => void;
  /** 在整张关系网里看这个人 */
  onOpenGraph: () => void;
  /** 在时间轴里放大看这个人的时间树 */
  onOpenTimeline: () => void;
  onError: (message: string) => void;
}

/**
 * 一个人的档案：顶上一大块照片和名字，下面分四页——
 * 概览（专辑封面 + 成员 + 常一起署名的人）、作品（按专辑列歌）、成员与合作、时间轴。
 * 原来右栏的合作者和小关系网搬进了「成员与合作」，右栏让给中间。
 */
export function PersonProfile({
  personId,
  data,
  focusAlbumId,
  playingSongId,
  onOpenPerson,
  onOpenTrack,
  onPlayAll,
  onMembersChanged,
  onOpenCloud,
  onOpenGraph,
  onOpenTimeline,
  onError,
}: ProfileProps) {
  const [works, setWorks] = useState<PersonWork[] | null>(null);
  // 目录里没有的人（只在成员表里、一首署名都没有）也要能看：按 id 补一份
  const [fallback, setFallback] = useState<{ id: number; name: string; imagePath: string } | null>(null);
  const entry: DirectoryEntry | undefined = data?.dir.byId.get(personId);
  const [tab, setTabState] = useState<ProfileTab>(() => (focusAlbumId !== null ? "works" : tabMemory.get(personId) ?? "overview"));
  const setTab = (t: ProfileTab) => {
    tabMemory.set(personId, t);
    setTabState(t);
  };

  useEffect(() => {
    let alive = true;
    setWorks(null);
    api
      .personWorks(personId)
      .then((w) => alive && setWorks(w))
      .catch((err: unknown) => {
        if (!alive) return;
        setWorks([]);
        onError(tr("读不出作品：{err}", { err: messageOf(err) }));
      });
    return () => {
      alive = false;
    };
  }, [personId, onError]);

  // 合作者：概览里列前几位，「成员与合作」里列全部
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
        onError(tr("读不出合作者：{err}", { err: messageOf(err) }));
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

  // 「⋯」菜单：换照片、恢复刮削的照片、资料来源
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!menuOpen) return;
    const close = (e: PointerEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenuOpen(false);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setMenuOpen(false);
    window.addEventListener("pointerdown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [menuOpen]);

  // 换照片：刮削有时把专辑封面当成了歌手照片。换的这张刮削不再覆盖
  const [photoBusy, setPhotoBusy] = useState(false);
  const changePhoto = async () => {
    setMenuOpen(false);
    try {
      const picked = await openDialog({ title: tr("给 {name} 换照片", { name }), filters: [{ name: tr("图片"), extensions: ["jpg", "jpeg", "png", "webp"] }] });
      if (typeof picked !== "string") return;
      setPhotoBusy(true);
      await api.artistImageSet(name, picked);
      invalidatePerformers();
      onMembersChanged();
    } catch (err) {
      onError(tr("照片没换成：{err}", { err: messageOf(err) }));
    } finally {
      setPhotoBusy(false);
    }
  };
  const resetPhoto = async () => {
    setMenuOpen(false);
    setPhotoBusy(true);
    try {
      await api.artistImageReset(name);
      invalidatePerformers();
      onMembersChanged();
    } catch (err) {
      onError(tr("没恢复成：{err}", { err: messageOf(err) }));
    } finally {
      setPhotoBusy(false);
    }
  };

  const sections = useMemo(() => sectionWorks(personId, works ?? [], bands), [personId, works, bands]);
  const mainGroups = useMemo(() => groupByAlbum(sections.find((x) => !x.collapsed)?.works ?? []), [sections]);
  const releases = useMemo(() => splitReleases(mainGroups), [mainGroups]);
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

  // 要定位的那张专辑：左栏「专辑」点进来的、概览里点的封面、发行轴上点的。
  // 先切到「作品」，滚到那一张，闪一下；在收起的区里就先展开那一区。nonce 让同一张点第二次也再闪
  const [focus, setFocus] = useState<{ key: string; nonce: number } | null>(null);
  useEffect(() => {
    if (focusAlbumId === null) return setFocus(null);
    setTabState("works");
    setFocus({ key: `id:${focusAlbumId}`, nonce: Date.now() });
  }, [personId, focusAlbumId]);
  const showAlbum = (key: string) => {
    setTab("works");
    setFocus({ key, nonce: Date.now() });
  };
  useEffect(() => {
    if (focus === null || works === null || tab !== "works") return;
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
  }, [focus, works, sections, opened, tab]);

  const name = entry?.name ?? fallback?.name ?? "";
  const image = entry?.imagePath || fallback?.imagePath || "";
  const roles = entry ? rolesOf(entry) : [];
  const isBand = members.length > 0 || entry?.artistType === "Group";
  const years = yearSpan(entry);
  const canEditMembers = isBand || (entry !== undefined && entry.performer > 0 && entry.artistType !== "Person" && bands.length === 0);
  const canCloud = entry !== undefined && entry.performer > 0;

  // 顶栏照片按自己的宽高比居中摆，先量一下
  const [photoAspect, setPhotoAspect] = useState<number | null>(null);
  useEffect(() => {
    setPhotoAspect(null);
    if (!image) return;
    let alive = true;
    const img = new Image();
    img.onload = () => alive && img.naturalHeight > 0 && setPhotoAspect(img.naturalWidth / img.naturalHeight);
    img.src = convertFileSrc(image);
    return () => {
      alive = false;
    };
  }, [image]);

  if (name === "") return <p className="muted pad">{tr("加载中…")}</p>;

  const TABS: { key: ProfileTab; label: string }[] = [
    { key: "overview", label: tr("概览") },
    { key: "works", label: tr("作品") },
    { key: "people", label: tr("成员与合作") },
    { key: "timeline", label: tr("时间轴") },
  ];

  return (
    <div className="profile artist" key={personId}>
      <header className={`artist-hero ${image ? "" : "no-photo"}`}>
        <div className="artist-hero-art" aria-hidden="true">
          {image ? (
            <>
              <div className="artist-hero-glow" style={{ backgroundImage: `url("${cssUrl(image)}")` }} />
              <div
                className="artist-hero-photo"
                style={{ backgroundImage: `url("${cssUrl(image)}")`, ["--photo-aspect" as string]: photoAspect ?? 1 }}
              />
            </>
          ) : (
            <span className="artist-hero-initial">
              <Avatar path="" name={name} size={150} />
            </span>
          )}
        </div>
        <div className="artist-hero-text">
          <div className="role-tags">
            {isBand && <span className="role-tag band">{tr("乐队")}</span>}
            {roles.map((r) => (
              <span key={r} className="role-tag" style={{ ["--role" as string]: ROLE_COLORS[r] }}>
                {ROLE_LABELS[r] ?? r}
              </span>
            ))}
            {years && <span className="artist-hero-years">{entry?.ended ? tr("{years} · 已结束活动", { years }) : years}</span>}
          </div>
          <h1>{name}</h1>
          {bands.length > 0 && (
            <div className="artist-hero-bands">
              {bands.map((b) => (
                <button key={b.bandId} className="belong" onClick={() => onOpenPerson(b.bandId)}>
                  <Avatar path={b.bandImage} name={b.bandName} size={20} />
                  {b.leftAt ? tr("曾属 {bandName} · {joined}", { bandName: b.bandName, joined: spanOf(b.joined, b.leftAt) }) : tr("{bandName} 成员{v}", { bandName: b.bandName, v: b.part ? ` · ${b.part}` : "" })}
                </button>
              ))}
            </div>
          )}
          <div className="profile-stats">
            {entry && entry.performer > 0 && <Fact value={entry.performer} label={bands.length > 0 ? tr("个人演唱") : tr("演唱")} />}
            {entry && entry.songs - entry.performer > 0 && <Fact value={entry.songs - entry.performer} label={tr("幕后署名")} />}
            {works !== null && mainGroups.length > 0 && <Fact value={mainGroups.length} label={tr("专辑 / 单曲")} />}
          </div>
          <div className="hero-actions">
            {works !== null && works.length > 0 && (
              <button className="primary play-all" onClick={() => onPlayAll(sections[0]?.works ?? works)}>
                <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
                  <path d="M7 5.5v13a1 1 0 0 0 1.5.86l11-6.5a1 1 0 0 0 0-1.72l-11-6.5A1 1 0 0 0 7 5.5Z" fill="currentColor" />
                </svg>{tr("播放")}</button>
            )}
            {canCloud && (
              <button className="btn" onClick={onOpenCloud} title={tr("这个人唱的歌里最常出现的词")}>{tr("词云")}</button>
            )}
            <div className="more-menu" ref={menuRef}>
              <button
                className={`btn more-btn ${menuOpen ? "on" : ""}`}
                onClick={() => setMenuOpen((v) => !v)}
                disabled={photoBusy}
                title={tr("更多")}
                aria-label={tr("更多")}
                aria-expanded={menuOpen}
              >
                <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
                  <circle cx="6" cy="12" r="1.6" fill="currentColor" />
                  <circle cx="12" cy="12" r="1.6" fill="currentColor" />
                  <circle cx="18" cy="12" r="1.6" fill="currentColor" />
                </svg>
              </button>
              {menuOpen && (
                <div className="menu-panel menu-left" role="menu">
                  <button role="menuitem" onClick={() => void changePhoto()} title={tr("换一张照片（jpg / png / webp）")}>{tr("换照片…")}</button>
                  {entry?.imageManual && (
                    <button role="menuitem" onClick={() => void resetPhoto()}>{tr("恢复刮削的照片")}</button>
                  )}
                  <ArtistProvenance name={name} refreshKey={`${image}|${entry?.imageManual ?? ""}`} />
                </div>
              )}
            </div>
          </div>
        </div>
      </header>

      <div className="profile-tabs" role="tablist" aria-label={tr("档案分页")}>
        {TABS.map((t) => (
          <button key={t.key} role="tab" aria-selected={tab === t.key} className={tab === t.key ? "on" : ""} onClick={() => setTab(t.key)}>
            {t.label}
          </button>
        ))}
      </div>

      {tab === "overview" && (
        <div className="profile-overview" role="tabpanel">
          <section className="overview-main">
            {works === null && <p className="muted">{tr("加载作品…")}</p>}
            {works !== null && mainGroups.length === 0 && <p className="muted">{tr("没有作品记录")}</p>}
            {[
              { key: "singles", title: tr("单曲和 EP"), list: releases.singles },
              { key: "albums", title: tr("专辑"), list: releases.albums },
            ]
              .filter((part) => part.list.length > 0)
              .map((part) => (
                <div key={part.key} className="overview-releases">
                  <div className="overview-head">
                    <h2>{part.title}</h2>
                    <span className="muted small">{tr("最新在前")}</span>
                    {part.list.length > 8 && (
                      <button className="link-btn" onClick={() => setTab("works")}>{tr("全部 {n} 张", { n: part.list.length })}</button>
                    )}
                  </div>
                  <div className="cover-grid">
                    {part.list.slice(0, 8).map((g, i) => (
                      <button
                        key={`${g.key}:${g.tracks[0]?.id ?? ""}`}
                        className="cover-card"
                        style={{ ["--n" as string]: i }}
                        onClick={() => showAlbum(g.key)}
                        title={g.title}
                      >
                        <Cover path={g.coverPath} size={160} rounded={8} />
                        <b>{part.key === "singles" ? releaseTitle(g.title) : g.title}</b>
                        <small>
                          {[g.year.slice(0, 4), part.key === "singles" ? (RELEASE_SUFFIX.exec(g.title)?.[1]?.toUpperCase() === "EP" ? "EP" : tr("单曲")) : tr("{length} 首", { length: g.tracks.length })]
                            .filter(Boolean)
                            .join(" · ")}
                        </small>
                      </button>
                    ))}
                  </div>
                </div>
              ))}
          </section>

          <aside className="overview-side">
            {(members.length > 0 || bands.length > 0) && (
              <div className="overview-card">
                <div className="overview-head">
                  <h2>{members.length > 0 ? tr("成员") : tr("所属乐队")}</h2>
                  {canEditMembers && <button className="link-btn" onClick={() => setTab("people")}>{tr("编辑")}</button>}
                </div>
                <div className="overview-people">
                  {members.length > 0
                    ? members.map((m) => (
                        <button key={m.memberId} className={`mini-person ${m.leftAt ? "former" : ""}`} onClick={() => onOpenPerson(m.memberId)}>
                          <Avatar path={m.memberImage} name={m.memberName} size={38} />
                          <span>
                            <b>{m.memberName}</b>
                            <small>{[m.part, spanOf(m.joined, m.leftAt)].filter(Boolean).join(" · ")}</small>
                          </span>
                        </button>
                      ))
                    : bands.map((b) => (
                        <button key={b.bandId} className={`mini-person ${b.leftAt ? "former" : ""}`} onClick={() => onOpenPerson(b.bandId)}>
                          <Avatar path={b.bandImage} name={b.bandName} size={38} />
                          <span>
                            <b>{b.bandName}</b>
                            <small>{[b.part, spanOf(b.joined, b.leftAt)].filter(Boolean).join(" · ")}</small>
                          </span>
                        </button>
                      ))}
                </div>
              </div>
            )}

            {peers !== null && peers.length > 0 && (
              <div className="overview-card">
                <div className="overview-head">
                  <h2>{tr("常一起署名")}</h2>
                  <button className="link-btn" onClick={() => setTab("people")}>{tr("全部")}</button>
                </div>
                <div className="overview-peers">
                  {peers.slice(0, 4).map((p) => (
                    <button key={p.personId} className="peer-tile" onClick={() => onOpenPerson(p.personId)} title={p.name}>
                      <Avatar path={p.imagePath} name={p.name} size={46} />
                      <b>{p.name}</b>
                      <small>{tr("{n} 首", { n: p.sharedTracks })}</small>
                    </button>
                  ))}
                </div>
              </div>
            )}

            {canCloud && (
              <button className="overview-card cloud-card" onClick={onOpenCloud} title={tr("这个人唱的歌里最常出现的词")}>
                <span className="cloud-card-art" aria-hidden="true">
                  <b>夢</b>
                  <i>君</i>
                  <em>空</em>
                </span>
                <span className="cloud-card-text">
                  <strong>{tr("词云")}</strong>
                  <small>{tr("这个人唱的歌里最常出现的词")}</small>
                </span>
                <span className="cloud-card-go">{tr("打开")}</span>
              </button>
            )}
          </aside>
        </div>
      )}

      {tab === "works" && (
        <section className="profile-works" role="tabpanel">
          {works === null && <p className="muted">{tr("加载作品…")}</p>}
          {works !== null && works.length === 0 && <p className="muted">{tr("没有作品记录")}</p>}
          {sections.map((sec) => {
            const open = isOpen(sec.key, sec.collapsed);
            return (
              <div className="work-section" key={sec.key}>
                {sec.title && (
                  <h2 className="section-label section-head">
                    {sec.title}
                    <span className="section-count">{tr("{length} 首", { length: sec.works.length })}</span>
                    {sec.collapsed && (
                      <button className="link-btn" onClick={() => toggleSection(sec.key)} aria-expanded={open}>
                        {open ? tr("收起") : tr("展开")}
                      </button>
                    )}
                  </h2>
                )}
                {sec.note && <p className="section-note">{sec.note}</p>}
                {open &&
                  groupByAlbum(sec.works).map((g, i) => (
                    <div className="profile-album" key={g.key} data-key={g.key} style={{ ["--n" as string]: Math.min(i, 12) }}>
                      <Cover path={g.coverPath} size={96} rounded={8} />
                      <div className="profile-album-body">
                        <h3>
                          {g.title}
                          <small>
                            {g.year && `${g.year.slice(0, 4)} · `}{tr("{length} 首", { length: g.tracks.length })}</small>
                        </h3>
                        <ol className="album-rows">
                          {g.tracks.map((t, k) => (
                            <li key={t.id}>
                              <button
                                className={`album-row ${playingSongId === t.id ? "now" : ""}`}
                                onClick={(e) => onOpenTrack(t, e.currentTarget)}
                                title={`${t.title} · ${t.artist}`}
                              >
                                <span className="album-row-no">{playingSongId === t.id ? "♪" : k + 1}</span>
                                <span className="album-row-title">{t.title}</span>
                                {t.artist !== name && <span className="album-row-artist">{t.artist}</span>}
                                <span className="album-row-time">{formatDuration(t.durationSec)}</span>
                              </button>
                            </li>
                          ))}
                        </ol>
                      </div>
                    </div>
                  ))}
              </div>
            );
          })}
        </section>
      )}

      {tab === "people" && (
        <div className="profile-people" role="tabpanel">
          {canEditMembers && (
            <MembersEditor bandId={personId} members={members} data={data} onOpen={onOpenPerson} onChanged={onMembersChanged} onError={onError} />
          )}
          <PeopleAround personId={personId} data={data} peers={peers} onOpenPerson={onOpenPerson} onOpenGraph={onOpenGraph} />
        </div>
      )}

      {tab === "timeline" && (
        <div className="profile-timeline" role="tabpanel">
          {works === null && <p className="muted">{tr("加载作品…")}</p>}
          {works !== null && works.length > 0 && (
            <ReleaseAxis sections={sections} onPick={showAlbum} ended={entry?.ended ?? ""} />
          )}
          <button className="btn" onClick={onOpenTimeline}>{tr("在时间树里看")}</button>
        </div>
      )}
    </div>
  );
}

/** 「成员与合作」：这个人周围的小关系网 + 全部合作者 */
function PeopleAround({
  personId,
  data,
  peers,
  onOpenPerson,
  onOpenGraph,
}: {
  personId: number;
  data: DirectoryData | null;
  peers: Collaborator[] | null;
  onOpenPerson: (id: number) => void;
  onOpenGraph: () => void;
}) {
  const local = useMemo(
    () => (data === null ? null : buildGraph(data.entries, data.edges, data.memberships, { focus: personId, depth: 1 })),
    [data, personId],
  );
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
    <div className="people-around">
      <section className="people-list">
        <h2 className="section-label section-head">{tr("合作者")}<span className="section-count">{tr("同一首歌里一起署名的人")}</span></h2>
        {peers === null && <p className="muted">{tr("加载中…")}</p>}
        {peers !== null && peers.length === 0 && <p className="muted">{tr("没有合作者。多数歌只有一位歌手，关系的多少取决于数据本身。")}</p>}
        {peers?.map((p, i) => (
          <button key={p.personId} className="peer" style={{ ["--n" as string]: Math.min(i, 14) }} onClick={() => onOpenPerson(p.personId)}>
            <Avatar path={p.imagePath} name={p.name} size={34} />
            <span className="peer-text">
              <b>{p.name}</b>
              <span className="peer-bar">
                <i style={{ width: `${(p.sharedTracks / max) * 100}%`, background: ROLE_COLORS[p.roles[0] ?? ""] ?? "var(--accent)" }} />
              </span>
              <small>
                {bandmates.has(p.personId) && tr("同队 · ")}{tr("{sharedTracks} 首共同 · {map}", { sharedTracks: p.sharedTracks, map: p.roles.map((r) => ROLE_LABELS[r] ?? r).join(" / ") })}</small>
            </span>
          </button>
        ))}
      </section>
      {local !== null && local.nodes.length > 1 && (
        <section className="people-graph">
          <h2 className="section-label section-head">{tr("关系网")}<button className="link-btn" onClick={onOpenGraph}>{tr("全局 ↗")}</button></h2>
          <RelationGraph className="local-graph" graph={local} focusId={personId} compact onSelect={(id) => id !== personId && onOpenPerson(id)} />
        </section>
      )}
    </div>
  );
}

/** 歌手资料从哪来：MusicBrainz（带置信度）、Deezer 照片、你换的照片。没记录就不显示 */
function ArtistProvenance({ name, refreshKey }: { name: string; refreshKey: string }) {
  const [rows, setRows] = useState<FieldProvenance[]>([]);
  useEffect(() => {
    if (!name) return;
    let alive = true;
    // 读不到来源不影响看档案，静默
    qualityApi.artistProvenance(name).then((r) => alive && setRows(r)).catch(() => alive && setRows([]));
    return () => {
      alive = false;
    };
  }, [name, refreshKey]);
  if (rows.length === 0) return null;
  const bySource = new Map<string, FieldProvenance[]>();
  for (const r of rows) bySource.set(r.source, [...(bySource.get(r.source) ?? []), r]);
  const parts = [...bySource].map(([source, list]) => {
    const fields = list.map((r) => ARTIST_FIELD_LABELS[r.field] ?? r.field).join(tr("、"));
    const conf = list[0]?.confidence;
    return `${fields}${tr("：")}${FIELD_SOURCE_LABELS[source] ?? source}${conf != null ? tr("（置信度 {conf}）", { conf: percent(conf) }) : ""}`;
  });
  return <p className="muted small artist-provenance" title={tr("资料来源")}>{tr("资料来源 · {parts}", { parts: parts.join(" · ") })}</p>;
}

const ARTIST_FIELD_LABELS: Record<string, string> = {
  artist_type: tr("类型"),
  country: tr("国家"),
  formed: tr("成立"),
  ended: tr("结束"),
  mbid: tr("MusicBrainz 编号"),
  image_path: tr("照片"),
};

function yearSpan(entry: DirectoryEntry | undefined): string {
  if (!entry?.firstYear) return "";
  const from = entry.firstYear.slice(0, 4);
  const to = (entry.ended || entry.lastYear).slice(0, 4);
  return from === to || !to ? from : `${from}–${to}`;
}

/**
 * 发行轴：一年一列，这一年发的专辑 / 单曲画成封面在列里一张挨一张排开（专辑方、单曲圆）。
 * 自己的作品正常显示，乐队时期的带一圈灰边，合作 / 幕后小一号带绿边。
 * 悬停看名字，点一下滚到下面那一张；放不下时整条轴横着滚。
 * 连着几年没发行折成「…」，乐队解散那一年标「解散」（见 `releaseAxis.ts`）。
 */
function ReleaseAxis({ sections, onPick, ended }: { sections: WorkSection[]; onPick: (key: string) => void; ended: string }) {
  const items = sections.flatMap((sec) =>
    groupByAlbum(sec.works)
      .filter((g) => /^\d{4}/.test(g.year))
      .map((g) => ({ g, kind: sec.key === "own" ? "own" : sec.key === "credits" ? "credit" : "band", year: Number(g.year.slice(0, 4)) })),
  );
  const endYear = /^\d{4}/.test(ended) ? Number(ended.slice(0, 4)) : null;
  const columns = buildAxisColumns(items, endYear);
  const [hover, setHover] = useState<string | null>(null);
  if (columns.length === 0) return null;
  const albums = items.filter((x) => x.g.albumId !== null).length;
  const years = items.map((x) => x.year);
  const span = `${Math.min(...years)}–${Math.max(...years)}`;
  return (
    <section className="raxis" aria-label={tr("发行时间轴")}>
      {/* 悬停的那张写在这一行（轴是横向滚动的，气泡会被裁掉，所以不用气泡） */}
      <p className={`raxis-caption ${hover !== null ? "on" : ""}`} aria-live="polite">
        {hover ?? tr("{span} · {albums} 张专辑 · {v} 首单曲{v2}", { span, albums, v: items.length - albums, v2: endYear !== null ? tr(" · {endYear} 解散", { endYear }) : "" })}
      </p>
      <div className="raxis-scroll">
      <div className="raxis-track">
        {columns.map((col, i) =>
          col.type === "gap" ? (
            <div key={`gap-${col.from}`} className="raxis-gap" title={tr("{from}–{to} 没有发行", { from: col.from, to: col.to })} style={{ ["--i" as string]: i }}>
              <span className="raxis-gap-dots" aria-hidden="true" />
              <span className="raxis-year">{col.to - col.from >= 1 ? `${col.from}–${String(col.to).slice(2)}` : col.from}</span>
            </div>
          ) : (
            <div
              key={col.year}
              className={`raxis-col ${col.items.length === 0 ? "empty" : ""}`}
              style={{ ["--i" as string]: i, ["--count" as string]: col.items.length }}
            >
              <div className="raxis-stack">
                {col.items
                  .sort((a, b) => a.g.year.localeCompare(b.g.year))
                  .map(({ g, kind }, k) => (
                    <button
                      key={`${kind}-${g.key}`}
                      className={`raxis-cover ${kind} ${g.albumId === null ? "single" : ""}`}
                      style={{ ["--k" as string]: k, zIndex: col.items.length - k }}
                      onClick={() => onPick(g.key)}
                      onPointerEnter={() => setHover(describe(g, kind))}
                      onPointerLeave={() => setHover(null)}
                      onFocus={() => setHover(describe(g, kind))}
                      onBlur={() => setHover(null)}
                      aria-label={describe(g, kind)}
                    >
                      <Cover path={g.coverPath} size={kind === "credit" ? 30 : 40} rounded={g.albumId === null ? 20 : 6} alt="" />
                    </button>
                  ))}
              </div>
              <span className="raxis-node" aria-hidden="true" />
              <span className="raxis-year">
                {col.year}
                {col.items.length > 1 && <small>{col.items.length}</small>}
              </span>
              {col.ended && <span className="raxis-end">{tr("解散")}</span>}
            </div>
          ),
        )}
      </div>
      </div>
    </section>
  );
}

/** 「左右盲 · 2023-04-05 · 12 首 · 乐队时期」 */
function describe(g: AlbumGroup, kind: string): string {
  const role = kind === "band" ? tr(" · 乐队时期") : kind === "credit" ? tr(" · 合作 / 幕后") : "";
  return tr("{title} · {slice} · {length} 首{role}", { title: g.title, slice: g.year.slice(0, 10), length: g.tracks.length, role });
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
        onError(tr("读不出专辑：{err}", { err: messageOf(err) }));
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
            {album && <Fact value={album.trackCount} label={tr("首")} />}
          </div>
        </div>
        {tracks !== null && tracks.length > 0 && (
          <button className="primary play-all" onClick={() => onPlayAll(tracks)}>{tr("▶ 播放")}</button>
        )}
      </header>
      <section className="album-tracklist">
        {tracks === null && <p className="muted">{tr("加载中…")}</p>}
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
