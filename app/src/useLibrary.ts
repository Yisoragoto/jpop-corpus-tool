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

import { useCallback, useState } from "react";

import { api, type Credit, type LyricLine, type Track, type WordInCorpus } from "./api";
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
  /** 打开曲目。autoPlay 为真且引擎可用时同时开播。 */
  openTrack: (
    track: Track,
    opts?: { scrollTo?: number; seekTo?: number | null; autoPlay?: boolean },
  ) => Promise<void>;
  /** 按 id 打开，曲目不在当前列表里时会去查。 */
  openTrackById: (
    songId: string,
    opts?: { scrollTo?: number; seekTo?: number | null; autoPlay?: boolean },
  ) => Promise<boolean>;
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

export function useLibrary(
  audioReady: boolean,
  onError: (message: string) => void,
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
      try {
        await api.audioLoad(track.id, { positionSec: opts.seekTo ?? null, autoplay: opts.autoPlay !== false });
      } catch (err) {
        // 音频出问题不该挡住歌词浏览，只提示
        onError(err instanceof Error ? err.message : String(err));
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
      openWord,
      clearWord,
      consumeScroll,
      setAlbumFilter,
      refreshAfterChange,
      reloadLyrics,
    },
  ];
}
