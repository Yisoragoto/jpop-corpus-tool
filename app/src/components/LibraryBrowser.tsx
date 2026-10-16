/**
 * 资料库左栏：四种找法——音乐人、幕后、专辑、歌曲。
 *
 * 「音乐人」只列台前的：乐队和以个人名义唱歌的人。成员默认收在乐队下面（点「N 人」展开），
 * 不和乐队平级各占一行——きのこ帝国 和 佐藤千亜妃 不会再看着像同一份东西列了两遍。
 * 单飞的人另外以个人身份出现在顶层，副标题写着「曾属 X」。规则见 `libraryIndex.ts`。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { api, formatDuration, ROLE_LABELS, type Album, type CollabEdge, type DirectoryEntry, type Membership, type Track } from "../api";
import { messageOf } from "../errors";
import {
  albumOwner,
  BEHIND_ROLES,
  buildDirectory,
  filterBehind,
  filterMusicians,
  memberSubtitle,
  musicianSubtitle,
  roleCount,
  rolesOf,
  type BehindRole,
  type Directory,
  type LibraryView,
} from "../libraryIndex";
import { Avatar } from "./Avatar";
import { Cover } from "./Cover";

export type BrowseMode = "artists" | "behind" | "albums" | "songs";

const MODES: { key: BrowseMode; label: string }[] = [
  { key: "artists", label: "音乐人" },
  { key: "behind", label: "幕后" },
  { key: "albums", label: "专辑" },
  { key: "songs", label: "歌曲" },
];

const MODE_KEY = "jp.library.mode";
const OPEN_KEY = "jp.library.openBands";

function readMode(): BrowseMode {
  try {
    const raw = localStorage.getItem(MODE_KEY);
    return MODES.some((m) => m.key === raw) ? (raw as BrowseMode) : "artists";
  } catch {
    return "artists";
  }
}

function readOpen(): Set<number> {
  try {
    const raw = localStorage.getItem(OPEN_KEY);
    return new Set(raw === null ? [] : (JSON.parse(raw) as number[]));
  } catch {
    return new Set();
  }
}

/** 角色的颜色：幕后列表的小圆点、档案页的角色标签共用 */
export const ROLE_COLORS: Record<string, string> = {
  performer: "#4cc2ff",
  composer: "#f7a8c4",
  lyricist: "#ffd479",
  arranger: "#9be3a6",
};

export interface DirectoryData {
  dir: Directory;
  albums: Album[];
  memberships: Membership[];
  /** 关系网的边（谁和谁一起署过名、几首） */
  edges: CollabEdge[];
  entries: DirectoryEntry[];
}

/**
 * 左栏和中栏都要的那份目录：人、成员关系、专辑。`version` 变了（导入、刮削、改了成员）重拉。
 * 读不出来时给空目录并报错，页面照样能按歌找。
 */
export function useDirectory(version: number, onError: (message: string) => void): DirectoryData | null {
  const [data, setData] = useState<DirectoryData | null>(null);
  useEffect(() => {
    let alive = true;
    Promise.all([api.peopleDirectory(), api.memberships(), api.listAlbums(undefined, 2000), api.collaborationEdges()])
      .then(([entries, memberships, albums, edges]) => {
        if (alive) setData({ dir: buildDirectory(entries, memberships), albums, memberships, edges, entries });
      })
      .catch((err: unknown) => {
        if (!alive) return;
        onError(`读不出音乐人列表：${messageOf(err)}`);
        setData({ dir: buildDirectory([], []), albums: [], memberships: [], edges: [], entries: [] });
      });
    return () => {
      alive = false;
    };
  }, [version, onError]);
  return data;
}

interface Props {
  data: DirectoryData | null;
  tracks: Track[];
  view: LibraryView;
  selectedSongId: string | null;
  missingIds: Set<string>;
  onOpenPerson: (id: number, from?: HTMLElement) => void;
  onOpenAlbum: (album: Album) => void;
  onOpenTrack: (track: Track, from: HTMLElement) => void;
  /** 找不到音频的歌的提示条（原来在曲库左栏顶上） */
  banner?: React.ReactNode;
  onOpenGraph: () => void;
  /** 没给就不显示时间轴按钮 */
  onOpenTimeline?: () => void;
  onOpenCloud: () => void;
}

