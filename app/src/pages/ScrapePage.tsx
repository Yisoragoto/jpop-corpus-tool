import { Icon } from "../components/Icon";
/**
 * 刮削页 = 库的完整度。
 *
 * 用户关心的不是刮削器的状态机，而是「我的库还缺什么、点哪里补」。所以一种 metadata 一行：
 * 进度条 + 齐了几个 / 一共几个；齐了的打勾变灰，缺的那行右边只有一个写明数量的按钮（「补 1 张」）。
 * 点之前就知道会做什么。
 *
 * 其余的按重要性往下排：
 *
 * 1. **正在跑的作业**只在跑的时候出现一条，可以中断。批量刮削要十几分钟，不能只给一个转圈。
 * 2. **复核队列。** 置信度不够的停在这里等人看，而不是自信地猜错写进库。空的时候就一行字。
 * 3. 失败、跳过的数字可以点开看是哪几首。
 *
 * 操作的结果写在那一行右边，不弹横幅——横幅只留给真的出错。
 * 技术说明（MusicBrainz → Deezer 的顺序、用曲目封面填专辑封面）放进按钮的 tooltip。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../events";

import {
  api,
  ERROR_HINTS,
  formatDuration,
  SCRAPE_STATUS_LABELS,
  type ArtistRow,
  type Completeness,
  type CompletenessCount,
  type LyricsProgress,
  type LyricsUpgradeProgress,
  type ReviewItem,
  type ScrapeAttempt,
  type ScrapeCandidate,
  type ScrapeProgress,
  type ScrapeStatusName,
  type ScrapeSummaryCounts,
} from "../api";
import { Avatar } from "../components/Avatar";
import { CommandButton } from "../components/CommandButton";
import { LyricsUpgradePanel } from "../components/LyricsUpgradePanel";
import { TimedLyricsPanel } from "../components/TimedLyricsPanel";
import { tr } from "../i18n";

interface Props {
  onError: (message: string) => void;
  /** 刮削会改 songs 行和封面，完事要让外面刷新 */
  onChanged: () => void;
}

type RowKey = "identified" | "trackCovers" | "albumCovers" | "albumInfo" | "photos" | "lyrics" | "durations";

const errorText = (e: unknown) => String((e as { message?: string })?.message ?? e);

