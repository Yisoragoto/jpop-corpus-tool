/**
 * 曲目 / 歌词 / 语料的共享状态。
 *
 * 提到 App 一级而不是留在某个页面里，是因为**跨页跳转要保持连续**：
 * 在 KWIC 页点一条命中，要能跳到曲库页的那一行并开始播放。
 * 这是要求书第十八条那句「不应该感觉自己在几个独立的软件之间切换」
 * 在状态层面的落地。
 *
 * 播放状态不在这里——那在 Rust 引擎里，`usePlayback` 只做订阅。
 */

import { useCallback, useRef, useState } from "react";

import { api, type Credit, type LyricLine, type PlaybackState, type Track, type WordInCorpus } from "./api";
import { getActiveScope, mergeSubcorpus } from "./subcorpusScope";
import type { WordJump } from "./wordCloud";

/** 右栏例句最多列这么多句（原来是 24，常用词一下就截断了） */
export const WORD_EXAMPLE_LIMIT = 1000;
import { tr } from "./i18n";

export interface LibraryState {
  tracks: Track[];
  selected: Track | null;
  lyrics: LyricLine[];
  credits: Credit[];
  lyricsLoading: boolean;
  word: WordInCorpus | null;
  /** 例句限定在哪（从词云带过来的歌手、年份）；null 是整个库（或当前子语料） */
  wordJump: WordJump | null;
  wordLoading: boolean;
  activeLemma: string | null;
  /** 打开曲目后要滚到的那一行。消费掉就清空，避免下次渲染又滚一遍。 */
  pendingScrollUtterance: number | null;
  /** 曲库列表当前限定在哪张专辑。null 表示全部。 */
  albumFilter: { albumId: number; title: string; tracks: Track[] } | null;
}

export interface LibraryActions {
  setTracks: (tracks: Track[]) => void;
  /**
   * 打开曲目：歌词栏换成这首。**看和放是两件事**——
   *
   * - `autoPlay` 不是 false：同时从头（或 `seekTo`）开播；
   * - `autoPlay: false`：只是看。播放条里已经有别的歌（在放、或者放到一半暂停着）就不动它；
   *   播放条空着、放完了、或者里面只是上一首「看过没放」的歌时，把这首备上（不开播），按空格就能放。
   */
  openTrack: (
    track: Track,
    opts?: { scrollTo?: number; seekTo?: number | null; autoPlay?: boolean },
  ) => Promise<void>;
  /** 按 id 打开，曲目不在当前列表里时会去查。 */
  openTrackById: (
    songId: string,
    opts?: { scrollTo?: number; seekTo?: number | null; autoPlay?: boolean },
  ) => Promise<boolean>;
  /** 只放不看：播放条换成这首并开播，歌词栏、中栏停在原处。队列、试听、时间轴里点歌都走这个 */
  playTrack: (track: Track, opts?: { seekTo?: number | null }) => Promise<boolean>;
  /** 同上，按 id；曲目不在当前列表里时会去查 */
  playTrackById: (songId: string, opts?: { seekTo?: number | null }) => Promise<boolean>;
  /** 查一个词的例句。`jump` 是从词云来时带的范围；不给就是整个库 */
  openWord: (lemma: string, jump?: WordJump | null) => Promise<void>;
  /** 把曲库列表限定到某张专辑。传 null 清除。 */
  setAlbumFilter: (album: { albumId: number; title: string } | null) => Promise<void>;
  clearWord: () => void;
  consumeScroll: () => void;
  /** 曲库改了（编辑、删除、找回音频）之后重拉列表；动的是当前这首就一并刷新，删掉了就清空 */
  refreshAfterChange: (songIds: string[], removed: boolean) => Promise<void>;
  /** 只重拉当前这首的歌词和署名。补歌词、换歌词之后用——列表那一层没变，不必整个重拉。 */
  reloadLyrics: () => Promise<void>;
}

/**
 * 「只是看」的时候能不能把这首备进播放条：播放条空着、放完了，或者里面是上一首备上之后一直没放的歌。
 * 在放的、放到一半暂停的，都不能动——那是用户正在听的。
 */
export function canCue(now: Pick<PlaybackState, "songId" | "playState" | "positionSec">, cuedId: string | null): boolean {
  if (now.playState === "empty" || now.playState === "ended") return true;
  return now.playState === "paused" && cuedId !== null && now.songId === cuedId && now.positionSec < 0.5;
}