export function LibraryBrowser({
  data,
  tracks,
  view,
  selectedSongId,
  missingIds,
  onOpenPerson,
  onOpenAlbum,
  onOpenTrack,
  banner,
  onOpenGraph,
  onOpenTimeline,
  onOpenCloud,
}: Props) {
  const [mode, setModeState] = useState<BrowseMode>(readMode);
  const [query, setQuery] = useState("");
  const [role, setRole] = useState<BehindRole>("all");
  const [open, setOpen] = useState<Set<number>>(readOpen);
  const listRef = useRef<HTMLDivElement>(null);

  const setMode = useCallback((next: BrowseMode) => {
    setModeState(next);
    listRef.current?.scrollTo({ top: 0 });
    try {
      localStorage.setItem(MODE_KEY, next);
    } catch {
      // 存不了就只在这次会话里生效
    }
  }, []);

  const toggleBand = useCallback((id: number) => {
    setOpen((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      try {
        localStorage.setItem(OPEN_KEY, JSON.stringify([...next]));
      } catch {
        // 同上
      }
      return next;
    });
  }, []);

  const personId = view.kind === "person" ? view.id : null;
  // 看着某位成员时，他的乐队自动展开——不然左栏上找不到高亮在哪
  const viewingBands = useMemo(
    () => new Set((personId !== null ? data?.dir.bandsOf.get(personId) : undefined)?.map((m) => m.bandId) ?? []),
    [data, personId],
  );

  const modeIndex = MODES.findIndex((m) => m.key === mode);
  const placeholder =
    mode === "albums" ? "筛选专辑 / 歌手" : mode === "songs" ? "筛选歌名 / 歌手" : "筛选名字";

  return (
    <aside className="pane library browser">
      {banner}
      <div className="browser-head">
        <input className="search" placeholder={placeholder} value={query} onChange={(e) => setQuery(e.target.value)} />
        <button className={`btn icon-btn-ctl browser-view-btn ${view.kind === "graph" ? "on" : ""}`} onClick={onOpenGraph} title="关系网：所有人和他们的合作" aria-label="关系网">
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.7" aria-hidden="true">
            <circle cx="6" cy="7" r="2.4" />
            <circle cx="18" cy="6" r="2" />
            <circle cx="12" cy="17" r="2.8" />
            <path d="m8 8.4 2.6 6.4M16.4 7.5 13.6 14.6M8.3 6.7 16 6.1" />
          </svg>
        </button>
        <button className={`btn icon-btn-ctl browser-view-btn ${view.kind === "cloud" ? "on" : ""}`} onClick={onOpenCloud} title="词云：歌词里最常出现的词" aria-label="词云">
          <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor" aria-hidden="true">
            <text x="1" y="12" fontSize="10" fontWeight="700">あ</text>
            <text x="11" y="10" fontSize="7">夢</text>
            <text x="9" y="21" fontSize="9" fontWeight="700">君</text>
            <text x="2" y="20" fontSize="5.5" opacity=".6">空</text>
          </svg>
        </button>
        {onOpenTimeline && <button className={`btn icon-btn-ctl browser-view-btn ${view.kind === "timeline" ? "on" : ""}`} onClick={onOpenTimeline} title="时间轴：谁在哪年发了什么" aria-label="时间轴">
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" aria-hidden="true">
            <path d="M3 6h18M3 12h18M3 18h18" opacity=".35" />
            <path d="M5 6h7M9 12h10M4 18h5" strokeWidth="3" />
          </svg>
        </button>}
      </div>
      <div className="segmented seg-slide browse-modes" style={{ ["--seg-i" as string]: modeIndex, ["--seg-n" as string]: MODES.length }}>
        <span className="seg-thumb" aria-hidden="true" />
        {MODES.map((m) => (
          <button key={m.key} className={mode === m.key ? "on" : ""} onClick={() => setMode(m.key)}>
            {m.label}
          </button>
        ))}
      </div>
      {mode === "behind" && (
        <div className="role-chips">
          {BEHIND_ROLES.map((r) => (
            <button key={r.key} className={`chip ${role === r.key ? "on" : ""}`} onClick={() => setRole(r.key)}>
              {r.key !== "all" && <i style={{ background: ROLE_COLORS[r.key] }} />}
              {r.label}
            </button>
          ))}
        </div>
      )}

      <div className="list browse-list" ref={listRef} key={mode}>
        {data === null && mode !== "songs" && <p className="muted pad">加载中…</p>}
        {data !== null && mode === "artists" && (
          <ArtistItems
            data={data}
            query={query}
            open={open}
            forcedOpen={viewingBands}
            personId={personId}
            onToggle={toggleBand}
            onOpen={onOpenPerson}
          />
        )}
        {data !== null && mode === "behind" && (
          <BehindItems entries={filterBehind(data.dir.behind, role, query)} role={role} personId={personId} onOpen={onOpenPerson} />
        )}
        {data !== null && mode === "albums" && (
          <AlbumItems albums={data.albums} query={query} view={view} onOpen={onOpenAlbum} />
        )}
        {mode === "songs" && (
          <SongItems tracks={tracks} query={query} selectedSongId={view.kind === "song" ? selectedSongId : null} missingIds={missingIds} onOpen={onOpenTrack} />
        )}
      </div>
    </aside>
  );
}

