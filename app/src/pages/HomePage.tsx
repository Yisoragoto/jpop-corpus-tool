/**
 * 首页 = 打开就能接着干的地方。
 *
 * 版面照一张参考图定的（2026-10-15）：顶上一条全库搜索；左边一大块「继续听」，
 * 下面一排最近播放；右边一列「今日」「语料库」「最近添加」。
 *
 * 这一页是「区块不是方框」的例外：几块面板用半透明填充压在云母上，
 * 没有描边——左右两栏需要靠面板分出层次，平铺在页底色上会糊成一片。
 *
 * **每一块都要能点进去**：点曲目进曲库、点搜索结果去对应的地方。
 * 点不动、也不回答「我该干什么 / 我进步了没有」的东西不放。
 */

import { useCallback, useEffect, useRef, useState } from "react";

import {
  api,
  formatDuration,
  formatLongDuration,
  type AddedTrack,
  type ContinueTrack,
  type HomeSummary,
  type PlayedTrack,
  type QuickHit,
} from "../api";
import { useCoverColor } from "../coverColor";
import { Cover } from "../components/Cover";
import { hitHint, hitKey, hitLabel, KIND_LABELS, useQuickSearch } from "../components/CommandPalette";
import { formatRelative } from "../relativeTime";
import { useAppSettings } from "../settings";
import type { LibraryActions } from "../useLibrary";

interface Props {
  actions: LibraryActions;
  onNavigate: (route: "library" | "kwic" | "analytics") => void;
  onError: (message: string) => void;
  /** 选中一条搜索结果。和 Ctrl K 面板走同一个落点映射（在 App 里） */
  onPickHit: (hit: QuickHit) => void | Promise<void>;
  /** 播放状态变了就刷新——听完一首之后「继续听」「今日」应当立刻反映 */
  playbackSongId: string;
  /** 库里的歌变了就加一（启动横幅上直接导入新歌时，人还停在这一页） */
  libraryVersion: number;
}

export function HomePage({ actions, onNavigate, onError, onPickHit, playbackSongId, libraryVersion }: Props) {
  const [home, setHome] = useState<HomeSummary | null>(null);
  /** 今天制了几张卡。没装 Anki 是 null，那一句就不显示 */
  const [cardsToday, setCardsToday] = useState<number | null>(null);

  const load = useCallback(() => {
    api
      .homeSummary()
      .then(setHome)
      .catch((e) => onError(String(e.message ?? e)));
    // 读 Anki 要复制一份 collection，慢的时候别拖着首页；读不到就当没有
    api
      .ankiNotesToday()
      .then(setCardsToday)
      .catch(() => setCardsToday(null));
  }, [onError]);

  useEffect(load, [load, libraryVersion]);
  // 换歌时重新拉一次：上一首的收听记录这时候刚落库
  useEffect(() => {
    if (playbackSongId) load();
  }, [playbackSongId, load]);

  const open = useCallback(
    async (songId: string) => {
      onNavigate("library");
      await actions.openTrackById(songId, { autoPlay: false });
    },
    [actions, onNavigate],
  );

  /** 留在首页接着放：底下的播放条会动起来，不必跳走 */
  const resume = useCallback(
    async (songId: string, fromSec: number) => {
      await actions.openTrackById(songId, { seekTo: fromSec, autoPlay: true });
    },
    [actions],
  );

  if (!home) return <div className="page muted pad">加载中…</div>;

  return (
    <div className="page home">
      <HomeSearch onPickHit={onPickHit} />

      <div className="home-grid">
        <section className="home-panel home-main">
          <h2 className="home-panel-title">继续听</h2>
          {home.continueTrack === null ? (
            <p className="section-empty">还没有收听记录。去曲库挑一首，听满 5 秒就会出现在这里。</p>
          ) : (
            <ContinueHero
              track={home.continueTrack}
              onResume={(from) => void resume(home.continueTrack!.songId, from)}
              onOpen={() => void open(home.continueTrack!.songId)}
            />
          )}
          {home.recentlyPlayed.length > 1 && (
            <div className="home-recent">
              {home.recentlyPlayed.slice(1, 9).map((t) => (
                <RecentTile key={t.songId} track={t} onClick={() => void open(t.songId)} />
              ))}
            </div>
          )}
        </section>

        <div className="home-side">
          <TodayPanel home={home} cardsToday={cardsToday} />
          <CorpusPanel home={home} onMore={() => onNavigate("analytics")} />
          <AddedPanel tracks={home.recentlyAdded} onOpen={(id) => void open(id)} />
        </div>
      </div>
    </div>
  );
}