export function ScrapePage({ onError, onChanged }: Props) {
  const [summary, setSummary] = useState<ScrapeSummaryCounts | null>(null);
  const [complete, setComplete] = useState<Completeness | null>(null);
  const [artists, setArtists] = useState<ArtistRow[]>([]);
  const [review, setReview] = useState<ReviewItem[]>([]);
  /** 点开的「失败」或「已跳过」列表；null 是收着 */
  const [extra, setExtra] = useState<ScrapeStatusName | null>(null);
  const [extraItems, setExtraItems] = useState<ReviewItem[]>([]);
  const [progress, setProgress] = useState<ScrapeProgress | null>(null);
  const [lyricsJob, setLyricsJob] = useState<LyricsProgress | null>(null);
  const [running, setRunning] = useState(false);
  /** 正在跑的一次性操作（不是后台作业的那种），那一行的按钮显示「…中」 */
  const [busyRow, setBusyRow] = useState<RowKey | null>(null);
  /** 每一行最近一次操作的结果，写在那一行右边 */
  const [notes, setNotes] = useState<Partial<Record<RowKey | "review", string>>>({});
  const [openPath, setOpenPath] = useState<string | null>(null);
  const [history, setHistory] = useState<ScrapeAttempt[]>([]);
  const [menuOpen, setMenuOpen] = useState(false);
  const errorRef = useRef(onError);
  errorRef.current = onError;

  const note = (row: RowKey | "review", text: string) => setNotes((n) => ({ ...n, [row]: text }));

  const refresh = useCallback(async () => {
    try {
      const [counts, completeness, roster, queue] = await Promise.all([
        api.scrapeSummary(),
        api.libraryCompleteness(),
        api.artistRoster(),
        api.scrapeReviewQueue(["low_confidence"], 200),
      ]);
      setSummary(counts);
      setComplete(completeness);
      setArtists(roster);
      setReview(queue);
    } catch (e) {
      errorRef.current(errorText(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
    void api.scrapeIsRunning().then(setRunning).catch(() => undefined);

  }, [refresh]);

  // 点开的失败 / 跳过列表跟着刷新
  useEffect(() => {
    if (extra === null) return;
    void api
      .scrapeReviewQueue([extra], 200)
      .then(setExtraItems)
      .catch((e) => errorRef.current(errorText(e)));
  }, [extra, summary]);

  // 后台刮削（曲目或歌手）的进度
  useEffect(() => {
    const off = listen<ScrapeProgress>("scrape://progress", (event) => {
      setProgress(event.payload);
      if (event.payload.finished) {
        setRunning(false);
        const p = event.payload;
        // 整批断了要说出来：这是真错误，走横幅
        if (p.error) errorRef.current(tr("刮削中断：{error}", { error: p.error }));
        note(
          p.kind === "artist" ? "photos" : p.kind === "album" ? "albumInfo" : "identified",
          p.kind === "all"
            ? p.cancelled ? tr("一键刮削已中断") : tr("一键刮削跑完了")
            : tr("{v}：处理 {done}/{total}", { v: p.cancelled ? tr("已中断") : tr("跑完了"), done: p.done, total: p.total }),
        );
        void refresh();
        onChanged();
      }
    });
    return () => {
      void off.then((f) => f());
    };
  }, [refresh, onChanged]);

  useEffect(() => {
    let alive = true;
    const accept = (p: LyricsUpgradeProgress) => {
      if (!alive || p.total === 0) return;
      setLyricsJob({ ...EMPTY_LYRICS, done:p.done, total:p.total, title:p.title, filled:p.upgraded+p.lineFilled, notFound:p.notFound, failed:p.failed, finished:!p.running, cancelled:p.cancelled, error:p.error });
      if (!p.running) { void refresh(); onChanged(); }
    };
    void api.lyricsUpgradeStatus().then(accept).catch(() => undefined);
    const off = listen<LyricsUpgradeProgress>("lyrics-upgrade://progress", event => accept(event.payload));
    return () => { alive = false; void off.then(f => f()); };
  }, [refresh, onChanged]);
  // 补歌词的进度
  useEffect(() => {
    const off = listen<LyricsProgress>("lyrics://progress", (event) => {
      setLyricsJob(event.payload);
      const p = event.payload;
      if (p.finished) {
        if (p.error !== "") errorRef.current(tr("补齐歌词中断：{error}", { error: p.error }));
        note(
          "lyrics",
          tr("{v}：补上 {filled}，挑不出来 {notFound}", { v: p.cancelled ? tr("已停下") : tr("补完了"), filled: p.filled, notFound: p.notFound }) +
            (p.failed > 0 ? tr("，失败 {failed}", { failed: p.failed }) : ""),
        );
        void refresh();
        onChanged();
      }
    });
    return () => {
      void off.then((f) => f());
    };
  }, [refresh, onChanged]);

  const startJob = useCallback(
    async (kind: "track" | "artist" | "album", row: RowKey, begin: () => Promise<number>, nothing: string) => {
      setMenuOpen(false);
      try {
        const total = await begin();
        if (total === 0) {
          note(row, nothing);
          return;
        }
        setRunning(true);
        setProgress({ ...EMPTY_PROGRESS, kind, total });
      } catch (e) {
        errorRef.current(errorText(e));
      }
    },
    [],
  );

  /** 一键刮削：一个后台作业按顺序跑完四步，进度和单项作业走同一个事件 */
  const startAll = useCallback(async () => {
    setMenuOpen(false);
    try {
      await api.scrapeAllStart();
      setRunning(true);
      setProgress({ ...EMPTY_PROGRESS, kind: "all", step: 1, steps: 4 });
    } catch (e) {
      errorRef.current(errorText(e));
    }
  }, []);

  /** 一次性的操作：跑完写一句结果到那一行，然后刷新计数 */
  const runOnce = useCallback(
    async (row: RowKey, work: () => Promise<string>) => {
      setBusyRow(row);
      try {
        note(row, await work());
        await refresh();
        onChanged();
      } catch (e) {
        errorRef.current(errorText(e));
      } finally {
        setBusyRow(null);
      }
    },
    [refresh, onChanged],
  );

  const accept = useCallback(
    async (item: ReviewItem, index: number) => {
      try {
        const applied = await api.scrapeAccept(item.filePath, index);
        // 说清楚实际写了什么——要让用户看得见它确实做了事
        const parts = [
          applied.fieldsFilled > 0 ? tr("补了 {fieldsFilled} 个字段", { fieldsFilled: applied.fieldsFilled }) : "",
          applied.creditsProtected > 0 ? tr("{creditsProtected} 条信用标为人工", { creditsProtected: applied.creditsProtected }) : "",
          applied.creditsAdded > 0 ? tr("新增 {creditsAdded} 条信用", { creditsAdded: applied.creditsAdded }) : "",
          applied.coverPath !== null ? tr("封面已下载") : "",
        ].filter((p) => p !== "");
        note("review", `${item.localTitle || item.filePath}${tr("：")}${parts.length > 0 ? parts.join(tr("，")) : tr("已确认，没有需要补写的")}`);
        await refresh();
        onChanged();
      } catch (e) {
        errorRef.current(errorText(e));
      }
    },
    [refresh, onChanged],
  );

  const skip = useCallback(
    async (item: ReviewItem) => {
      try {
        await api.scrapeSkip(item.filePath, tr("复核时跳过"));
        note("review", tr("{v}：已跳过", { v: item.localTitle || item.filePath }));
        await refresh();
      } catch (e) {
        errorRef.current(errorText(e));
      }
    },
    [refresh],
  );

  const showHistory = useCallback(
    async (item: ReviewItem) => {
      if (openPath === item.filePath) {
        setOpenPath(null);
        return;
      }
      setOpenPath(item.filePath);
      setHistory(await api.scrapeAttempts(item.filePath, 20).catch(() => []));
    },
    [openPath],
  );

  // 菜单：点到外面就收起
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!menuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenuOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [menuOpen]);

  if (summary === null || complete === null) return <div className="page muted pad">{tr("加载中…")}</div>;

  const missingArtists = artists.filter((a) => !a.hasImage);
  const photos: CompletenessCount = { done: artists.length - missingArtists.length, total: artists.length };
  const lyricsRunning = lyricsJob !== null && !lyricsJob.finished;
  const blocked = running || busyRow !== null;
  const gap = (c: CompletenessCount) => c.total - c.done;

  const jobLine =
    running && progress !== null
      ? {
          label: progress.steps > 0 ? scrapeStepLabel(progress) : tr("正在{label}", { label: scrapeStepLabel(progress) }),
          done: progress.done,
          total: progress.total,
          // 补封面那一步的标题是 Rust 拼的一句中文汇总，界面上不另外显示
          detail: progress.kind === "cover" ? "" : `${progress.title}${progress.matched ? ` → ${progress.matched}` : ""}`,
          stop: () => void api.scrapeCancel(),
        }
      : lyricsRunning && lyricsJob !== null
        ? {
            label: tr("正在{label}", { label: tr("补歌词") }),
            done: lyricsJob.done,
            total: lyricsJob.total,
            detail: lyricsJob.title,
            stop: () => void api.lyricsFillCancel(),
          }
        : null;

  return (
    <div className="page scrape">
      <header className="scrape-head">
        <h2>{tr("库的完整度")}</h2>
        <span className="scrape-head-hint">{tr("{total} 首里认出 {success} 首", { total: summary.total, success: summary.success })}{complete.skipped > 0 ? tr(" · 跳过 {skipped} 首", { skipped: complete.skipped }) : ""}
        </span>
        <div className="scrape-head-actions" ref={menuRef}>
          <CommandButton
            icon="scrape"
            label={running ? tr("刮削中…") : tr("一键刮削")}
            primary
            disabled={blocked}
            onClick={() => void startAll()}
            title={tr("按顺序做完：识别没认出的歌 → 歌手照片 → 补封面 → 补专辑信息。认不准的不会写进库，停在下面「需要你确认」")}
          />
          <button
            className="btn scrape-more"
            aria-label={tr("更多刮削操作")}
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((v) => !v)}
          >
            <Icon name="more" />
          </button>
          {menuOpen && (
            <div className="scrape-menu" role="menu">
              <button
                role="menuitem"
                disabled={blocked}
                onClick={() => void startJob("track", "identified", () => api.scrapeStart(true, 1000), tr("没有要刮的：每首都刮过了"))}
              >{tr("刮新加的歌")}<span>{tr("只做第一步：识别还没认出的歌")}</span>
              </button>
              <button
                role="menuitem"
                disabled={blocked}
                onClick={() => void startJob("track", "identified", () => api.scrapeStart(false, 1000), tr("库里没有歌"))}
              >{tr("全部重刮")}<span>{tr("每一首重新搜一遍，要十几分钟")}</span>
              </button>
              <button
                role="menuitem"
                disabled={blocked || summary.failed === 0}
                onClick={() => {
                  setMenuOpen(false);
                  void runOnce("identified", async () => tr("{v} 首失败的打回待刮，点「刮新加的歌」重试", { v: await api.scrapeRetryFailed() }));
                }}
              >{tr("重试失败的")}<span>{summary.failed === 0 ? tr("没有失败的") : tr("{failed} 首；需要确认的不动——那缺的是决定不是重试", { failed: summary.failed })}</span>
              </button>
            </div>
          )}
        </div>
      </header>

      <LyricsUpgradePanel onChanged={() => { void refresh(); onChanged(); }} onError={onError} />
      <div className="scrape-rows">
        <CompletenessRow
          label={tr("曲目识别")}
          count={complete.identified}
          note={notes.identified}
          action={
            gap(complete.identified) > 0
              ? {
                  label: tr("刮 {identified} 首", { identified: gap(complete.identified) }),
                  disabled: blocked,
                  run: () => void startJob("track", "identified", () => api.scrapeStart(true, 1000), tr("没有要刮的")),
                  title: tr("失败过的、需要确认的、跳过的也算在没认出里；需要确认的在下面挑"),
                }
              : undefined
          }
        />
        <CompletenessRow
          label={tr("曲目封面")}
          count={complete.trackCovers}
          note={notes.trackCovers}
          action={
            gap(complete.trackCovers) > 0
              ? {
                  label: busyRow === "trackCovers" ? tr("补封面中…") : tr("补 {trackCovers} 张", { trackCovers: gap(complete.trackCovers) }),
                  disabled: blocked,
                  run: () =>
                    void runOnce("trackCovers", async () => {
                      const r = await api.scrapeBackfillCovers();
                      return r.checked === 0
                        ? tr("每首歌都有封面了")
                        : tr("补上 {filled}", { filled: r.filled }) +
                            (r.failed > 0 ? tr("，还是下不到 {failed}", { failed: r.failed }) : "") +
                            (r.noCandidate > 0 ? tr("，{noCandidate} 首没有候选，要重刮", { noCandidate: r.noCandidate }) : "");
                    }),
                  title: tr("用刮削时存下的候选重新取封面（MusicBrainz 的封面在 Cover Art Archive），不重新搜索"),
                }
              : undefined
          }
        />
        <CompletenessRow
          label={tr("专辑封面")}
          count={complete.albumCovers}
          note={notes.albumCovers}
          action={
            gap(complete.albumCovers) > 0
              ? {
                  label: busyRow === "albumCovers" ? tr("填封面中…") : tr("补 {albumCovers} 张", { albumCovers: gap(complete.albumCovers) }),
                  disabled: blocked,
                  run: () =>
                    void runOnce("albumCovers", async () => {
                      const n = await api.fillAlbumArtwork();
                      return n === 0 ? tr("填不上：这些专辑的曲目也还没有封面，先补曲目封面") : tr("填了 {n} 张", { n });
                    }),
                  title: tr("同一张专辑的曲目封面就是专辑封面，直接拿来用，不额外发请求"),
                }
              : undefined
          }
        />
        <CompletenessRow
          label={tr("专辑信息")}
          count={complete.albumInfo}
          note={notes.albumInfo}
          action={
            gap(complete.albumInfo) > 0
              ? {
                  label: tr("补 {n} 张", { n: gap(complete.albumInfo) }),
                  disabled: blocked,
                  run: () => void startJob("album", "albumInfo", () => api.scrapeAlbumFillStart(), tr("每张专辑的发行类型和曲目数都齐了")),
                  title: tr("发行类型（单曲 / EP / 专辑）、整张碟的曲目数、厂牌。对已经识别的歌重新匹配一次，只补空着的字段，不改动已有的信息和识别状态"),
                }
              : undefined
          }
        />
        <CompletenessRow
          label={tr("歌手照片")}
          count={photos}
          note={notes.photos}
          action={
            missingArtists.length > 0
              ? {
                  label: tr("刮 {length} 位", { length: missingArtists.length }),
                  disabled: blocked,
                  run: () => void startJob("artist", "photos", () => api.scrapeArtistsStart(true), tr("每个歌手都已经有照片了")),
                  title:
                    tr("MusicBrainz 拿资料和别名，Deezer 拿照片。顺序不能反——很多日本歌手在 Deezer 上按罗马字收录（ずっと真夜中でいいのに。→ ZUTOMAYO），不带别名查不到"),
                }
              : undefined
          }
        >
          {missingArtists.length > 0 && (
            <div className="scrape-missing">
              {missingArtists.map((a) => (
                <button
                  key={a.name}
                  className="scrape-missing-artist"
                  disabled={blocked}
                  title={tr("{name}：{trackCount} 首，还没有照片。点一下单独刮这一位", { name: a.name, trackCount: a.trackCount })}
                  onClick={() =>
                    void runOnce("photos", async () => {
                      const got = await api.scrapeArtist(a.name);
                      return got.notFound ? tr("{name}：一条都没查到", { name: a.name }) : got.imagePath ? tr("{name}：照片有了", { name: a.name }) : tr("{name}：认出来了，但没有照片", { name: a.name });
                    })
                  }
                >
                  <Avatar path={a.imagePath} name={a.name} size={32} />
                  <span>{a.name}</span>
                </button>
              ))}
            </div>
          )}
        </CompletenessRow>
        <CompletenessRow
          label={tr("歌词")}
          count={complete.lyrics}
          note={notes.lyrics}
          action={
            gap(complete.lyrics) > 0 && !lyricsRunning
              ? {
                  label: tr("补 {lyrics} 首", { lyrics: gap(complete.lyrics) }),
                  disabled: blocked,
                  run: () => {
                    setLyricsJob(EMPTY_LYRICS);
                    api.lyricsFillStart().catch((e) => {
                      setLyricsJob(null);
                      errorRef.current(errorText(e));
                    });
                  },
                  title: tr("使用已启用的歌词来源，优先逐字，找不到时回退逐行；不确定的保留待复核"),
                }
              : undefined
          }
        />
        <CompletenessRow
          label={tr("时长")}
          count={complete.durations}
          note={notes.durations}
          action={
            gap(complete.durations) > 0
              ? {
                  label: busyRow === "durations" ? tr("读时长中…") : tr("补 {durations} 首", { durations: gap(complete.durations) }),
                  disabled: blocked,
                  run: () =>
                    void runOnce("durations", async () => {
                      const r = await api.backfillDurations();
                      return tr("补上 {written}", { written: r.written }) + (r.missing > 0 ? tr("，{missing} 个文件不在了", { missing: r.missing }) : "") + (r.failed + r.unknown > 0 ? tr("，{v} 个读不出来", { v: r.failed + r.unknown }) : "");
                    }),
                  title: tr("扫一遍音频文件，把读得出的时长写进库"),
                }
              : undefined
          }
        />
      </div>

      {jobLine !== null && (
        <div className="scrape-job" role="status">
          <span className="scrape-job-label">{jobLine.label}</span>
          <div className="scrape-bar">
            <div style={{ width: `${jobLine.total > 0 ? (jobLine.done / jobLine.total) * 100 : 0}%` }} />
          </div>
          <span className="scrape-job-count">
            {jobLine.done}/{jobLine.total}
          </span>
          <span className="scrape-job-detail">{jobLine.detail}</span>
          <CommandButton icon="cancel" label={tr("中断")} onClick={jobLine.stop} />
        </div>
      )}

      <TimedLyricsPanel onChanged={() => { void refresh(); onChanged(); }} onError={onError} />

      <section className="section scrape-review">
        <header className="section-head">
          <h2>{tr("需要你确认 · {length}", { length: review.length })}</h2>
          <span className="section-hint">{tr("置信度不够的不会自动写进库，停在这里等你挑")}</span>
        </header>
        {notes.review !== undefined && <p className="scrape-note">{notes.review}</p>}
        {review.length === 0 ? (
          <p className="section-empty">{tr("刮削结果都很确定，没有要你看的。")}</p>
        ) : (
          <div className="list">
            {review.map((item) => (
              <ReviewRow
                key={item.filePath}
                item={item}
                open={openPath === item.filePath}
                history={openPath === item.filePath ? history : []}
                onAccept={(index) => void accept(item, index)}
                onSkip={() => void skip(item)}
                onToggleHistory={() => void showHistory(item)}
              />
            ))}
          </div>
        )}

        <p className="scrape-others">
          {(["failed", "skipped"] as const).map((key) => {
            const n = key === "failed" ? summary.failed : summary.skipped;
            return (
              <button
                key={key}
                className={`link-btn${extra === key ? " on" : ""}`}
                disabled={n === 0}
                onClick={() => setExtra((v) => (v === key ? null : key))}
              >
                {SCRAPE_STATUS_LABELS[key]} · {n}
              </button>
            );
          })}
        </p>
        {extra !== null && (
          <div className="list">
            {extraItems.length === 0 ? (
              <p className="section-empty">{tr("这一类是空的。")}</p>
            ) : (
              extraItems.map((item) => (
                <ReviewRow
                  key={item.filePath}
                  item={item}
                  open={openPath === item.filePath}
                  history={openPath === item.filePath ? history : []}
                  onAccept={(index) => void accept(item, index)}
                  onSkip={() => void skip(item)}
                  onToggleHistory={() => void showHistory(item)}
                />
              ))
            )}
          </div>
        )}
      </section>
    </div>
  );
}

const EMPTY_PROGRESS: ScrapeProgress = {
  kind: "track",
  done: 0,
  total: 0,
  songId: "",
  title: "",
  status: "",
  confidence: 0,
  matched: "",
  coverSaved: false,
  finished: false,
  cancelled: false,
  error: "",
  step: 0,
  steps: 0,
};

const EMPTY_LYRICS: LyricsProgress = {
  done: 0,
  total: 0,
  songId: "",
  title: "",
  status: "",
  source: "",
  matched: "",
  lyricLines: 0,
  message: "",
  error: "",
  filled: 0,
  notFound: 0,
  failed: 0,
  finished: false,
  cancelled: false,
};

/**
 * 完整度的一行：名字、进度条、齐了几个 / 一共几个、右边一个按钮。
 * 齐了就打勾变灰、不给按钮——没有事要做的行不该抢眼。
 */
function CompletenessRow({
  label,
  count,
  note,
  action,
  children,
}: {
  label: string;
  count: CompletenessCount;
  note?: string | undefined;
  action?: { label: string; run: () => void; disabled?: boolean; title?: string } | undefined;
  children?: React.ReactNode;
}) {
  const full = count.total > 0 && count.done >= count.total;
  const empty = count.total === 0;
  const pct = count.total > 0 ? (count.done / count.total) * 100 : 0;
  return (
    <div className={`scrape-row${full || empty ? " done" : ""}`}>
      <span className="scrape-row-mark" aria-hidden="true">
        <Icon name={full || empty ? "check" : "status"} />
      </span>
      <span className="scrape-row-label">{label}</span>
      <div className="scrape-bar">
        <div style={{ width: `${pct}%` }} />
      </div>
      <span className="scrape-row-count">{empty ? "—" : `${count.done.toLocaleString()} / ${count.total.toLocaleString()}`}</span>
      <span className="scrape-row-action">
        {action !== undefined && (
          <button onClick={action.run} disabled={action.disabled} title={action.title}>
            {action.label}
          </button>
        )}
      </span>
      {note !== undefined && note !== "" && <span className="scrape-row-note">{note}</span>}
      {children !== undefined && <div className="scrape-row-extra">{children}</div>}
    </div>
  );
}

function ReviewRow({
  item,
  open,
  history,
  onAccept,
  onSkip,
  onToggleHistory,
}: {
  item: ReviewItem;
  open: boolean;
  history: ScrapeAttempt[];
  onAccept: (index: number) => void;
  onSkip: () => void;
  onToggleHistory: () => void;
}) {
  const hint = ERROR_HINTS[item.errorType];
  return (
    <div className="track">
      <div className="track-title">
        {item.localTitle || item.filePath}
        <span className={`tag scrape-${item.status}`}>
          {SCRAPE_STATUS_LABELS[item.status]} {item.confidence.toFixed(2)}
        </span>
      </div>
      <div className="track-meta">
        <span>{item.localArtist || "—"}</span>
        <span>{item.localAlbum || "—"}</span>
        {item.retryCount > 0 && <span>{tr("试过 {retryCount} 次", { retryCount: item.retryCount })}</span>}
        <button className="chip" onClick={onToggleHistory}>
          {open ? tr("收起历史") : tr("历史")}
        </button>
      </div>

      {item.errorMessage !== "" && (
        <p className={item.errorType === "" ? "muted small" : "warn"}>
          {item.errorMessage}
          {hint ? `　—　${hint}` : ""}
        </p>
      )}
      {/* 打分解释：用户要能看出为什么它不确定 */}
      {item.explain !== "" && <p className="muted small">{item.explain}</p>}

      {item.candidates.length > 0 && (
        <div className="candidates">
          {item.candidates.slice(0, 5).map((c, index) => (
            <CandidateRow
              key={`${c.provider}-${c.providerId}-${index}`}
              candidate={c}
              local={item}
              onAccept={() => onAccept(index)}
            />
          ))}
        </div>
      )}

      <div className="toolbar">
        <button onClick={onSkip}>{tr("跳过这首")}</button>
      </div>

      {open && (
        <div className="attempts">
          {history.length === 0 ? (
            <p className="muted small">{tr("没有历史记录。")}</p>
          ) : (
            history.map((a) => (
              <p key={a.id} className="muted small">
                {a.attemptedAt} · {SCRAPE_STATUS_LABELS[a.status]}
                {a.provider ? ` · ${a.provider}` : ""}
                {a.errorMessage ? ` · ${a.errorMessage}` : ""}
              </p>
            ))
          )}
        </div>
      )}
    </div>
  );
}

function CandidateRow({
  candidate,
  local,
  onAccept,
}: {
  candidate: ScrapeCandidate;
  local: ReviewItem;
  onAccept: () => void;
}) {
  const score = candidate.breakdown?.final_score ?? 0;
  // 和本地不同的字段标出来——用户判断「该不该接受」看的就是这个
  const differs = (a: string, b: string) => a.trim() !== "" && b.trim() !== "" && a !== b;
  return (
    <div className="candidate">
      {candidate.thumbUrl !== "" && (
        <img src={candidate.thumbUrl} alt="" width={48} height={48} loading="lazy" />
      )}
      <div className="candidate-body">
        <div className={differs(candidate.title, local.localTitle) ? "warn" : ""}>
          {candidate.title || "—"}
        </div>
        <div className="muted small">
          <span className={differs(candidate.artist, local.localArtist) ? "warn" : ""}>
            {candidate.artist || "—"}
          </span>
          {" · "}
          <span>{candidate.album || "—"}</span>
          {" · "}
          <span>{formatDuration(candidate.durationSec)}</span>
          {" · "}
          <span>{candidate.year || "—"}</span>
          {" · "}
          <span>{candidate.provider}</span>
        </div>
        {candidate.breakdown !== null && candidate.breakdown.penalties.length > 0 && (
          <div className="muted small">
            {candidate.breakdown.penalties
              .map((p) => `${p.reason} ${p.delta.toFixed(2)}`)
              .join("　")}
          </div>
        )}
      </div>
      <div className="candidate-actions">
        <b>{score.toFixed(3)}</b>
        <button onClick={onAccept}>{tr("采用")}</button>
      </div>
    </div>
  );
}

/** 进度行的标题。一键刮削时是「第 2/4 步 · 歌手照片」 */
function scrapeStepLabel(p: ScrapeProgress): string {
  const what =
    p.kind === "artist" ? tr("歌手照片") : p.kind === "album" ? tr("专辑信息") : p.kind === "cover" ? tr("补封面") : tr("识别曲目");
  return p.steps > 0 ? tr("第 {step}/{steps} 步 · {what}", { step: p.step, steps: p.steps, what }) : what;
}