/** 进场时逐个浮上来，排到后面的一起出，不让长列表等半天 */
const stagger = (i: number) => ({ ["--n" as string]: Math.min(i, 18) });

function ArtistItems({
  data,
  query,
  open,
  forcedOpen,
  personId,
  onToggle,
  onOpen,
}: {
  data: DirectoryData;
  query: string;
  open: Set<number>;
  forcedOpen: Set<number>;
  personId: number | null;
  onToggle: (id: number) => void;
  onOpen: (id: number, from?: HTMLElement) => void;
}) {
  const hits = filterMusicians(data.dir.musicians, query);
  if (hits.length === 0) {
    return <p className="muted pad">{query ? "没有匹配的音乐人" : "还没有演唱署名。导入带歌手信息的歌之后这里就有了"}</p>;
  }
  return (
    <>
      {hits.map(({ row, memberHit }, i) => {
        const id = row.entry.id;
        const expanded = row.members.length > 0 && (open.has(id) || forcedOpen.has(id) || memberHit);
        return (
          <div className="browse-group" key={id}>
            <button
              className={`browse-item ${personId === id ? "on" : ""} ${row.members.length > 0 ? "has-toggle" : ""}`}
              style={stagger(i)}
              onClick={(e) => onOpen(id, e.currentTarget)}
            >
              <Avatar path={row.entry.imagePath} name={row.entry.name} size={34} />
              <span className="browse-text">
                <b>{row.entry.name}</b>
                <small>{musicianSubtitle(row)}</small>
              </span>
            </button>
            {row.members.length > 0 && (
              <button
                className={`chip member-toggle ${expanded ? "open" : ""}`}
                onClick={() => onToggle(id)}
                title={expanded ? "收起成员" : "展开成员"}
                aria-expanded={expanded}
              >
                <span className="chev" aria-hidden="true">›</span> {row.members.length} 人
              </button>
            )}
            {expanded &&
              row.members.map((m, j) => (
                <button
                  key={m.membership.memberId}
                  className={`browse-item member ${m.former ? "former" : ""} ${personId === m.membership.memberId ? "on" : ""}`}
                  style={stagger(j)}
                  onClick={(e) => onOpen(m.membership.memberId, e.currentTarget)}
                >
                  <Avatar path={m.entry?.imagePath || m.membership.memberImage} name={m.membership.memberName} size={28} />
                  <span className="browse-text">
                    <b>{m.membership.memberName}</b>
                    <small>{memberSubtitle(m)}</small>
                  </span>
                </button>
              ))}
          </div>
        );
      })}
    </>
  );
}

