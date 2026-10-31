/**
 * 「词的去处」：一个词出现在哪些歌里，按歌分组；点一首就从这个词开始放。
 *
 * 原来是词云右边那一栏（WordCloud 里的 SongPanel）。抽出来之后，分析、词典、检索也用同一份：
 * 任何一个语料里的词都能直接查到出处、听到它、再去看释义和制卡，而且人留在原来的页面上。
 *
 * `docked` 是放在普通页面右栏里的样子（跟着主题走的实色卡片）；不传就是词云上的毛玻璃浮层。
 */

import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";

import { api, type PersonSummary } from "../api";
import { pyGrouped } from "../pyFormat";
import { updateSettings, useAppSettings } from "../settings";
import { keywordSpan, REACTION_SEC, wordStart, type WordStart, type WordTiming } from "../wordTiming";
import { mergeSubcorpus, useActiveScope } from "../subcorpusScope";
import { groupHitsBySong, type CloudWord, type SongHits, type WordJump } from "../wordCloud";
import { Avatar } from "./Avatar";
import { Cover } from "./Cover";
import { loadPerformers } from "./PerformerPicker";
import { setFocusWord } from "../focusWord";
import { tr } from "../i18n";

/** 弹出的歌曲列表：KWIC 最多取这么多行再按歌分组 */
const KWIC_LIMIT = 3000;

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

/** 只知道词元时拼一个最小的 CloudWord（分析表、词典、检索都只有词元） */
export function wordOf(lemma: string, pos: string | null = null): CloudWord {
  return { lemma, variants: [lemma], freq: 0, songCount: 0, pos, mainPos: pos ?? "", surfaces: [] };
}

/** 面板要往外接的几条线：放哪一句、封面、去检索、去例句和释义。App 准备好一份，各页面原样传进来 */
export interface WordPanelLinks {
  playing: { songId: string; utteranceId: number } | null;
  coverOf: (songId: string) => string;
  onPlayLine: (songId: string, utteranceId: number, timeSec: number | null, fullscreen: boolean) => void;
  onSearchWord: (lemma: string, jump: WordJump) => void;
  onShowExamples: (lemma: string, jump: WordJump) => void;
  onError: (message: string) => void;
}

/** 页面内容 + 右边停靠的「词的去处」。没选词时只有内容 */
export function WithWordPanel({ word, onClose, links, children }: { word: CloudWord | null; onClose: () => void; links: WordPanelLinks; children: ReactNode }) {
  return (
    <div className={`with-word-panel${word !== null ? " open" : ""}`}>
      {children}
      {word !== null && <WordWhere key={word.variants.join("|")} docked word={word} onClose={onClose} {...links} />}
    </div>
  );
}

const NO_IDS: number[] = [];
const NO_YEARS = { from: null, to: null };

