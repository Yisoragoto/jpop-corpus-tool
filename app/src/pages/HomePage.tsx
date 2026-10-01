/**
 * 首页 = 分岔口，不是仪表盘。
 *
 * 排版规则（全项目统一，见 `components/Section.tsx`）：一块只有一行标题，
 * 内容直接铺在页底色上，**不再每块套一个方框**。
 *
 * **每一块都要能点进去**：点曲目进曲库、点专辑把曲库限定到这张专辑、
 * 点词进语料。点不动的东西就是摆设，不如不放——年代 / 流派这类没有去处的分面
 * 已经移回分析页。
 *
 * 顺序上先给「库里有什么」再给「你听过什么」：打开软件第一眼看到的是语料，
 * 这是 Corpus First 在版面上的体现。
 */

import { useCallback, useEffect, useState } from "react";

import {
  api,
  formatDuration,
  formatLongDuration,
  POS_LABELS,
  type Album,
  type HomeSummary,
  type PlayedTrack,
  type Track,
} from "../api";
import { Cover } from "../components/Cover";
import { Section, Shelf } from "../components/Section";
import type { LibraryActions } from "../useLibrary";

interface Props {
  actions: LibraryActions;
  onNavigate: (route: "library" | "kwic" | "analytics") => void;
  onError: (message: string) => void;
  /** 播放状态变了就刷新——听完一首之后「最近播放」应当立刻反映 */
  playbackSongId: string;
}

export function HomePage({ actions, onNavigate, onError, playbackSongId }: Props) {
  const [home, setHome] = useState<HomeSummary | null>(null);

  const load = useCallback(() => {
    api
      .homeSummary()
      .then(setHome)
      .catch((e) => onError(String(e.message ?? e)));
  }, [onError]);

  useEffect(load, [load]);
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

  const openAlbum = useCallback(
    async (album: Album) => {
      // 专辑没有独立页面：把曲库限定到这张专辑，正好补上 Artist → Album → Track 的中间一级
      await actions.setAlbumFilter({ albumId: album.id, title: album.title });
      onNavigate("library");
    },
    [actions, onNavigate],
  );

  if (!home) return <div className="page muted pad">加载中…</div>;

  const { overview } = home;

  return (
    <div className="page home">
      <p className="home-summary">
        <b>{overview.tracks}</b> 首 · <b>{overview.albums}</b> 张专辑 · <b>{overview.people}</b> 人 ·{" "}
        <b>{overview.lyricLines.toLocaleString()}</b> 行歌词 · <b>{overview.vocabulary.toLocaleString()}</b> 个词 ·{" "}
        {formatLongDuration(overview.totalDurationSec)}
        <button className="link-btn" onClick={() => onNavigate("analytics")}>
          完整分析 →
        </button>
      </p>

      <Section
        title="继续听"
        action={
          <button className="link-btn" onClick={() => onNavigate("library")}>
            去曲库 →
          </button>
        }
        {...(home.recentlyPlayed.length === 0 ? { empty: "还没有收听记录。听满 5 秒的曲目会出现在这里。" } : {})}
      >
        <Shelf>
          {home.recentlyPlayed.map((t) => (
            <TrackTile
              key={t.songId}
              cover={t.coverPath}
              title={t.title}
              meta={formatRelative(t.lastPlayed)}
              sub={t.artist}
              onClick={() => void open(t.songId)}
            />
          ))}
        </Shelf>
      </Section>

      <Section
        title="专辑"
        hint={`${overview.albums} 张`}
        {...(home.albums.length === 0 ? { empty: "还没有专辑信息，刮削之后会有。" } : {})}
      >
        <Shelf>
          {home.albums.map((a) => (
            <TrackTile
              key={a.id}
              cover={a.artworkPath}
              title={a.title}
              sub={a.albumArtist}
              meta={`${a.trackCount} 首${a.year ? ` · ${a.year}` : ""}`}
              onClick={() => void openAlbum(a)}
            />
          ))}
        </Shelf>
      </Section>

      <Section title="收藏" show={home.favorites.length > 0}>
        <Shelf>
          {home.favorites.map((t: Track) => (
            <TrackTile
              key={t.id}
              cover={t.coverPath}
              title={t.title}
              sub={t.artist}
              meta={t.durationSec === null ? "" : formatDuration(t.durationSec)}
              onClick={() => void open(t.id)}
            />
          ))}
        </Shelf>
      </Section>

      <Section
        title="从这些词开始"
        hint="语料里最常出现的名词"
        action={
          <button className="link-btn" onClick={() => onNavigate("kwic")}>
            KWIC 检索 →
          </button>
        }
      >
        <div className="word-cloud">
          {home.topWords.map((w) => (
            <button
              key={`${w.lemma}-${w.pos}`}
              className="word-chip"
              title={`${POS_LABELS[w.pos] ?? w.pos} · ${w.freq} 次 · ${w.songCount} 首`}
              onClick={async () => {
                await actions.openWord(w.lemma);
                onNavigate("library");
              }}
            >
              {w.lemma}
              <em>{w.freq}</em>
            </button>
          ))}
        </div>
      </Section>

      <Section title="听得最多" show={home.mostPlayed.length > 0}>
        <ol className="rank-list">
          {home.mostPlayed.slice(0, 8).map((t: PlayedTrack, i) => (
            <li key={t.songId}>
              <button onClick={() => void open(t.songId)}>
                <span className="rank-n">{i + 1}</span>
                <Cover path={t.coverPath} size={32} rounded={4} />
                <span className="rank-text">
                  <span className="rank-title">{t.title}</span>
                  <span className="rank-artist">{t.artist}</span>
                </span>
                <span className="rank-meta">
                  {t.playCount} 次 · {formatLongDuration(t.totalListenedSec)}
                </span>
              </button>
            </li>
          ))}
        </ol>
      </Section>
    </div>
  );
}

/** 架子上的一格：封面 + 两行字。曲目、专辑、收藏共用 */
function TrackTile({
  cover,
  title,
  sub,
  meta,
  onClick,
}: {
  cover: string | null | undefined;
  title: string;
  sub: string;
  meta?: string;
  onClick: () => void;
}) {
  return (
    <button className="tile" onClick={onClick} title={`${title} · ${sub}`}>
      <Cover path={cover} size={140} rounded={8} />
      <span className="tile-title">{title}</span>
      <span className="tile-sub">{sub}</span>
      {meta !== undefined && meta !== "" && <span className="tile-meta">{meta}</span>}
    </button>
  );
}

/** ISO 时间 → 「3 分钟前」。收听记录看的是「多久以前」，不是具体几点。 */
function formatRelative(iso: string): string {
  const then = Date.parse(iso);
  if (!Number.isFinite(then)) return "";
  const seconds = Math.max(0, (Date.now() - then) / 1000);
  if (seconds < 60) return "刚刚";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} 分钟前`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} 小时前`;
  return `${Math.floor(seconds / 86400)} 天前`;
}