export function useLibrary(
  audioReady: boolean,
  onError: (message: string) => void,
  /** 现在播放条里是什么。只在打开曲目的那一刻读一次，所以给函数而不是值 */
  getPlayback: () => PlaybackState,
): [LibraryState, LibraryActions] {
  const [tracks, setTracks] = useState<Track[]>([]);
  const [selected, setSelected] = useState<Track | null>(null);
  const [lyrics, setLyrics] = useState<LyricLine[]>([]);
  const [credits, setCredits] = useState<Credit[]>([]);
  const [lyricsLoading, setLyricsLoading] = useState(false);
  const [word, setWord] = useState<WordInCorpus | null>(null);
  const [wordLoading, setWordLoading] = useState(false);
  const [wordJump, setWordJump] = useState<WordJump | null>(null);
  const [activeLemma, setActiveLemma] = useState<string | null>(null);
  const [pendingScrollUtterance, setPendingScroll] = useState<number | null>(null);
  const [albumFilter, setAlbumFilterState] =
    useState<LibraryState["albumFilter"]>(null);
  /** 「只是看」时备进播放条、还没放过的那首。下一次看别的歌可以把它换掉 */
  const cued = useRef<string | null>(null);

  const openTrack = useCallback<LibraryActions["openTrack"]>(
    async (track, opts = {}) => {
      setSelected(track);
      setLyricsLoading(true);
      setLyrics([]);
      setCredits([]);
      setPendingScroll(opts.scrollTo ?? null);
      try {
        const [lines, creditRows] = await Promise.all([
          api.lyrics(track.id),
          api.trackCredits(track.id),
        ]);
        setLyrics(lines);
        setCredits(creditRows);
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err));
      } finally {
        setLyricsLoading(false);
      }

      if (!audioReady || !track.audioPath) return;
      const play = opts.autoPlay !== false;
      if (!play && !canCue(getPlayback(), cued.current)) return;
      try {
        await api.audioLoad(track.id, { positionSec: opts.seekTo ?? null, autoplay: play });
        cued.current = play ? null : track.id;
      } catch (err) {
        // 音频出问题不该挡住歌词浏览，只提示
        onError(err instanceof Error ? err.message : String(err));
      }
    },
    [audioReady, onError, getPlayback],
  );

  const playTrack = useCallback<LibraryActions["playTrack"]>(
    async (track, opts = {}) => {
      if (!audioReady) return false;
      if (!track.audioPath) {
        onError(tr("《{title}》没有音频文件", { title: track.title }));
        return false;
      }
      try {
        await api.audioLoad(track.id, { positionSec: opts.seekTo ?? null, autoplay: true });
        cued.current = null;
        return true;
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err));
        return false;
      }
    },
    [audioReady, onError],
  );

  const openTrackById = useCallback<LibraryActions["openTrackById"]>(
    async (songId, opts = {}) => {
      const known = tracks.find((t) => t.id === songId);
      const track = known ?? (await api.getTrack(songId).catch(() => null));
      if (!track) {
        onError(tr("找不到曲目 {songId}", { songId }));
        return false;
      }
      await openTrack(track, opts);
      return true;
    },
    [tracks, openTrack, onError],
  );

  const playTrackById = useCallback<LibraryActions["playTrackById"]>(
    async (songId, opts = {}) => {
      const known = tracks.find((t) => t.id === songId) ?? (selected?.id === songId ? selected : undefined);
      const track = known ?? (await api.getTrack(songId).catch(() => null));
      if (!track) {
        onError(tr("找不到曲目 {songId}", { songId }));
        return false;
      }
      return playTrack(track, opts);
    },
    [tracks, selected, playTrack, onError],
  );

  const openWord = useCallback(
    async (lemma: string, jump: WordJump | null = null) => {
      setActiveLemma(lemma);
      setWordJump(jump);
      setWordLoading(true);
      try {
        const scope = jump
          ? mergeSubcorpus({ performerIds: jump.performerIds, yearFrom: jump.yearFrom, yearTo: jump.yearTo }, getActiveScope())
          : getActiveScope()?.filter;
        setWord(await api.wordInCorpus(lemma, selected?.id, WORD_EXAMPLE_LIMIT, scope));
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err));
      } finally {
        setWordLoading(false);
      }
    },
    [selected, onError],
  );

  const clearWord = useCallback(() => {
    setWord(null);
    setWordJump(null);
    setActiveLemma(null);
  }, []);

  const setAlbumFilter = useCallback<LibraryActions["setAlbumFilter"]>(
    async (album) => {
      if (!album) return setAlbumFilterState(null);
      try {
        const tracks = await api.albumTracks(album.albumId);
        setAlbumFilterState({ ...album, tracks });
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err));
      }
    },
    [onError],
  );

  const consumeScroll = useCallback(() => setPendingScroll(null), []);

  const reloadLyrics = useCallback<LibraryActions["reloadLyrics"]>(async () => {
    if (selected === null) return;
    setLyricsLoading(true);
    try {
      const [lines, creditRows] = await Promise.all([
        api.lyrics(selected.id),
        api.trackCredits(selected.id),
      ]);
      setLyrics(lines);
      setCredits(creditRows);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setLyricsLoading(false);
    }
  }, [selected, onError]);

  const refreshAfterChange = useCallback<LibraryActions["refreshAfterChange"]>(
    async (songIds, removed) => {
      try {
        setTracks(await api.listTracks(1000));
        if (albumFilter) {
          const albumTracks = await api.albumTracks(albumFilter.albumId);
          setAlbumFilterState(albumTracks.length > 0 ? { ...albumFilter, tracks: albumTracks } : null);
        }
        if (selected !== null && songIds.includes(selected.id)) {
          if (removed) {
            setSelected(null);
            setLyrics([]);
            setCredits([]);
            setWord(null);
            setActiveLemma(null);
          } else {
            const [track, creditRows] = await Promise.all([api.getTrack(selected.id), api.trackCredits(selected.id)]);
            if (track) setSelected(track);
            setCredits(creditRows);
          }
        }
      } catch (err) {
        onError(err instanceof Error ? err.message : String(err));
      }
    },
    [albumFilter, selected, onError],
  );

  return [
    {
      tracks,
      selected,
      lyrics,
      credits,
      lyricsLoading,
      word,
      wordJump,
      wordLoading,
      activeLemma,
      pendingScrollUtterance,
      albumFilter,
    },
    {
      setTracks,
      openTrack,
      openTrackById,
      playTrack,
      playTrackById,
      openWord,
      clearWord,
      consumeScroll,
      setAlbumFilter,
      refreshAfterChange,
      reloadLyrics,
    },
  ];
}
