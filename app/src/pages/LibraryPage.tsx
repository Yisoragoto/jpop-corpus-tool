/**
 * 曲库页：Library → Track → Lyrics → 点词 → Corpus。
 *
 * 三栏。中栏的歌词跟随播放，实词可点；右栏是该词在全语料里的样子，
 * 点例句可以跳到另一首歌——这就是要求书第十八条那条回路。
 */

import { Fragment, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";

import {
  api,
  CONTENT_POS,
  formatDuration,
  POS_LABELS,
  ROLE_LABELS,
  type PlaybackState,
  type Track,
} from "../api";
import { Stat } from "../components/Stat";
import { useArtistPhotos } from "../components/PerformerPicker";
import { Cover } from "../components/Cover";
import { Avatar } from "../components/Avatar";
import { AudioRepair } from "../components/AudioRepair";
import { LyricsStage } from "../components/LyricsStage";
import { SongEditor } from "../components/SongEditor";
import { LyricsTools } from "../components/LyricsTools";
import { LyricsFill } from "../components/LyricsFill";
import { lyricFontStack, useLyricFontFiles } from "../fonts";
import { retryFurigana, useLyricsDisplay, useSongFurigana } from "../lyricsDisplay";
import { useAppSettings } from "../settings";
import { csvProblem, describeDelete, describeEdit, libraryApi, type MissingAudio } from "../libraryAdmin";
import { segmentLine, type Piece } from "../lyricSegments";
import type { LibraryActions, LibraryState } from "../useLibrary";
import { useCurrentLine, useLineLoop } from "../usePlayback";
import { dictApi, type FindTermsResult } from "../dict/api";
import { LookupResults } from "../dict/LookupResults";
import { MineButton } from "../dict/MineButton";
import { mineApi, useMineSettings, type MineCheck } from "../dict/mine";
import { ColumnSplitter, useColumnWidths } from "../components/ColumnSplitter";

/** 查词是从哪一行第几个词发起的（制卡时用来取例句、切音频）。点释义里的链接查词时没有。 */
interface LineContext {
  songId: string;
  utteranceId: number;
  tokenIndex: number;
}

/** 右栏里的查词：从点的那个词起往后查（和 Yomitan 一样取最长匹配，活用形会还原）。 */
interface LyricLookup {
  text: string;
  line: LineContext | null;
  result: FindTermsResult | null;
  loading: boolean;
  error: string | null;
}

/** 查词结果的命中长度按 Unicode 字符计，这里也按字符截。 */
function matchedPrefix(text: string, length: number): string {
  return Array.from(text).slice(0, length).join("");
}

/** 一个词里的字：有注音的套 ruby */
function renderPieces(pieces: Piece[]): ReactNode {
  return pieces.map((piece, k) =>
    piece.reading === undefined ? (
      <Fragment key={k}>{piece.text}</Fragment>
    ) : (
      <ruby key={k}>
        {piece.text}
        <rt>{piece.reading}</rt>
      </ruby>
    ),
  );
}

/** 右栏看的是词典还是例句，记住——下次查词还停在你上次看的那一页。 */
const CORPUS_TAB_KEY = "jp.library.corpusTab";

type CorpusTab = "dict" | "examples";

function readStoredTab(): CorpusTab {
  try {
    return localStorage.getItem(CORPUS_TAB_KEY) === "examples" ? "examples" : "dict";
  } catch {
    return "dict";
  }
}

/** 列表还是网格、收起了哪些歌手：只关乎这台机器上的这个用户，存 localStorage */
const VIEW_KEY = "jp.library.view";
const COLLAPSED_KEY = "jp.library.collapsed";

function IconList() {
  return (
    <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
      <path d="M4 7h16M4 12h16M4 17h16" />
    </svg>
  );
}

function IconGrid() {
  return (
    <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" aria-hidden="true">
      <rect x="4" y="4" width="7" height="7" rx="1.5" />
      <rect x="13" y="4" width="7" height="7" rx="1.5" />
      <rect x="4" y="13" width="7" height="7" rx="1.5" />
      <rect x="13" y="13" width="7" height="7" rx="1.5" />
    </svg>
  );
}

interface Props {
  library: LibraryState;
  actions: LibraryActions;
  playback: PlaybackState;
  audioReady: boolean;
  onAudioError: (message: string) => void;
  /** 删歌、改歌之后总览的数字要重拉 */
  onLibraryChanged: () => void;
  /** 外面（播放条左下角那张卡片）请求打开全屏歌词 */
  stageWanted?: boolean;
  /** 收到了，把请求清掉 */
  onStageConsumed?: () => void;
}

export function LibraryPage({
  library,
  actions,
  playback,
  audioReady,
  onAudioError,
  onLibraryChanged,
  stageWanted = false,
  onStageConsumed,
}: Props) {
  const { tracks, selected, lyrics, credits, lyricsLoading, word, wordLoading, activeLemma } =
    library;
  const [filter, setFilter] = useState("");
  const [favorite, setFavorite] = useState(false);
  const lyricsRef = useRef<HTMLDivElement>(null);
  const currentLineRef = useRef<HTMLParagraphElement>(null);
  const [lookup, setLookup] = useState<LyricLookup | null>(null);
  const lookupSeq = useRef(0);
  const [corpusTab, setCorpusTab] = useState<CorpusTab>(readStoredTab);
  const pickTab = useCallback((tab: CorpusTab) => {
    setCorpusTab(tab);
    try {
      localStorage.setItem(CORPUS_TAB_KEY, tab);
    } catch {
      // 存不了就只在这次会话里生效
    }
  }, []);

  // 连点几个词时只认最后一次的结果；查的时候先留着上一次的结果，免得右栏闪一下
  const lookUp = useCallback((text: string, line: LineContext | null = null) => {
    const seq = ++lookupSeq.current;
    setLookup((prev) => ({ text, line, result: prev?.result ?? null, loading: true, error: null }));
    dictApi.lookup(text).then(
      (result) => {
        if (lookupSeq.current === seq) setLookup({ text, line, result, loading: false, error: null });
      },
      (e: unknown) => {
        if (lookupSeq.current === seq) {
          setLookup({ text, line, result: null, loading: false, error: String((e as { message?: string })?.message ?? e) });
        }
      },
    );
  }, []);
  // 释义里的站内链接：没有例句上下文
  const lookUpLink = useCallback((text: string) => lookUp(text, null), [lookUp]);

  // 查到结果后问一次 Anki：哪些词已经有卡了
  const mineSettings = useMineSettings();
  const [mineCheck, setMineCheck] = useState<{ result: FindTermsResult; check: MineCheck } | null>(null);
  const lookupResult = lookup?.result ?? null;
  useEffect(() => {
    if (lookupResult === null || lookupResult.dictionaryEntries.length === 0) {
      setMineCheck(null);
      return;
    }
    let alive = true;
    const expressions = lookupResult.dictionaryEntries.map((e) => e.headwords[0]?.term ?? "");
    mineApi.check(mineSettings.model, expressions).then(
      (check) => {
        if (alive) setMineCheck({ result: lookupResult, check });
      },
      () => {
        if (alive) setMineCheck(null);
      },
    );
    return () => {
      alive = false;
    };
  }, [lookupResult, mineSettings.model]);

  // 列表 / 网格，以及收起了哪些歌手。都记在 localStorage，下次打开还是这样
  const [view, setView] = useState<"list" | "grid">(() => (localStorage.getItem(VIEW_KEY) === "grid" ? "grid" : "list"));
  // 两边的宽度可以拖，并且记住。网格视图左边本来就该宽一些，所以两套各记各的。
  const columns = useColumnWidths(
    view === "grid" ? "library-grid" : "library",
    view === "grid" ? { left: 420, right: 320 } : { left: 260, right: 320 },
  );
  useEffect(() => {
    try {
      localStorage.setItem(VIEW_KEY, view);
    } catch {
      // 存不了就只在这次会话里生效
    }
  }, [view]);
  const [collapsed, setCollapsed] = useState<Set<string>>(() => {
    try {
      const raw = localStorage.getItem(COLLAPSED_KEY);
      return new Set(raw === null ? [] : (JSON.parse(raw) as string[]));
    } catch {
      return new Set();
    }
  });
  // 网格里点开的那位歌手（`gridByArtist` 开着时）。只活在这次会话里：
  // 下次打开还是从歌手墙开始，和 Spotify 的「艺人」一样
  const [gridArtist, setGridArtist] = useState<string | null>(null);
  const settings = useAppSettings();
  const artistPhotos = useArtistPhotos();

  // 曲库维护：编辑、删除（两步确认，不弹窗）、找回丢了的音频
  const [panel, setPanel] = useState<null | "edit" | "repair">(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  // 维护操作的结果说明，记着是哪首歌的：删掉之后（没有选中的歌）也要看得到删了什么
  const [maintainNote, setMaintainNote] = useState<{ forSong: string | null; text: string } | null>(null);
  const note = maintainNote !== null && maintainNote.forSong === (selected?.id ?? null) ? maintainNote.text : "";
  const [missing, setMissing] = useState<MissingAudio[]>([]);
  const refreshMissing = useCallback(() => {
    libraryApi.missingAudio().then(setMissing, () => undefined);
  }, []);
  useEffect(() => refreshMissing(), [refreshMissing]);
  const missingIds = useMemo(() => new Set(missing.map((m) => m.songId)), [missing]);
  useEffect(() => {
    setConfirmDelete(false);
    setPanel((p) => (p === "edit" ? null : p));
  }, [selected?.id]);

  const deleteSelected = useCallback(async () => {
    if (!selected) return;
    const id = selected.id;
    try {
      const result = await libraryApi.remove(id);
      setConfirmDelete(false);
      await actions.refreshAfterChange([id], true);
      onLibraryChanged();
      refreshMissing();
      setMaintainNote({ forSong: null, text: describeDelete(result) });
      const problem = csvProblem(result.csv);
      if (problem) onAudioError(problem);
    } catch (err) {
      onAudioError(String((err as { message?: string })?.message ?? err));
    }
  }, [selected, actions, onLibraryChanged, refreshMissing, onAudioError]);

  // 换歌时收起查词
  useEffect(() => {
    lookupSeq.current++;
    setLookup(null);
  }, [selected?.id]);

  // 换歌时重新查收藏状态
  useEffect(() => {
    if (!selected) return setFavorite(false);
    api.isFavorite("song", selected.id).then(setFavorite).catch(() => setFavorite(false));
  }, [selected]);

  const toggleFavorite = useCallback(async () => {
    if (!selected) return;
    try {
      setFavorite(await api.toggleFavorite("song", selected.id));
    } catch (err) {
      onAudioError(err instanceof Error ? err.message : String(err));
    }
  }, [selected, onAudioError]);

  // 字体、字号、振假名等显示设置；振假名开着时取这首歌的注音；选了自带或导入的字体就按文件加载
  // 有歌词、一个词都没分的歌：查不了词、没有振假名，而且看上去和别的歌没两样。
  // 词典装好之后后端会顺手补，但没词典时入库、或者迁移时被当成「已在库中」跳过的歌，
  // 用户只看得到症状——所以就在歌词上方说清楚，并给一个就地补的按钮。
  const untokenized = !lyricsLoading && lyrics.length > 0 && lyrics.every((line) => line.tokens.length === 0);
  const [tokenizing, setTokenizing] = useState(false);
  const tokenizeNow = useCallback(async () => {
    setTokenizing(true);
    try {
      await api.tokenizeMissingRun();
      retryFurigana();
      await actions.reloadLyrics();
      onLibraryChanged();
    } catch (err) {
      onAudioError(err instanceof Error ? err.message : String(err));
    } finally {
      setTokenizing(false);
    }
  }, [actions, onLibraryChanged, onAudioError]);
  // 歌词状态在外壳里，换页不清：在设置页补完分词回来，这首歌拿的还是补之前那份。
  // 挂载时看一眼，没分词就重读一次（只有这种歌才多读这一次）。
  useEffect(() => {
    if (untokenized) void actions.reloadLyrics();
    // 只在进这一页时看一次，所以依赖是空的
  }, []);

  const display = useLyricsDisplay();
  const furigana = useSongFurigana(selected?.id, display.furigana, display.furiganaMode, onAudioError);
  useLyricFontFiles([display.fontFamily, display.fallbackFamily], onAudioError);
  // 全屏歌词；stageSide 是全屏里右边的查词栏（点了词才出来）
  const [stageOpen, setStageOpen] = useState(false);
  // 播放条左下角那张卡片点一下就进全屏。**请求 + 消费**，和跨页跳转那条路一样：
  // 点卡片时这一页往往还没挂载（人在分析页），用「计数变了才算」会把请求漏掉——
  // 挂载时读到的就是新值，看着像没变过。
  useEffect(() => {
    if (!stageWanted) return;
    onStageConsumed?.();
    if (selected !== null) setStageOpen(true);
  }, [stageWanted, selected, onStageConsumed]);
  const [stageSide, setStageSide] = useState(false);
  const closeStage = useCallback(() => setStageOpen(false), []);
  // 每行拆成「词之间的原文 + 词（里面带注音）」。播放轮询每秒重渲染 10 次，拆分结果要缓存
  const lineSegments = useMemo(
    () =>
      lyrics.map((line) => {
        if (line.tokens.length === 0) return null;
        return (
          segmentLine(line.text, line.tokens, furigana?.get(line.utteranceId) ?? []) ??
          // 注音对不上就不注音，词照样能点
          segmentLine(line.text, line.tokens, [])
        );
      }),
    [lyrics, furigana],
  );
  const lyricsStyle = {
    "--lyric-size": `${display.fontSize}px`,
    "--ruby-size": `${display.rubySize}px`,
    "--lyric-gap": `${display.lineGap}px`,
    "--lyric-letter": `${display.letterSpacing}px`,
    "--lyric-font": lyricFontStack(display.fontFamily, display.fallbackFamily),
  } as CSSProperties;

  // 当前行由位置推导，不是引擎给的——见 usePlayback.ts。
  // **歌词属于 selected，位置属于正在放的那一首**，换歌的一瞬间两者不是同一首。
  const lyricsArePlaying = selected !== null && playback.songId === selected.id;
  const currentLine = useCurrentLine(lyrics, playback.positionSec, lyricsArePlaying);
  const loopLine = useLineLoop(lyrics, playback.durationSec);

  // 换歌先回到开头。歌词栏是个常驻的滚动容器，内容换了 scrollTop 不会跟着变，
  // 上一首滚到了 3:38 的位置，新歌一进来就停在半腰。
  // 放在跨页跳转那个 effect 前面：那边用 rAF 滚到指定行，后发生，赢。
  useEffect(() => {
    lyricsRef.current?.scrollTo({ top: 0 });
  }, [selected?.id]);

  // 上边那道渐隐只在滚起来之后才有：停在顶上时淡的就是曲目标题，那样难看。
  useEffect(() => {
    const box = lyricsRef.current;
    if (box === null) return;
    let frame = 0;
    const measure = () => {
      frame = 0;
      box.classList.toggle("scrolled", box.scrollTop > 8);
    };
    const onScroll = () => {
      if (frame === 0) frame = requestAnimationFrame(measure);
    };
    measure();
    box.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      box.removeEventListener("scroll", onScroll);
      if (frame !== 0) cancelAnimationFrame(frame);
    };
  }, [selected?.id]);

  // 跨页跳转过来时滚到指定行
  useEffect(() => {
    const target = library.pendingScrollUtterance;
    if (target === null || lyrics.length === 0) return;
    requestAnimationFrame(() => {
      lyricsRef.current
        ?.querySelector(`[data-utt="${target}"]`)
        ?.scrollIntoView({ block: "center", behavior: "smooth" });
      actions.consumeScroll();
    });
  }, [library.pendingScrollUtterance, lyrics, actions]);

  // 歌词跟随。只在播放时滚——用户手动翻歌词时被抢走滚动位置很烦人。
  // 也只在看的就是正在放的那一首时滚：否则换歌、或者一边放一边翻别的歌时，
  // 另一首的位置会把这一屏歌词拖走。
  useEffect(() => {
    if (!lyricsArePlaying || playback.playState !== "playing") return;
    currentLineRef.current?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [currentLine, playback.playState, lyricsArePlaying]);

  /** 一行歌词：词之间的原文 + 可点的词（里面带振假名）。普通视图和全屏共用 */
  const renderLineText = (idx: number): ReactNode => {
    const line = lyrics[idx];
    if (line === undefined || selected === null) return null;
    const songId = selected.id;
    return (
      <span className="text" lang="ja">
        {line.tokens.length === 0
          ? line.text
          : (lineSegments[idx] ?? line.tokens.map((_, index) => ({ kind: "token" as const, index, pieces: [] }))).map((segment, k) => {
              if (segment.kind === "gap") return <Fragment key={`gap-${k}`}>{segment.text}</Fragment>;
              const i = segment.index;
              const token = line.tokens[i];
              if (token === undefined) return null;
              // 分词对不齐原文时（拆分结果是 null）退回只显示词
              const content = segment.pieces.length > 0 ? renderPieces(segment.pieces) : token.surface;
              // 从这个词起到行尾，交给词典做最长匹配
              const rest = () => line.tokens.slice(i).map((t) => t.surface).join("");
              const context = { songId, utteranceId: line.utteranceId, tokenIndex: i };
              return CONTENT_POS.has(token.pos) ? (
                <button
                  key={i}
                  className={`token ${activeLemma === token.lemma ? "on" : ""}`}
                  onClick={(e) => {
                    // 全屏里点行是跳转，点词不跳
                    e.stopPropagation();
                    void actions.openWord(token.lemma);
                    lookUp(rest(), context);
                    setStageSide(true);
                  }}
                  title={`${token.lemma} · ${POS_LABELS[token.pos] ?? token.pos}`}
                >
                  {content}
                </button>
              ) : (
                <span
                  key={i}
                  className="token-lookup"
                  onClick={(e) => {
                    e.stopPropagation();
                    lookUp(rest(), context);
                    setStageSide(true);
                  }}
                  title="查词典"
                >
                  {content}
                </span>
              );
            })}
      </span>
    );
  };

  const visible = useMemo(() => {
    // 限定了专辑就只在那张专辑里筛
    const source = library.albumFilter?.tracks ?? tracks;
    const q = filter.trim().toLowerCase();
    if (!q) return source;
    return source.filter(
      (t) => t.title.toLowerCase().includes(q) || t.artist.toLowerCase().includes(q),
    );
  }, [tracks, filter, library.albumFilter]);

  /** 按歌手分组，组内保持原来的顺序；组按曲数多的在前，同数按歌手名 */
  const groups = useMemo(() => {
    const byArtist = new Map<string, Track[]>();
    for (const track of visible) {
      const key = track.artist || "（未知歌手）";
      const list = byArtist.get(key);
      if (list === undefined) byArtist.set(key, [track]);
      else list.push(track);
    }
    return [...byArtist.entries()]
      .map(([artist, list]) => ({ artist, tracks: list }))
      .sort((a, b) => b.tracks.length - a.tracks.length || a.artist.localeCompare(b.artist, "ja"));
  }, [visible]);

  const toggleGroup = useCallback((artist: string) => {
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(artist)) next.delete(artist);
      else next.add(artist);
      try {
        localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...next]));
      } catch {
        // 存不了就只在这次会话里生效
      }
      return next;
    });
  }, []);

  /**
   * 网格视图先显示歌手墙，点一位进去看他的歌（设置里可以关）。
   *
   * `drilled` 是点开的那一位；筛选把他滤掉了就当作没点开，不至于空白一片。
   * 列表视图不参与——那边是可折叠的分组，本来就看得见全部。
   */
  const byArtist = view === "grid" && settings.gridByArtist;
  const drilled = byArtist && gridArtist !== null ? (groups.find((g) => g.artist === gridArtist) ?? null) : null;
  const artistWall = byArtist && drilled === null;

  /**
   * 歌手照片。合作署名（`ずっと真夜中でいいのに。/森カリオペ`）退到第一位演唱者：
   * 信用表本来就是按 `/` 拆的，第一位是主唱，和封面目录的约定一致。
   * 名字本身**精确匹配**，别名不猜——和人物页一样。
   */
  const photoOf = useCallback(
    (artist: string): string =>
      artistPhotos.get(artist) || artistPhotos.get(artist.split("/")[0]?.trim() ?? "") || "",
    [artistPhotos],
  );

  /** 一首歌在左栏里的样子：网格是封面卡片，列表是一行 */
  const trackButton = (track: Track) =>
    view === "grid" ? (
      <button
        key={track.id}
        className={`track-card ${selected?.id === track.id ? "active" : ""}`}
        onClick={() => void actions.openTrack(track)}
        title={`${track.title} · ${track.artist}`}
      >
        <Cover path={track.coverPath} size={118} rounded={8} />
        <span className="track-title">{track.title}</span>
        <span className="track-meta">
          {track.durationSec !== null && formatDuration(track.durationSec)}
          {track.lineCount === 0 && <em className="warn"> · 无歌词</em>}
          {missingIds.has(track.id) && <em className="warn"> · 音频丢失</em>}
        </span>
      </button>
    ) : (
      <button
        key={track.id}
        className={`track with-cover ${selected?.id === track.id ? "active" : ""}`}
        onClick={() => void actions.openTrack(track)}
      >
        <Cover path={track.coverPath} size={36} />
        <span className="track-lines">
          <span className="track-title">{track.title}</span>
          <span className="track-meta">
            {track.artist}
            {track.durationSec !== null && ` · ${formatDuration(track.durationSec)}`}
            {track.lineCount === 0 && <em className="warn"> · 无歌词</em>}
            {missingIds.has(track.id) && <em className="warn"> · 音频丢失</em>}
          </span>
        </span>
      </button>
    );

  // 「词典」这一页的内容。标题行只说查的是哪几个字——折叠交给上面的标签页
  const dictionarySection = lookup !== null && (
    <section className="corpus-dict">
      <h3 className="corpus-dict-head">
        <span className="matched" lang="ja">
          {lookup.result !== null && lookup.result.originalTextLength > 0
            ? matchedPrefix(lookup.text, lookup.result.originalTextLength)
            : matchedPrefix(lookup.text, 8)}
        </span>
        <button className="corpus-dict-close" onClick={() => setLookup(null)} title="关闭这次查词">
          ✕
        </button>
      </h3>
      {(
        <LookupResults
          result={lookup.result}
          loading={lookup.loading}
          error={lookup.error}
          emptyText="词典里没有查到（在「词典」页导入词典）"
          onLookup={lookUpLink}
          compact
          entryActions={(entry) => {
            const check = mineCheck?.result === lookup.result ? mineCheck.check : null;
            const index = lookup.result?.dictionaryEntries.indexOf(entry) ?? -1;
            return (
              <MineButton
                entry={entry}
                context={{
                  lookupText: lookup.text,
                  songId: lookup.line?.songId,
                  utteranceId: lookup.line?.utteranceId,
                  tokenIndex: lookup.line?.tokenIndex,
                  artist: lookup.line?.songId === selected?.id ? selected?.artist : undefined,
                }}
                existing={check !== null && index >= 0 ? check.notes[index] : undefined}
                unavailable={check !== null && !check.connected ? check.message : ""}
                onError={onAudioError}
              />
            );
          }}
        />
      )}
    </section>
  );

  // 右栏：词头（这个词是什么）+ 标签页（词典 / 例句）+ 内容。全屏歌词的查词栏用同一份。
  //
  // 为什么要标签页：原来是「统计 → 词典 → 例句」一路堆下来，想看例句每次都得滚到底，
  // 而词典的释义可以很长。现在两页并列，**选了哪一页记住**——下次查词还停在这一页。
  const examplesSection = word && !wordLoading && (
    <div className="examples">
      {word.examples.map((ex) => (
        <button
          key={ex.utteranceId}
          className={`example ${ex.songId === selected?.id ? "same" : ""}`}
          onClick={() =>
            void actions.openTrackById(ex.songId, {
              scrollTo: ex.utteranceId,
              seekTo: ex.timeSec,
              autoPlay: false,
            })
          }
        >
          <span className="ex-text">{ex.text}</span>
          <span className="ex-meta">
            {ex.artist} · {ex.title}
          </span>
        </button>
      ))}
      {word.examples.length === 0 && <p className="muted">没有例句</p>}
    </div>
  );

  const corpusContent = (
    <>
      {!word && !wordLoading && lookup === null && (
        <p className="muted pad">点歌词里的词：看释义，以及它在整个语料里的样子</p>
      )}
      {wordLoading && <p className="muted pad">查询中…</p>}
      {word && !wordLoading && (
        <>
          <h2 className="lemma">{word.lemma}</h2>
          <div className="numbers">
            <Stat label="出现" value={word.occurrences} align="start" />
            <Stat label="歌曲" value={word.songCount} align="start" />
            <Stat label="歌手" value={word.artistCount} align="start" />
          </div>
          {word.firstYear && (
            <p className="muted small">
              {word.firstYear} — {word.lastYear}
            </p>
          )}
          <div className="chips">
            {word.posDistribution.slice(0, 4).map((p) => (
              <span className="chip" key={p.pos}>
                {POS_LABELS[p.pos] ?? p.pos} {p.count}
              </span>
            ))}
          </div>
        </>
      )}

      {(word !== null || lookup !== null) && !wordLoading && (
        <>
          <div className="segmented corpus-tabs">
            <button className={corpusTab === "dict" ? "on" : ""} onClick={() => pickTab("dict")}>
              词典
            </button>
            <button className={corpusTab === "examples" ? "on" : ""} onClick={() => pickTab("examples")}>
              例句{word ? ` ${word.examples.length}` : ""}
            </button>
          </div>
          {corpusTab === "dict"
            ? dictionarySection || <p className="muted pad">这次没有查词典（点歌词里的词会查）</p>
            : examplesSection || <p className="muted pad">没有例句</p>}
        </>
      )}
    </>
  );

  return (
    <div
      className={`columns ${view === "grid" ? "wide-library" : ""}`}
      style={columns.style}
      ref={columns.container}
    >
      <aside className="pane library">
        {library.albumFilter && (
          <button
            className="album-chip"
            onClick={() => void actions.setAlbumFilter(null)}
            title="点击显示全部曲目"
          >
            <span>{library.albumFilter.title}</span>
            <em>✕</em>
          </button>
        )}
        {missing.length > 0 && (
          <button className="missing-banner" onClick={() => setPanel("repair")} title="扫文件夹或逐首选文件找回">
            ⚠ {missing.length} 首找不到音频文件 · 找回
          </button>
        )}
        <div className="library-bar">
          <input
            className="search"
            placeholder={library.albumFilter ? "在这张专辑里筛选" : "筛选曲目 / 歌手"}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
          <div className="segmented view-toggle">
            <button
              className={view === "list" ? "on" : ""}
              onClick={() => setView("list")}
              title="列表"
              aria-label="列表视图"
            >
              <IconList />
            </button>
            <button
              className={view === "grid" ? "on" : ""}
              onClick={() => setView("grid")}
              title="封面网格"
              aria-label="网格视图"
            >
              <IconGrid />
            </button>
          </div>
        </div>

        <div className={`list ${view === "grid" ? "as-grid" : ""}`}>
          {artistWall && (
            <div className="artist-wall">
              {groups.map((group) => (
                <button
                  key={group.artist}
                  className="artist-card"
                  onClick={() => setGridArtist(group.artist)}
                  title={`${group.artist} · ${group.tracks.length} 首`}
                >
                  <Avatar path={photoOf(group.artist)} name={group.artist} size={118} />
                  <span className="artist-name">{group.artist}</span>
                  <span className="artist-count">{group.tracks.length} 首</span>
                </button>
              ))}
            </div>
          )}

          {drilled !== null && (
            <>
              <div className="artist-head">
                <button className="artist-back" onClick={() => setGridArtist(null)} title="回到所有歌手" aria-label="回到所有歌手">
                  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <path d="m14 8-4 4 4 4" />
                  </svg>
                </button>
                <Avatar path={photoOf(drilled.artist)} name={drilled.artist} size={44} />
                <span className="artist-head-text">
                  <b>{drilled.artist}</b>
                  <span className="muted small">{drilled.tracks.length} 首</span>
                </span>
              </div>
              <div className="group-body">{drilled.tracks.map(trackButton)}</div>
            </>
          )}

          {!byArtist &&
            groups.map((group) => {
              const folded = collapsed.has(group.artist);
              return (
                <section className="track-group" key={group.artist}>
                  <button
                    className={`group-head ${folded ? "folded" : ""}`}
                    onClick={() => toggleGroup(group.artist)}
                    title={folded ? "展开" : "收起"}
                  >
                    <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                      <path d="m8 10 4 4 4-4" />
                    </svg>
                    <span className="group-name">{group.artist}</span>
                    <span className="group-count">{group.tracks.length}</span>
                  </button>
                  {!folded && <div className="group-body">{group.tracks.map(trackButton)}</div>}
                </section>
              );
            })}
          {visible.length === 0 && <p className="muted pad">没有匹配的曲目</p>}
        </div>
      </aside>

      <ColumnSplitter
        edge="left"
        label="曲库栏宽度"
        onDrag={columns.drag}
        onNudge={columns.nudge}
        onReset={columns.reset}
      />

      <main className={`pane lyrics ${furigana !== null ? "with-furigana" : ""}`} ref={lyricsRef} style={lyricsStyle}>
        {panel === "repair" && (
          <AudioRepair
            missing={missing}
            onApplied={(ids) => {
              refreshMissing();
              void actions.refreshAfterChange(ids, false);
              setMaintainNote({ forSong: selected?.id ?? null, text: `找回了 ${ids.length} 首的音频` });
            }}
            onClose={() => setPanel(null)}
            onError={onAudioError}
          />
        )}
        {panel !== "repair" && note && !selected && <p className="maintain-note">{note}</p>}
        {panel !== "repair" && !selected && <p className="muted pad">从左侧选一首歌</p>}
        {panel !== "repair" && selected && (
          <>
            <div className="track-head track-head-row">
              <Cover path={selected.coverPath} size={96} alt="" rounded={6} />
              <div className="track-head-text">
                <h1>
                {selected.title}
                <button
                  className={`fav-btn ${favorite ? "on" : ""}`}
                  onClick={() => void toggleFavorite()}
                  title={favorite ? "取消收藏" : "收藏"}
                >
                  {favorite ? "♥" : "♡"}
                </button>
              </h1>
              <p className="muted">
                {selected.artist}
                {selected.album && ` · ${selected.album}`}
                {selected.year && ` · ${selected.year}`}
              </p>
                {credits.length > 0 && (
                  <ul className="credits">
                    {credits.map((c) => (
                      <li key={`${c.role}-${c.personId}`}>
                        <span className="role">{ROLE_LABELS[c.role] ?? c.role}</span>
                        {c.name}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
              <div className="track-side">
                <LyricsTools display={display} onError={onAudioError} onExpand={() => setStageOpen(true)} />
                <div className="track-actions">
                  <button className="link-btn" onClick={() => setPanel(panel === "edit" ? null : "edit")}>
                    编辑信息
                  </button>
                  {lyrics.length > 0 && (
                    <LyricsFill
                      songId={selected.id}
                      hasLyrics
                      onAttached={() => void actions.reloadLyrics()}
                      onError={onAudioError}
                    />
                  )}
                  {confirmDelete ? (
                    <span className="confirm-delete">
                      删掉《{selected.title}》？歌词和分词一起删，音频文件不动
                      <button className="link-btn danger" onClick={() => void deleteSelected()}>
                        确认删除
                      </button>
                      <button className="link-btn" onClick={() => setConfirmDelete(false)}>
                        取消
                      </button>
                    </span>
                  ) : (
                    <button className="link-btn" onClick={() => setConfirmDelete(true)}>
                      删除
                    </button>
                  )}
                </div>
              </div>
            </div>
            {panel === "edit" && (
              <SongEditor
                key={selected.id}
                track={selected}
                onSaved={(report) => {
                  setPanel(null);
                  setMaintainNote({ forSong: selected.id, text: describeEdit(report) });
                  void actions.refreshAfterChange([selected.id], false);
                  onLibraryChanged();
                  const problem = csvProblem(report.csv);
                  if (problem) onAudioError(problem);
                }}
                onCancel={() => setPanel(null)}
                onError={onAudioError}
              />
            )}
            {note && <p className="maintain-note">{note}</p>}
            {missingIds.has(selected.id) && (
              <p className="warn small">
                这首歌的音频文件找不到了：{selected.audioPath || "（没有记录路径）"}。
                <button className="link-btn" onClick={() => setPanel("repair")}>
                  找回
                </button>
              </p>
            )}

            {untokenized && (
              <p className="warn small">
                这首歌有歌词但还没分词，所以点不了词、也没有振假名（多半是在还没有分词词典时入库的）。
                <button className="link-btn" onClick={() => void tokenizeNow()} disabled={tokenizing}>
                  {tokenizing ? "分词中…" : "现在补分词"}
                </button>
              </p>
            )}
            {lyricsLoading && <p className="muted pad">加载歌词…</p>}
            {!lyricsLoading && lyrics.length === 0 && (
              <div className="empty-state pad">
                <p>这首歌还没有歌词</p>
                <p className="muted small">
                  导入只认音频旁边的同名 .lrc。在线搜一下，或者自己选一份文件——
                  歌词会存进语料库目录的 raw/lyrics_lrc，和导入进来的一样。
                </p>
                <LyricsFill
                  songId={selected.id}
                  hasLyrics={false}
                  onAttached={() => void actions.reloadLyrics()}
                  onError={onAudioError}
                />
              </div>
            )}

            {lyrics.map((line, idx) => {
              const active = idx === currentLine;
              const looped =
                playback.loopRegion !== null &&
                line.timeSec !== null &&
                Math.abs(playback.loopRegion.startSec - line.timeSec) < 0.01;
              return (
                <p
                  className={`line ${active ? "current" : ""} ${looped ? "looped" : ""}`}
                  key={line.utteranceId}
                  data-utt={line.utteranceId}
                  ref={active ? currentLineRef : undefined}
                >
                  <button
                    className="time"
                    disabled={!audioReady || line.timeSec === null}
                    onClick={() => line.timeSec !== null && void api.audioSeek(line.timeSec)}
                    title={line.timeSec === null ? "这行没有时间轴" : "跳到这一句"}
                  >
                    {line.timeSec === null ? "—" : formatDuration(line.timeSec)}
                  </button>
                  <button
                    className="loop-line"
                    disabled={!audioReady || line.timeSec === null}
                    onClick={async () => {
                      if (looped) {
                        await api.audioSetLoop();
                      } else if (!(await loopLine(idx))) {
                        onAudioError("这一句太短，无法循环");
                      }
                    }}
                    title={looped ? "取消单句循环" : "循环这一句"}
                  >
                    🔁
                  </button>
                  {renderLineText(idx)}
                </p>
              );
            })}
          </>
        )}
      </main>

      <ColumnSplitter
        edge="right"
        label="查词栏宽度"
        onDrag={columns.drag}
        onNudge={columns.nudge}
        onReset={columns.reset}
      />

      <aside className="pane corpus">
        {corpusContent}
      </aside>

      {stageOpen && selected && (
        <LyricsStage
          title={selected.title}
          meta={[selected.artist, selected.album, selected.year].filter(Boolean).join(" · ")}
          coverPath={selected.coverPath}
          lines={lyrics}
          currentLine={currentLine}
          renderText={renderLineText}
          canSeek={audioReady}
          onSeekLine={(idx) => {
            const time = lyrics[idx]?.timeSec;
            if (audioReady && time !== null && time !== undefined) void api.audioSeek(time);
          }}
          display={display}
          withFurigana={furigana !== null}
          style={lyricsStyle}
          side={stageSide && (word !== null || wordLoading || lookup !== null) ? corpusContent : null}
          onCloseSide={() => {
            // 关掉查词栏时把歌词上那个记号也清掉——面板都关了，记号留着没有意义
            setStageSide(false);
            setLookup(null);
            actions.clearWord();
          }}
          onError={onAudioError}
          onClose={closeStage}
        />
      )}
    </div>
  );
}
