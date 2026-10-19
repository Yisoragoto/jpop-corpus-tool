/**
 * 资料库：原来的「曲库」和「人物」合成一页。Library → 人 / 专辑 / 歌 → Lyrics → 点词 → Corpus。
 *
 * 三栏。左栏四种找法（音乐人、幕后、专辑、歌曲，见 `LibraryBrowser`）。
 * 中栏看的是一个人的档案、一张专辑，或者一首歌的歌词——歌词跟随播放，实词可点；
 * 右栏跟着中栏变：看人时是合作者，看歌时是该词在全语料里的样子，
 * 点例句可以跳到另一首歌——这就是要求书第十八条那条回路。
 */

import { Fragment, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";

import {
  api,
  CONTENT_POS,
  formatDuration,
  POS_LABELS,
  ROLE_LABELS,
  type Album,
  type PlaybackState,
  type Track,
} from "../api";
import { Stat } from "../components/Stat";
import { useArtistPhotos } from "../components/PerformerPicker";
import { LibraryBrowser, useDirectory } from "../components/LibraryBrowser";
import { AlbumView, PersonProfile, PersonSide } from "../components/PersonProfile";
import { GraphView } from "../components/GraphView";
import { TimelineView } from "../components/TimelineView";
import { CloudView, focusCloudOn } from "../components/CloudView";
import { Cover } from "../components/Cover";
import { Avatar } from "../components/Avatar";
import { AudioRepair } from "../components/AudioRepair";
import { LyricsStage } from "../components/LyricsStage";
import { SongEditor } from "../components/SongEditor";
import { LyricsEditor } from "../components/LyricsEditor";
import { SongLineagePanel } from "../components/SongLineagePanel";
import { SubcorpusPicker } from "../components/SubcorpusPicker";
import { LyricsTools } from "../components/LyricsTools";
import { LyricsFill } from "../components/LyricsFill";
import { lyricFontStack, useLyricFontFiles } from "../fonts";
import { retryFurigana, useLyricsDisplay, useSongFurigana } from "../lyricsDisplay";
import { csvProblem, describeDelete, describeEdit, libraryApi, type MissingAudio } from "../libraryAdmin";
import { segmentLine, type Piece } from "../lyricSegments";
import { albumOwner, sameView, type LibraryView } from "../libraryIndex";
import { morph } from "../motion";
import { holdFullscreen, releaseFullscreen } from "../fullscreen";
import type { LibraryActions, LibraryState } from "../useLibrary";
import { useCurrentLine, useLineLoop } from "../usePlayback";
import { useAppSettings } from "../settings";
import { useKaraoke } from "../useKaraoke";
import { charTimeline, type WordTiming } from "../wordTiming";
import { audition } from "../listen";
import { SongProfile } from "../components/SongProfile";
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

/**
 * 这首歌的词级时间（逐字歌词 + 手动校准），给逐字点亮用。关着时不取；换歌（歌词重读）时重取。
 * 取不到（旧库没有这张表之类）就当没有，全部按字数估。
 */
function useSongWordTimings(lyrics: { utteranceId: number }[], enabled: boolean): WordTiming[] {
  const [state, setState] = useState<{ lyrics: unknown; list: WordTiming[] }>({ lyrics: null, list: [] });
  useEffect(() => {
    if (!enabled || lyrics.length === 0) return;
    let alive = true;
    api.wordTimings(lyrics.map((l) => l.utteranceId)).then(
      (list) => alive && setState({ lyrics, list }),
      () => alive && setState({ lyrics, list: [] }),
    );
    return () => {
      alive = false;
    };
  }, [lyrics, enabled]);
  return enabled && state.lyrics === lyrics ? state.list : NO_TIMINGS;
}

const NO_TIMINGS: WordTiming[] = [];

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

interface Props {
  library: LibraryState;
  actions: LibraryActions;
  playback: PlaybackState;
  audioReady: boolean;
  onAudioError: (message: string) => void;
  /** 删歌、改歌之后总览的数字要重拉 */
  onLibraryChanged: () => void;
  /** 导入、刮削之后加一：音乐人和专辑列表要重拉 */
  libraryVersion: number;
  /** 从命令面板跳过来时要看的人。消费掉就清空 */
  focusPersonId: number | null;
  onFocusConsumed: () => void;
  /** 词云里点「在检索页看」 */
  onSearchWord: (lemma: string) => void;
  /** 外面（分析页）请求打开词云 */
  cloudWanted?: boolean;
  onCloudConsumed?: () => void;
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
  libraryVersion,
  focusPersonId,
  onFocusConsumed,
  onSearchWord,
  cloudWanted = false,
  onCloudConsumed,
  stageWanted = false,
  onStageConsumed,
}: Props) {
  const { tracks, selected, lyrics, credits, lyricsLoading, word, wordLoading, activeLemma } =
    library;
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

  // 两边的宽度可以拖，并且记住
  // 大屏（27 寸、2560 宽）第一次打开时两边给宽一点；拖过的照记住的来
  const columns = useColumnWidths(
    "library",
    window.innerWidth >= 1900 ? { left: 320, right: 400 } : { left: 260, right: 320 },
  );
  const artistPhotos = useArtistPhotos();
  // 左栏可以收起来，给中栏（词云、时间轴、歌词）多留地方。记住
  const [browserHidden, setBrowserHidden] = useState(() => {
    try {
      return localStorage.getItem("jp.library.browserHidden") === "1";
    } catch {
      return false;
    }
  });
  const toggleBrowser = useCallback(() => {
    setBrowserHidden((v) => {
      try {
        localStorage.setItem("jp.library.browserHidden", v ? "0" : "1");
      } catch {
        // 只在这次会话里生效
      }
      return !v;
    });
  }, []);
  // 封面按歌查：词云里列出的歌要显示它自己的封面
  const coverOf = useCallback((songId: string) => tracks.find((t) => t.id === songId)?.coverPath ?? "", [tracks]);

  // 曲库维护：编辑、删除（两步确认，不弹窗）、找回丢了的音频
  const [panel, setPanel] = useState<null | "edit" | "repair" | "lineage" | "lyrics">(null);
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
  const loopLine = useLineLoop(lyrics);

  // 逐字点亮（设置「歌词」页可关）：只算当前这一行每个字的时间，动画在 useKaraoke 里直接改 DOM
  const karaokeEnabled = useAppSettings().karaokeLyrics;
  const wordTimings = useSongWordTimings(lyrics, karaokeEnabled);
  const karaoke = useMemo(() => {
    if (!karaokeEnabled || !lyricsArePlaying) return null;
    const line = lyrics[currentLine];
    if (line === undefined || line.timeSec === null) return null;
    const next = lyrics.slice(currentLine + 1).find((l) => l.timeSec !== null)?.timeSec ?? null;
    const mine = wordTimings.filter((t) => t.utteranceId === line.utteranceId);
    return charTimeline(line.text, line.timeSec, next, mine);
  }, [karaokeEnabled, lyricsArePlaying, lyrics, currentLine, wordTimings]);
  useKaraoke(
    karaoke,
    playback.positionSec,
    playback.playState === "playing",
    playback.rate,
    `${stageOpen}|${furigana !== null}|${display.furiganaMode}`,
  );

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
    // 逐字点亮的那一行：每个字包一层 span（`data-c` 是字符下标），按渲染顺序数下去
    const karaokeLine = karaoke !== null && idx === currentLine;
    let c = 0;
    const chars = (text: string): ReactNode =>
      karaokeLine
        ? [...text].map((ch) => {
            const at = c++;
            return (
              <span key={at} className="kc" data-c={at}>
                {ch}
              </span>
            );
          })
        : text;
    const pieces = (list: Piece[]): ReactNode =>
      karaokeLine
        ? list.map((piece, k) =>
            piece.reading === undefined ? (
              <Fragment key={k}>{chars(piece.text)}</Fragment>
            ) : (
              <ruby key={k}>
                {chars(piece.text)}
                <rt>{piece.reading}</rt>
              </ruby>
            ),
          )
        : renderPieces(list);
    return (
      <span className={karaokeLine ? "text karaoke-line" : "text"} lang="ja">
        {line.tokens.length === 0
          ? chars(line.text)
          : (lineSegments[idx] ?? line.tokens.map((_, index) => ({ kind: "token" as const, index, pieces: [] }))).map((segment, k) => {
              if (segment.kind === "gap") return <Fragment key={`gap-${k}`}>{chars(segment.text)}</Fragment>;
              const i = segment.index;
              const token = line.tokens[i];
              if (token === undefined) return null;
              // 分词对不齐原文时（拆分结果是 null）退回只显示词
              const content = segment.pieces.length > 0 ? pieces(segment.pieces) : chars(token.surface);
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

  // ── 中栏在看什么 ──
  //
  // 来的路记在一个栈里，左上角的返回按钮一层层退回去。歌本身在 `selected` 里（外壳持有，换页不丢）。
  // 改了成员之后自己加一，和外面的版本号加在一起：两边都只增不减，和一变就重拉
  const [membersVersion, setMembersVersion] = useState(0);
  const bumpMembers = useCallback(() => setMembersVersion((v) => v + 1), []);
  const data = useDirectory(libraryVersion + membersVersion, onAudioError);
  const [view, setView] = useState<LibraryView | null>(() =>
    focusPersonId !== null ? { kind: "person", id: focusPersonId } : selected !== null ? { kind: "song" } : null,
  );
  const trail = useRef<LibraryView[]>([]);
  const [canBack, setCanBack] = useState(false);
  const viewRef = useRef(view);
  viewRef.current = view;
  const go = useCallback((next: LibraryView) => {
    const current = viewRef.current;
    // 同一个人、只换了要定位的专辑：不进栈
    if (current !== null && !sameView(current, next)) {
      trail.current = [...trail.current.slice(-30), current];
      setCanBack(true);
    }
    viewRef.current = next;
    setView(next);
  }, []);
  const back = useCallback(() => {
    const previous = trail.current.pop();
    setCanBack(trail.current.length > 0);
    if (previous !== undefined) setView(previous);
  }, []);

  // 什么都没看时（第一次进来、没选过歌），先打开排第一的音乐人——中栏空着像没加载出来
  useEffect(() => {
    if (view !== null || data === null) return;
    const first = data.dir.musicians[0];
    if (first !== undefined) setView({ kind: "person", id: first.entry.id });
  }, [view, data]);

  // 别处（命令面板、首页、检索）打开了一首歌：切到歌
  const lastSongId = useRef(selected?.id ?? null);
  useEffect(() => {
    const id = selected?.id ?? null;
    if (id === lastSongId.current) return;
    lastSongId.current = id;
    if (id !== null) go({ kind: "song" });
  }, [selected?.id, go]);

  // 命令面板点了一个人
  useEffect(() => {
    if (focusPersonId === null) return;
    go({ kind: "person", id: focusPersonId });
    onFocusConsumed();
  }, [focusPersonId, go, onFocusConsumed]);

  /** 专辑：对得上演唱者就去他的档案并定位到这一张，对不上就单独看这张专辑 */
  const openAlbum = useCallback(
    (album: Album) => {
      const owner = albumOwner(album.albumArtist);
      const who = data?.dir.musicians.find((m) => m.entry.name === owner);
      if (who !== undefined) go({ kind: "person", id: who.entry.id, albumId: album.id });
      else go({ kind: "album", albumId: album.id, title: album.title });
    },
    [data, go],
  );

  // 命令面板点了一张专辑（经由 useLibrary 的 albumFilter 传过来）
  useEffect(() => {
    const wanted = library.albumFilter;
    if (wanted === null || data === null) return;
    const album = data.albums.find((a) => a.id === wanted.albumId);
    if (album !== undefined) openAlbum(album);
    else go({ kind: "album", albumId: wanted.albumId, title: wanted.title });
    void actions.setAlbumFilter(null);
  }, [library.albumFilter, data, openAlbum, go, actions]);

  /** 打开一个人：头像从点的地方飞到档案头 */
  const openPerson = useCallback(
    (id: number, from?: HTMLElement) => {
      if (view?.kind === "person" && view.id === id) return;
      morph(() => go({ kind: "person", id }), {
        name: "person-photo",
        from: from?.querySelector(".avatar"),
        to: () => document.querySelector(".profile-photo .avatar"),
      });
    },
    [view, go],
  );

  /** 面包屑上的专辑：在目录里找到就按专辑的规矩走，找不到就单独看 */
  const openAlbumById = (albumId: number, title: string) => {
    const album = data?.albums.find((a) => a.id === albumId);
    if (album !== undefined) openAlbum(album);
    else go({ kind: "album", albumId, title });
  };
  const performers = credits.filter((c) => c.role === "performer");
  /** 右栏的署名按人合并：同一个人作曲又编曲只占一行 */
  const creditPeople = useMemo(() => {
    const byPerson = new Map<number, { personId: number; name: string; roles: string[] }>();
    for (const c of credits) {
      const row = byPerson.get(c.personId) ?? { personId: c.personId, name: c.name, roles: [] };
      if (!row.roles.includes(c.role)) row.roles.push(c.role);
      byPerson.set(c.personId, row);
    }
    return [...byPerson.values()];
  }, [credits]);

  // 歌名旁的「⋯」：编辑、换歌词、删除收在里面。点外面或按 Esc 关
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => setMenuOpen(false), [selected?.id]);
  useEffect(() => {
    if (!menuOpen) return;
    const away = (e: PointerEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenuOpen(false);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setMenuOpen(false);
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("pointerdown", away);
      document.removeEventListener("keydown", esc);
    };
  }, [menuOpen]);

  /** 打开一首歌：封面从点的地方飞到歌词栏的标题旁（共享元素过渡） */
  const openWithMorph = (track: Track, button: HTMLElement) => {
    if (selected?.id === track.id) {
      go({ kind: "song" });
      return;
    }
    morph(
      () => {
        go({ kind: "song" });
        lastSongId.current = track.id;
        void actions.openTrack(track);
      },
      {
        name: "track-cover",
        from: button.querySelector(".cover"),
        to: () => document.querySelector(".lyrics .track-head .cover"),
      },
    );
  };

  /**
   * 词云里点一首歌：从那一句开始放，人留在词云里（不切到歌词）。
   * `fullscreen` 时同时进全屏歌词（窗口也全屏）；退出全屏歌词，回到的还是词云。
   * 全屏歌词要等这首歌真的选上了再开，否则会先闪一下上一首。
   */
  const [stageFor, setStageFor] = useState<string | null>(null);
  const playLine = useCallback(
    (songId: string, utteranceId: number, timeSec: number | null, fullscreen: boolean) => {
      lastSongId.current = songId;
      if (fullscreen) setStageFor(songId);
      void actions.openTrackById(songId, { scrollTo: utteranceId, seekTo: timeSec, autoPlay: true });
    },
    [actions],
  );
  useEffect(() => {
    if (stageFor === null || selected?.id !== stageFor) return;
    setStageFor(null);
    holdFullscreen("stage");
    setStageOpen(true);
  }, [stageFor, selected?.id]);
  // 全屏歌词关了：放开窗口全屏（词云全屏、F11 还开着的话窗口照旧全屏）
  useEffect(() => {
    if (!stageOpen) releaseFullscreen("stage");
  }, [stageOpen]);
  useEffect(() => () => releaseFullscreen("stage"), []);

  // 分析页点「打开词云」
  useEffect(() => {
    if (!cloudWanted) return;
    go({ kind: "cloud" });
    onCloudConsumed?.();
  }, [cloudWanted, go, onCloudConsumed]);

  const playAll = useCallback(
    (list: Track[]) => {
      const first = list.find((t) => t.audioPath) ?? list[0];
      if (first === undefined) return;
      lastSongId.current = first.id;
      go({ kind: "song" });
      void actions.openTrack(first, { autoPlay: true });
    },
    [actions, go],
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
        <div key={ex.utteranceId} className="example-row">
          <button
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
          {ex.timeSec !== null && (
            <button
              className="kwic-play"
              onClick={() => void audition(actions, ex.utteranceId).catch((e) => onAudioError(String(e)))}
              title="试听这一句（到行尾自动停）"
              aria-label="试听这一句"
            >
              ▶
            </button>
          )}
        </div>
      ))}
      {word.examples.length === 0 && <p className="muted">没有例句</p>}
    </div>
  );

  const corpusContent = (
    <>
      {!word && !wordLoading && lookup === null && selected !== null && (
        <div className="song-facts">
          <h3 className="side-head">这首歌</h3>
          <dl className="facts">
            {selected.album && <><dt>专辑</dt><dd>{selected.album}</dd></>}
            {selected.year && <><dt>年份</dt><dd>{selected.year}</dd></>}
            {selected.genre && <><dt>流派</dt><dd>{selected.genre}</dd></>}
            {selected.durationSec !== null && <><dt>时长</dt><dd>{formatDuration(selected.durationSec)}</dd></>}
            <dt>歌词</dt><dd>{lyrics.length > 0 ? `${lyrics.length} 行` : "没有"}</dd>
          </dl>
          {credits.length > 0 && (
            <>
              <h3 className="side-head">署名</h3>
              {creditPeople.map((c, i) => (
                <button key={c.personId} className="peer" style={{ ["--n" as string]: i }} onClick={() => openPerson(c.personId)}>
                  <Avatar path={c.roles.includes("performer") ? photoOf(c.name) : ""} name={c.name} size={30} />
                  <span className="peer-text">
                    <b>{c.name}</b>
                    <small>{c.roles.map((r) => ROLE_LABELS[r] ?? r).join(" / ")}</small>
                  </span>
                </button>
              ))}
            </>
          )}
          <SongProfile songId={selected.id} onSearchWord={onSearchWord} />
          <p className="muted small hint">点歌词里的词，这里换成释义和它在整个语料里的例句</p>
        </div>
      )}
      {!word && !wordLoading && lookup === null && selected === null && (
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
          <SubcorpusPicker onError={onAudioError} onChange={() => void actions.openWord(word.lemma)} />
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

  const backButton = canBack && (
    <button className="btn nav-back" onClick={back} title="返回" aria-label="返回">
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="m14 7-5 5 5 5" />
      </svg>
    </button>
  );
  const songView = view?.kind === "song" || view === null;
  /** 关系网、时间轴占中栏加右栏 */
  const wide = view?.kind === "graph" || view?.kind === "timeline" || view?.kind === "cloud";

  return (
    <div
      className={`columns library-page ${browserHidden ? "browser-hidden" : ""}`}
      style={browserHidden ? { gridTemplateColumns: `0px 0px minmax(0, 1fr) 6px ${columns.widths.right}px` } : columns.style}
      ref={columns.container}
    >
      {browserHidden && (
        <button className="btn browser-reveal" onClick={toggleBrowser} title="展开左栏" aria-label="展开左栏">
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" aria-hidden="true">
            <rect x="3.5" y="4.5" width="17" height="15" rx="2" />
            <path d="M9 4.5v15M13 10l2 2-2 2" />
          </svg>
        </button>
      )}
      <LibraryBrowser
        data={data}
        tracks={tracks}
        view={view ?? { kind: "song" }}
        selectedSongId={selected?.id ?? null}
        missingIds={missingIds}
        onOpenPerson={openPerson}
        onOpenAlbum={openAlbum}
        onOpenTrack={openWithMorph}
        onOpenGraph={() => go({ kind: "graph", focus: view?.kind === "person" ? view.id : null })}
        onOpenTimeline={() => go({ kind: "timeline" })}
        onOpenCloud={() => go({ kind: "cloud" })}
        onHide={toggleBrowser}
        banner={
          missing.length > 0 && (
            <button className="missing-banner" onClick={() => { go({ kind: "song" }); setPanel("repair"); }} title="扫文件夹或逐首选文件找回">
              ⚠ {missing.length} 首找不到音频文件 · 找回
            </button>
          )
        }
      />

      <ColumnSplitter
        className="split-left"
        edge="left"
        label="资料库左栏宽度"
        onDrag={columns.drag}
        onNudge={columns.nudge}
        onReset={columns.reset}
      />

      {view?.kind === "graph" && (
        <main className="pane graph-pane">
          {backButton}
          <GraphView data={data} initialFocus={view.focus ?? null} onOpenPerson={(id) => go({ kind: "person", id })} />
        </main>
      )}

      {view?.kind === "cloud" && (
        <main className="pane graph-pane cloud-pane">
          {backButton}
          <CloudView
            playing={playback.songId !== null ? { songId: playback.songId, utteranceId: lyrics[currentLine]?.utteranceId ?? -1 } : null}
            nowLine={
              lyricsArePlaying && selected !== null && lyrics[currentLine] !== undefined
                ? { songId: selected.id, title: selected.title, artist: selected.artist, text: lyrics[currentLine]!.text }
                : null
            }
            onPlayLine={playLine}
            coverOf={coverOf}
            covered={stageOpen}
            onSearchWord={onSearchWord}
            onShowExamples={(lemma) => {
              go({ kind: "song" });
              void actions.openWord(lemma);
            }}
            onError={onAudioError}
          />
        </main>
      )}

      {view?.kind === "timeline" && (
        <main className="pane graph-pane timeline-pane">
          {backButton}
          <TimelineView
            data={data}
            version={libraryVersion}
            playingSongId={playback.songId}
            onOpenPerson={(id) => go({ kind: "person", id })}
            onOpenAlbum={(id, albumId) => go({ kind: "person", id, albumId })}
            onPlaySong={(songId) => {
              // 人留在时间树里：先记下这首，免得「换了歌」把中栏切到歌词
              lastSongId.current = songId;
              void actions.openTrackById(songId, { autoPlay: true });
            }}
            onError={onAudioError}
          />
        </main>
      )}

      {(view?.kind === "person" || view?.kind === "album") && (
        <main className="pane profile-pane" key={view.kind === "person" ? `p${view.id}` : `a${view.albumId}`}>
          {backButton}
          {view.kind === "person" ? (
            <PersonProfile
              personId={view.id}
              data={data}
              focusAlbumId={view.albumId ?? null}
              playingSongId={playback.songId}
              onOpenPerson={openPerson}
              onOpenTrack={openWithMorph}
              onPlayAll={playAll}
              onMembersChanged={bumpMembers}
              onOpenCloud={() => {
                focusCloudOn(view.id);
                go({ kind: "cloud" });
              }}
              onError={onAudioError}
            />
          ) : (
            <AlbumView
              album={data?.albums.find((a) => a.id === view.albumId) ?? null}
              title={view.title}
              playingSongId={playback.songId}
              onOpenTrack={openWithMorph}
              onPlayAll={playAll}
              onError={onAudioError}
            />
          )}
        </main>
      )}

      {/* 歌词栏一直挂着、看人时只是藏起来：回到这首歌时滚动位置还在 */}
      <main
        className={`pane lyrics ${furigana !== null ? "with-furigana" : ""} ${songView ? "" : "is-hidden"}`}
        ref={lyricsRef}
        style={lyricsStyle}
      >
        {backButton}
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
        {panel !== "repair" && !selected && <p className="muted pad">从左侧选一位音乐人、一张专辑或一首歌</p>}
        {panel !== "repair" && selected && (
          <>
            <nav className="crumbs" aria-label="这首歌在哪">
              {performers.length > 0
                ? performers.map((c, i) => (
                    <Fragment key={c.personId}>
                      {i > 0 && <span className="crumb-sep">/</span>}
                      <button onClick={() => openPerson(c.personId)}>{c.name}</button>
                    </Fragment>
                  ))
                : <span>{selected.artist || "未知歌手"}</span>}
              {selected.album && (
                <>
                  <span className="crumb-sep">›</span>
                  {selected.albumId !== null ? (
                    <button onClick={() => openAlbumById(selected.albumId!, selected.album)}>{selected.album}</button>
                  ) : (
                    <span>{selected.album}</span>
                  )}
                </>
              )}
              <span className="crumb-sep">›</span>
              <b>{selected.title}</b>
            </nav>
            <div className="track-head track-head-row">
              <Cover path={selected.coverPath} size={96} alt="" rounded={6} />
              <div className="track-head-text">
                <h1>{selected.title}</h1>
                <p className="muted">
                  {selected.artist}
                  {selected.album && ` · ${selected.album}`}
                  {selected.year && ` · ${selected.year}`}
                </p>
                {credits.length > 0 && (
                  <ul className="credits">
                    {credits.map((c) => (
                      <li key={`${c.role}-${c.personId}`}>
                        <button className="credit-link" onClick={() => openPerson(c.personId)} title={`看 ${c.name} 的档案`}>
                          <span className="role">{ROLE_LABELS[c.role] ?? c.role}</span>
                          {c.name}
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
              <div className="track-side">
                <div className="track-tools">
                  <LyricsTools display={display} onError={onAudioError} onExpand={() => setStageOpen(true)} />
                  <div className="more-menu" ref={menuRef}>
                    <button
                      className={`btn more-btn ${menuOpen ? "on" : ""}`}
                      onClick={() => setMenuOpen((v) => !v)}
                      title="更多"
                      aria-label="更多"
                      aria-expanded={menuOpen}
                    >
                      <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
                        <circle cx="6" cy="12" r="1.6" fill="currentColor" />
                        <circle cx="12" cy="12" r="1.6" fill="currentColor" />
                        <circle cx="18" cy="12" r="1.6" fill="currentColor" />
                      </svg>
                    </button>
                    {menuOpen && (
                      <div className="menu-panel" role="menu">
                        <button role="menuitem" onClick={() => { setMenuOpen(false); setPanel(panel === "edit" ? null : "edit"); }}>
                          编辑信息
                        </button>
                        <button role="menuitem" onClick={() => { setMenuOpen(false); setPanel(panel === "lineage" ? null : "lineage"); }}>
                          来源与校验
                        </button>
                        {lyrics.length > 0 && (
                          <button role="menuitem" onClick={() => { setMenuOpen(false); setPanel(panel === "lyrics" ? null : "lyrics"); }}>
                            逐行编辑歌词
                          </button>
                        )}
                        {lyrics.length > 0 && (
                          <LyricsFill songId={selected.id} hasLyrics onAttached={() => void actions.reloadLyrics()} onError={onAudioError} />
                        )}
                        <hr />
                        <button role="menuitem" className="danger" onClick={() => { setMenuOpen(false); setConfirmDelete(true); }}>
                          删除这首歌…
                        </button>
                      </div>
                    )}
                  </div>
                </div>
                {confirmDelete && (
                  <span className="confirm-delete">
                    删掉《{selected.title}》？歌词和分词一起删，音频文件不动
                    <button className="link-btn danger" onClick={() => void deleteSelected()}>
                      确认删除
                    </button>
                    <button className="link-btn" onClick={() => setConfirmDelete(false)}>
                      取消
                    </button>
                  </span>
                )}
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
            {panel === "lyrics" && (
              <LyricsEditor
                key={selected.id}
                songId={selected.id}
                lines={lyrics}
                onSaved={(text) => {
                  setPanel(null);
                  setMaintainNote({ forSong: selected.id, text });
                  void actions.reloadLyrics();
                }}
                onCancel={() => setPanel(null)}
                onError={onAudioError}
              />
            )}
            {panel === "lineage" && (
              <SongLineagePanel
                key={selected.id}
                songId={selected.id}
                onLyricsChanged={() => void actions.reloadLyrics()}
                onClose={() => setPanel(null)}
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
                  // 换歌时前十几行依次浮上来，再往后一起出（见 styles.css「动效」）
                  style={{ ["--n" as string]: Math.min(idx, 16) }}
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
        className={`split-right ${wide ? "is-hidden" : ""}`}
        edge="right"
        label="查词栏宽度"
        onDrag={columns.drag}
        onNudge={columns.nudge}
        onReset={columns.reset}
      />

      <aside className={`pane corpus ${wide ? "is-hidden" : ""}`}>
        {view?.kind === "person" ? (
          <PersonSide
            personId={view.id}
            data={data}
            onOpenPerson={openPerson}
            onOpenGraph={() => go({ kind: "graph", focus: view.id })}
            onError={onAudioError}
          />
        ) : (
          corpusContent
        )}
      </aside>

      {stageOpen && selected && (
        <LyricsStage
          title={selected.title}
          meta={[selected.artist, selected.album, selected.year].filter(Boolean).join(" · ")}
          coverPath={selected.coverPath}
          track={selected}
          credits={credits}
          photoOf={photoOf}
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