function BehindItems({
  entries,
  role,
  personId,
  onOpen,
}: {
  entries: DirectoryEntry[];
  role: BehindRole;
  personId: number | null;
  onOpen: (id: number, from?: HTMLElement) => void;
}) {
  if (entries.length === 0) return <p className="muted pad">没有匹配的人</p>;
  return (
    <>
      {entries.map((e, i) => (
        <button key={e.id} className={`browse-item ${personId === e.id ? "on" : ""}`} style={stagger(i)} onClick={(ev) => onOpen(e.id, ev.currentTarget)}>
          <Avatar path={e.imagePath} name={e.name} size={34} />
          <span className="browse-text">
            <b>{e.name}</b>
            <small>
              {rolesOf(e)
                .map((r) => `${ROLE_LABELS[r] ?? r} ${e[r]}`)
                .join(" · ")}
            </small>
          </span>
          <span className="role-dots" title={rolesOf(e).map((r) => ROLE_LABELS[r] ?? r).join(" / ")}>
            {rolesOf(e).map((r) => (
              <i key={r} style={{ background: ROLE_COLORS[r] }} />
            ))}
          </span>
          {role !== "all" && <em className="browse-count">{roleCount(e, role)}</em>}
        </button>
      ))}
    </>
  );
}

function AlbumItems({
  albums,
  query,
  view,
  onOpen,
}: {
  albums: Album[];
  query: string;
  view: LibraryView;
  onOpen: (album: Album) => void;
}) {
  const q = query.trim().toLowerCase();
  const list = albums
    .filter((a) => q === "" || a.title.toLowerCase().includes(q) || a.albumArtist.toLowerCase().includes(q))
    .sort((a, b) => b.year.localeCompare(a.year) || a.title.localeCompare(b.title, "ja"));
  if (list.length === 0) return <p className="muted pad">{q ? "没有匹配的专辑" : "还没有专辑信息"}</p>;
  const current =
    view.kind === "album" ? view.albumId : view.kind === "person" ? (view.albumId ?? null) : null;
  return (
    <>
      {list.map((a, i) => (
        <button key={a.id} className={`browse-item ${current === a.id ? "on" : ""}`} style={stagger(i)} onClick={() => onOpen(a)}>
          <Cover path={a.artworkPath} size={34} rounded={6} />
          <span className="browse-text">
            <b>{a.title || "（无题）"}</b>
            <small>
              {albumOwner(a.albumArtist) || "未知歌手"}
              {a.year && ` · ${a.year.slice(0, 4)}`}
            </small>
          </span>
          <em className="browse-count">{a.trackCount}</em>
        </button>
      ))}
    </>
  );
}

function SongItems({
  tracks,
  query,
  selectedSongId,
  missingIds,
  onOpen,
}: {
  tracks: Track[];
  query: string;
  selectedSongId: string | null;
  missingIds: Set<string>;
  onOpen: (track: Track, from: HTMLElement) => void;
}) {
  const q = query.trim().toLowerCase();
  const list = useMemo(
    () =>
      tracks
        .filter((t) => q === "" || t.title.toLowerCase().includes(q) || t.artist.toLowerCase().includes(q))
        .sort((a, b) => a.title.localeCompare(b.title, "ja")),
    [tracks, q],
  );
  if (list.length === 0) return <p className="muted pad">{q ? "没有匹配的歌" : "曲库是空的，先去「导入」"}</p>;
  return (
    <>
      {list.map((t, i) => (
        <button
          key={t.id}
          className={`browse-item song ${selectedSongId === t.id ? "on" : ""}`}
          style={stagger(i)}
          onClick={(e) => onOpen(t, e.currentTarget)}
        >
          <Cover path={t.coverPath} size={34} rounded={6} />
          <span className="browse-text">
            <b>{t.title}</b>
            <small>
              {t.artist}
              {t.lineCount === 0 && <span className="warn"> · 无歌词</span>}
              {missingIds.has(t.id) && <span className="warn"> · 音频丢失</span>}
            </small>
          </span>
          <em className="browse-count">{t.durationSec !== null ? formatDuration(t.durationSec) : ""}</em>
        </button>
      ))}
    </>
  );
}