/** 顶上那条搜索栏。和 Ctrl K 共用一套查询与落点，结果就在栏下面展开 */
function HomeSearch({ onPickHit }: { onPickHit: (hit: QuickHit) => void | Promise<void> }) {
  const [text, setText] = useState("");
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const query = text.trim();
  const { hits, busy } = useQuickSearch(query);
  const boxRef = useRef<HTMLDivElement>(null);

  useEffect(() => setActive(0), [hits]);

  // 点到外面就收起
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!boxRef.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  const pick = (index: number) => {
    const hit = hits[index];
    if (!hit) return;
    setOpen(false);
    setText("");
    void onPickHit(hit);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      setOpen(false);
      (e.target as HTMLInputElement).blur();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => (hits.length === 0 ? 0 : (i + 1) % hits.length));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => (hits.length === 0 ? 0 : (i - 1 + hits.length) % hits.length));
    } else if (e.key === "Enter") {
      e.preventDefault();
      pick(active);
    }
  };

  const showList = open && query.length > 0;

  return (
    <div className="home-search" ref={boxRef}>
      <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" className="home-search-icon">
        <circle cx="7" cy="7" r="4.5" fill="none" stroke="currentColor" strokeWidth="1.4" />
        <path d="M10.4 10.4 14 14" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
      </svg>
      <input
        type="search"
        value={text}
        placeholder="搜歌名、歌手、专辑、歌词、词"
        onChange={(e) => {
          setText(e.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onKeyDown={onKeyDown}
        aria-label="搜索全库"
      />
      <kbd>Ctrl K</kbd>
      {showList && (
        <div className="home-search-list palette-list" role="listbox">
          {hits.map((hit, i) => (
            <button
              key={hitKey(hit)}
              className="palette-row"
              data-active={i === active}
              onMouseEnter={() => setActive(i)}
              onClick={() => pick(i)}
            >
              <span className="palette-kind">{KIND_LABELS[hit.kind]}</span>
              <span className="palette-label">{hitLabel(hit)}</span>
              <span className="palette-hint">{hitHint(hit)}</span>
            </button>
          ))}
          {!busy && hits.length === 0 && <p className="muted pad">没有结果</p>}
        </div>
      )}
    </div>
  );
}

/** 「继续听」那一大块。底色取自封面，和全屏歌词同一套取色 */
function ContinueHero({
  track,
  onResume,
  onOpen,
}: {
  track: ContinueTrack;
  onResume: (fromSec: number) => void;
  onOpen: () => void;
}) {
  const color = useCoverColor(track.coverPath);
  const duration = track.durationSec ?? 0;
  // 听完了（或者停在最后几秒）就从头放，不然从停下的地方接着放
  const finished = track.completed || (duration > 0 && duration - track.positionSec < 3);
  const from = finished ? 0 : track.positionSec;
  const pct = duration > 0 ? Math.min(100, (track.positionSec / duration) * 100) : 0;

  return (
    <div className="home-hero" style={color ? { background: color } : undefined}>
      <button className="home-hero-cover" onClick={onOpen} title="在曲库里打开">
        <Cover path={track.coverPath} size={168} rounded={10} />
      </button>
      <div className="home-hero-body">
        <span className="home-hero-meta">
          {formatRelative(track.lastPlayed)} · 听过 {track.playCount} 次
        </span>
        <button className="home-hero-title" onClick={onOpen}>
          {track.title}
        </button>
        <span className="home-hero-artist">{track.artist}</span>
        {track.stoppedLine !== "" && (
          <span className="home-hero-line" title="上次停在这一行">
            「{track.stoppedLine}」
          </span>
        )}
        <div className="home-hero-progress">
          <div className="home-hero-bar">
            <div style={{ width: `${finished ? 100 : pct}%` }} />
          </div>
          <span>
            {finished ? "听完了" : `${formatDuration(track.positionSec)} / ${formatDuration(track.durationSec)}`}
          </span>
        </div>
        <div className="home-hero-actions">
          <button className="primary" onClick={() => onResume(from)}>
            ▶ {finished ? "再听一遍" : `从 ${formatDuration(from)} 继续`}
          </button>
          <button className="home-hero-secondary" onClick={onOpen}>
            看歌词
          </button>
        </div>
      </div>
    </div>
  );
}

/** 最近播放的一格，右上角标上次停在哪 */
function RecentTile({ track, onClick }: { track: PlayedTrack; onClick: () => void }) {
  const duration = track.durationSec ?? 0;
  const done = track.lastCompleted || (duration > 0 && duration - track.lastPositionSec < 3);
  const badge = done ? "听完" : duration > 0 ? `${Math.round((track.lastPositionSec / duration) * 100)}%` : "";
  return (
    <button className="tile" onClick={onClick} title={`${track.title} · ${track.artist}`}>
      <span className="home-tile-cover">
        <Cover path={track.coverPath} size={120} rounded={8} />
        {badge !== "" && <span className="home-badge">{badge}</span>}
      </span>
      <span className="tile-title">{track.title}</span>
      <span className="tile-sub">{formatRelative(track.lastPlayed)}</span>
    </button>
  );
}

function TodayPanel({ home, cardsToday }: { home: HomeSummary; cardsToday: number | null }) {
  const { dailyGoalMinutes } = useAppSettings();
  const { today } = home;
  const minutes = Math.floor(today.listenedSec / 60);
  const ratio = Math.min(1, minutes / dailyGoalMinutes);
  const extras = [
    cardsToday !== null ? `今天制卡 ${cardsToday} 张` : "",
    today.streakDays > 0 ? `连续 ${today.streakDays} 天` : "",
  ].filter((s) => s !== "");

  return (
    <section className="home-panel">
      <h2 className="home-panel-title">今日</h2>
      <div className="home-today">
        <Ring ratio={ratio} label={`${Math.round(ratio * 100)}%`} />
        <div className="home-today-text">
          <div className="home-today-big">
            {minutes} <span>/ {dailyGoalMinutes} 分钟</span>
          </div>
          <div className="muted small">
            {today.songs === 0 ? "今天还没听" : `${today.songs} 首 · 听到 ${today.linesHeard.toLocaleString()} 行歌词`}
          </div>
        </div>
      </div>
      {extras.length > 0 && <p className="home-today-extra">{extras.join(" · ")}</p>}
    </section>
  );
}

/** 进度环。SVG 画，不引图表库 */
function Ring({ ratio, label }: { ratio: number; label: string }) {
  const r = 22;
  const c = 2 * Math.PI * r;
  return (
    <svg className="home-ring" viewBox="0 0 56 56" width="56" height="56" role="img" aria-label={`完成 ${label}`}>
      <circle cx="28" cy="28" r={r} className="home-ring-track" />
      {/* 0 的时候不画：圆头线帽会留下一个点，像是有进度 */}
      {ratio > 0 && (
        <circle
          cx="28"
          cy="28"
          r={r}
          className="home-ring-fill"
          strokeDasharray={`${c * ratio} ${c}`}
          transform="rotate(-90 28 28)"
        />
      )}
      <text x="28" y="32" textAnchor="middle">
        {label}
      </text>
    </svg>
  );
}

function CorpusPanel({ home, onMore }: { home: HomeSummary; onMore: () => void }) {
  const { overview, reach } = home;
  const reachPct = reach.totalLemmas > 0 ? Math.round((reach.heardLemmas / reach.totalLemmas) * 100) : 0;
  const rows: [string, string, string?][] = [
    ["曲目", `${overview.tracks} 首 · ${formatLongDuration(overview.totalDurationSec)}`],
    ["歌词", `${overview.lyricLines.toLocaleString()} 行 · ${overview.vocabulary.toLocaleString()} 个词`],
    ["歌手 / 专辑", `${overview.people} 人 · ${overview.albums} 张`],
    ["你听过", `${reach.heardSongs} 首`, "听满 30 秒才算"],
    [
      "碰到过的词",
      `${reachPct}%`,
      `听过的歌里出现过 ${reach.heardLemmas.toLocaleString()} 个实词（名词、动词、形容词、副词），全库 ${reach.totalLemmas.toLocaleString()} 个`,
    ],
  ];
  return (
    <section className="home-panel">
      <header className="home-panel-head">
        <h2 className="home-panel-title">语料库</h2>
        <button className="link-btn" onClick={onMore}>
          完整分析 →
        </button>
      </header>
      <dl className="home-stats">
        {rows.map(([k, v, tip]) => (
          <div key={k} title={tip}>
            <dt>{k}</dt>
            <dd>{v}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

function AddedPanel({ tracks, onOpen }: { tracks: AddedTrack[]; onOpen: (songId: string) => void }) {
  if (tracks.length === 0) return null;
  return (
    <section className="home-panel">
      <h2 className="home-panel-title">最近添加</h2>
      <ul className="home-added">
        {tracks.map((t) => (
          <li key={t.songId}>
            <button onClick={() => onOpen(t.songId)} title={`${t.title} · ${t.artist}`}>
              <Cover path={t.coverPath} size={44} rounded={6} />
              <span className="rank-text">
                <span className="rank-title">{t.title}</span>
                <span className="rank-artist">
                  {t.artist} · {formatRelative(t.addedAt)}
                </span>
              </span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