/* ── 选中一个词：一栏列出出现它的歌。点一首就从那一句放，不离开当前页面 ── */
export function WordWhere({
  word,
  highlight,
  stack,
  performerIds,
  years,
  people: peopleProp,
  playing,
  compact,
  covered,
  coverOf,
  onClose,
  onPlayLine,
  onSearchWord,
  onShowExamples,
  onError,
  docked = false,
}: {
  word: CloudWord;
  highlight?: string;
  stack?: string;
  performerIds?: number[];
  years?: { from: number | null; to: number | null };
  people?: PersonSummary[];
  playing: { songId: string; utteranceId: number } | null;
  compact?: boolean;
  covered?: boolean;
  coverOf: (songId: string) => string;
  onClose: () => void;
  onPlayLine: (songId: string, utteranceId: number, timeSec: number | null, fullscreen: boolean) => void;
  onSearchWord: (lemma: string, jump: WordJump) => void;
  onShowExamples: (lemma: string, jump: WordJump) => void;
  onError: (message: string) => void;
  /** 放在普通页面的右栏里（实色卡片、跟主题走），不是浮在词云上 */
  docked?: boolean;
}) {
  const settings = useAppSettings();
  performerIds = performerIds ?? NO_IDS;
  years = years ?? NO_YEARS;
  compact = compact ?? false;
  covered = covered ?? false;
  highlight = highlight ?? "var(--accent)";
  stack = stack ?? "inherit";
  const [loadedPeople, setLoadedPeople] = useState<PersonSummary[]>([]);
  useEffect(() => {
    if (peopleProp !== undefined) return;
    let alive = true;
    void loadPerformers().then((list) => { if (alive) setLoadedPeople(list); }).catch(() => undefined);
    return () => { alive = false; };
  }, [peopleProp]);
  const people = peopleProp ?? loadedPeople;
  const [songs, setSongs] = useState<SongHits[] | null>(null);
  const [truncated, setTruncated] = useState(false);
  // 词云的词频按当前子语料算，点词列出来的歌也得是这块语料里的
  const scope = useActiveScope();
  const jump = useMemo<WordJump>(() => {
    const names = performerIds.map((id) => people.find((p) => p.id === id)?.name).filter(Boolean);
    const span = years.from !== null || years.to !== null ? `${years.from ?? ""}–${years.to ?? ""}` : "";
    return {
      performerIds,
      yearFrom: years.from,
      yearTo: years.to,
      pos: word.pos ?? "",
      variants: word.variants,
      label: [names.join("、"), span].filter(Boolean).join(" · "),
    };
  }, [performerIds, people, years.from, years.to, word]);

  // 底部「正在唱」字幕把这个词标亮
  useEffect(() => setFocusWord(word.variants.length > 0 ? word.variants : [word.lemma]), [word]);

  useEffect(() => {
    let alive = true;
    api
      .kwic({
        // 几种写法合并成一个词的，每种写法都查（关键词之间是「或」）
        keywords: word.variants.length > 0 ? word.variants : [word.lemma],
        field: "lemma",
        pos: word.pos,
        personIds: performerIds,
        limit: KWIC_LIMIT,
        subcorpus: mergeSubcorpus({ yearFrom: years.from, yearTo: years.to }, scope),
      })
      .then((hits) => {
        if (!alive) return;
        setSongs(groupHitsBySong(hits));
        setTruncated(hits.length >= KWIC_LIMIT);
      })
      .catch((e) => {
        if (!alive) return;
        onError(message(e));
        setSongs([]);
      });
    return () => {
      alive = false;
    };
  }, [word, performerIds, years.from, years.to, scope, onError]);

  // 每首歌显示的那一处：真实单位 / 校准起点，没有就从行首播放。
  const [timings, setTimings] = useState<WordTiming[]>([]);
  const loadTimings = useCallback(() => {
    if (songs === null || songs.length === 0) return;
    api
      .wordTimings(songs.map((s) => s.first.utteranceId))
      .then(setTimings)
      .catch(() => setTimings([]));
  }, [songs]);
  useEffect(loadTimings, [loadTimings]);

  /** 正在校准哪一首：从这行开头放，听到这个词时按一下 */
  const [calibrating, setCalibrating] = useState<string | null>(null);
  const startOf = (s: SongHits): WordStart | null => wordStart(s.first, timings);
  const playFromWord = (s: SongHits, fullscreen: boolean) => {
    const at = startOf(s);
    onPlayLine(s.songId, s.first.utteranceId, at?.sec ?? null, fullscreen);
  };
  const beginCalibrate = (s: SongHits) => {
    setCalibrating(s.songId);
    // 从这一行开头前一点放，留出反应的时间
    onPlayLine(s.songId, s.first.utteranceId, Math.max(0, (s.first.timeSec ?? 0) - 0.5), false);
  };
  const markHeard = async (s: SongHits) => {
    try {
      const state = await api.audioState();
      if (state.songId !== s.songId) {
        onError(tr("现在放的不是这首，先点「校准」从这一行放起来"));
        return;
      }
      // 暂停时的进度不是「听到这个词」的那一刻
      if (state.playState !== "playing") {
        onError(tr("先从这一行放起来，听到这个词时再点"));
        return;
      }
      const span = keywordSpan(s.first);
      const at = Math.max(0, state.positionSec - REACTION_SEC);
      await api.wordTimeSet(s.first.utteranceId, span.start, span.end, at);
      const fresh = await api.wordTimings(songs?.map((x) => x.first.utteranceId) ?? []);
      setTimings(fresh);
      // 马上从校准的位置再放一遍，听听准不准
      onPlayLine(s.songId, s.first.utteranceId, Math.max(0, at - 0.05), false);
    } catch (e) {
      onError(message(e));
    }
  };
  const nudge = async (s: SongHits, delta: number) => {
    const manual = timings.find((t) => t.utteranceId === s.first.utteranceId && t.source === "manual" && t.charStart === keywordSpan(s.first).start);
    if (!manual) return;
    try {
      const at = Math.max(0, manual.startSec + delta);
      await api.wordTimeSet(manual.utteranceId, manual.charStart, manual.charEnd, at);
      setTimings(await api.wordTimings(songs?.map((x) => x.first.utteranceId) ?? []));
      onPlayLine(s.songId, s.first.utteranceId, Math.max(0, at - 0.05), false);
    } catch (e) {
      onError(message(e));
    }
  };
  const clearCalibration = async (s: SongHits) => {
    try {
      await api.wordTimeClear(s.first.utteranceId, keywordSpan(s.first).start);
      setTimings(await api.wordTimings(songs?.map((x) => x.first.utteranceId) ?? []));
    } catch (e) {
      onError(message(e));
    }
  };

  // Esc 关掉这一栏（全屏时第二下 Esc 才退出全屏）。盖着全屏歌词时不管
  useEffect(() => {
    if (covered) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopImmediatePropagation();
        onClose();
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [onClose, covered]);

  const photos = useMemo(() => new Map(people.map((p) => [p.name, p.imagePath])), [people]);
  // 合作署名「A/B」退到第一位演唱者的照片
  const photoOf = (artist: string) => photos.get(artist) ?? photos.get(artist.split("/")[0]?.trim() ?? "") ?? "";
  const byArtist = useMemo(() => {
    const counts = new Map<string, number>();
    for (const s of songs ?? []) counts.set(s.artist, (counts.get(s.artist) ?? 0) + s.count);
    return [...counts].sort((a, b) => b[1] - a[1]).slice(0, 8);
  }, [songs]);
  const total = songs?.reduce((n, s) => n + s.count, 0) ?? 0;

  return (
    <aside className={`wc-panel${compact ? " compact" : ""}${docked ? " docked" : ""}`} aria-label={tr("「{lemma}」出现的歌", { lemma: word.lemma })} style={{ ["--hl" as string]: highlight }}>
      <div className="wc-pop-head">
        <span className="wc-pop-ghost" style={{ fontFamily: stack }} aria-hidden="true">
          {word.lemma}
        </span>
        <b style={{ fontFamily: stack }} lang="ja">
          {word.lemma}
        </b>
        <span>
          {songs === null
            ? tr("正在查…")
            : tr("出现 {total} 次 · {length} 首歌{v}", { total: pyGrouped(total), length: songs.length, v: truncated ? tr("（只查了前 {KWIC_LIMIT} 行）", { KWIC_LIMIT: pyGrouped(KWIC_LIMIT) }) : "" })}
        </span>
        {/* 只在词云里有意义：别的页面上没有「云」可以藏 */}
        {!docked && (
        <button
          className="wc-pop-hide"
          onClick={() => {
            const current = settings.wordCloudHidden;
            const add = word.variants.filter((v) => !current.includes(v));
            if (add.length > 0) updateSettings({ wordCloudHidden: [...current, ...add] });
            onClose();
          }}
          title={tr("以后词云里不再出现这个词。在「设置 → 词云」里能恢复")}
        >{tr("不在词云里显示")}</button>
        )}
        <button className="wc-pop-close" onClick={onClose} aria-label={tr("关闭（Esc）")}>
          ✕
        </button>
      </div>
      {byArtist.length > 1 && (
        <div className="wc-pop-by">
          {byArtist.map(([artist, n]) => (
            <span key={artist}>
              <Avatar path={photoOf(artist)} name={artist} size={18} />
              {artist} {pyGrouped(n)}
            </span>
          ))}
        </div>
      )}
      <div className="wc-pop-list">
        {songs !== null && songs.length === 0 && <p className="muted small wc-pop-none">{tr("没查到例句")}</p>}
        {songs?.map((s, i) => {
          const now = playing?.songId === s.songId;
          const at = startOf(s);
          const cal = calibrating === s.songId;
          return (
            <div
              key={s.songId}
              className={`wc-song${now ? " now" : ""}${cal ? " calibrating" : ""}`}
              style={{ animationDelay: `${60 + Math.min(i, 12) * 35}ms` }}
              onContextMenu={(e) => {
                // 校准不占位置：在这一行上右键打开（再右键收起）
                if (at === null) return;
                e.preventDefault();
                if (cal) setCalibrating(null);
                else beginCalibrate(s);
              }}
            >
              {/* 只有点封面才进全屏歌词 */}
              <button
                className="wc-song-face"
                onClick={() => playFromWord(s, true)}
                title={tr("从真实单位起点播放；没有词级时间则从行首播放，并进入全屏歌词")}
              >
                {coverOf(s.songId) ? <Cover path={coverOf(s.songId)} size={40} rounded={6} /> : <Avatar path={photoOf(s.artist)} name={s.artist} size={40} />}
                <span className="wc-song-icon" aria-hidden="true">
                  ⤢
                </span>
              </button>
              <button className="wc-song-meta" onClick={() => playFromWord(s, false)} title={tr("从真实单位起点播放；没有词级时间则从行首播放")}>
                <strong>{s.title}</strong>
                <small>{tr("{artist} · {count} 次", { artist: s.artist, count: s.count })}{at !== null ? ` · ${formatTime(at.sec)}` : ""}
                  {/* 只标你校准过的：逐字歌词的时间用户说不用标，每行挂一个「逐字」反而乱 */}
                  {at !== null && at.kind === "manual" && (
                    <span className="wc-timing manual" title={tr("从你校准过的位置放。右键这一行可以重新校准")}>
                      {tr("已校准")}
                    </span>
                  )}
                </small>
                <em lang="ja" style={{ fontFamily: stack }}>
                  {s.atStart && (
                    <span className="wc-start" title={tr("这一行就从这个词开始：放出来第一个听到的就是它")}>{tr("句首")}</span>
                  )}
                  {s.first.left}
                  <mark>{s.first.keyword}</mark>
                  {s.first.right}
                </em>
              </button>
              <button
                className="wc-song-go"
                onClick={() => playFromWord(s, false)}
                title={tr("从真实单位起点播放；没有词级时间则从行首播放")}
                aria-label={tr("播放 {title}", { title: s.title })}
              >
                {now ? (
                  <i className="wc-eq" aria-hidden="true">
                    <b />
                    <b />
                    <b />
                  </i>
                ) : (
                  <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor" aria-hidden="true">
                    <path d="M8 5.5v13a1 1 0 0 0 1.5.86l10.6-6.5a1 1 0 0 0 0-1.72L9.5 4.64A1 1 0 0 0 8 5.5z" />
                  </svg>
                )}
              </button>
              {cal && (
                <div className="wc-calib">
                  <button className="wc-calib-hit" onClick={() => void markHeard(s)}>{tr("听到「{keyword}」时点这里", { keyword: s.first.keyword })}</button>
                  <button className="wc-calib-done" onClick={() => setCalibrating(null)} title={tr("收起（在这一行上再右键也行）")}>{tr("完成")}</button>
                  {at?.kind === "manual" && (
                    <span className="wc-calib-tools">
                      <button onClick={() => void nudge(s, -0.1)} title={tr("早 0.1 秒")}>
                        −0.1
                      </button>
                      <button onClick={() => void nudge(s, 0.1)} title={tr("晚 0.1 秒")}>
                        +0.1
                      </button>
                      <button onClick={() => playFromWord(s, false)}>{tr("重听")}</button>
                      <button onClick={() => void clearCalibration(s)} title={tr("删掉这处校准，回到来源单位起点或行首")}>{tr("撤销")}</button>
                    </span>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>
      {!compact && (
        <div className="wc-pop-foot">
          <button onClick={() => onSearchWord(word.lemma, jump)}>{tr("在检索页看 KWIC")}</button>
          <button onClick={() => onShowExamples(word.lemma, jump)}>{tr("看例句和释义")}</button>
        </div>
      )}
    </aside>
  );
}

function formatTime(sec: number): string {
  const s = Math.max(0, Math.floor(sec));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

