/**
 * 刮削页。要求书第十一条那个「Scraping Review UI」。
 *
 * 三件事，按重要性排：
 *
 * 1. **复核队列。** 置信度不够的停在这里等人看，而不是自信地猜错写进库。
 *    每条都并排给出「本地写的是什么」和「候选是什么」，外加打分解释——
 *    用户要能一眼看出为什么它不确定。
 * 2. **失败要说清原因，并说该怎么办。** 「限流」和「搜索无结果」都算失败，
 *    但前者等一会儿重试就好，后者重试一万次也一样。
 * 3. **批量刮削可中断。** 209 首要十几分钟，不能只给一个转圈的图标。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { CommandButton } from "../components/CommandButton";

import {
  api,
  ERROR_HINTS,
  formatDuration,
  SCRAPE_STATUS_LABELS,
  type ArtistRow,
  type ReviewItem,
  type ScrapeAttempt,
  type ScrapeCandidate,
  type ScrapeProgress,
  type ScrapeStatusName,
  type ScrapeSummaryCounts,
} from "../api";
import { Stat } from "../components/Stat";

interface Props {
  onError: (message: string) => void;
  /** 刮削会改 songs 行和封面，完事要让外面刷新 */
  onChanged: () => void;
}

const QUEUES: { key: ScrapeStatusName; label: string }[] = [
  { key: "low_confidence", label: "需确认" },
  { key: "failed", label: "失败" },
  { key: "success", label: "已匹配" },
  { key: "skipped", label: "已跳过" },
];

export function ScrapePage({ onError, onChanged }: Props) {
  const [summary, setSummary] = useState<ScrapeSummaryCounts | null>(null);
  const [queue, setQueue] = useState<ScrapeStatusName>("low_confidence");
  const [items, setItems] = useState<ReviewItem[]>([]);
  const [progress, setProgress] = useState<ScrapeProgress | null>(null);
  const [running, setRunning] = useState(false);
  const [busy, setBusy] = useState(false);
  const [openPath, setOpenPath] = useState<string | null>(null);
  const [history, setHistory] = useState<ScrapeAttempt[]>([]);
  const [artists, setArtists] = useState<ArtistRow[]>([]);
  const [albumNote, setAlbumNote] = useState("");
  const [backfilling, setBackfilling] = useState(false);
  const [coverNote, setCoverNote] = useState("");
  const [acceptNote, setAcceptNote] = useState("");
  const errorRef = useRef(onError);
  errorRef.current = onError;

  const refresh = useCallback(
    async (status: ScrapeStatusName) => {
      try {
        const [counts, rows, roster] = await Promise.all([
          api.scrapeSummary(),
          api.scrapeReviewQueue([status], 200),
          api.artistRoster(),
        ]);
        setSummary(counts);
        setItems(rows);
        setArtists(roster);
      } catch (e) {
        errorRef.current(String((e as { message?: string })?.message ?? e));
      }
    },
    [],
  );

  useEffect(() => {
    void refresh(queue);
    void api.scrapeIsRunning().then(setRunning).catch(() => undefined);
  }, [queue, refresh]);

  // 后台批量作业的进度
  useEffect(() => {
    const unlisten = listen<ScrapeProgress>("scrape://progress", (event) => {
      setProgress(event.payload);
      if (event.payload.finished) {
        setRunning(false);
        // 整批断了要说出来。以前这里只会安静地变成「完成」
        if (event.payload.error) errorRef.current(`刮削中断：${event.payload.error}`);
        void refresh(queue);
        onChanged();
      }
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, [queue, refresh, onChanged]);

  const start = useCallback(
    async (onlyMissing: boolean) => {
      setBusy(true);
      try {
        const total = await api.scrapeStart(onlyMissing, 1000);
        if (total === 0) {
          errorRef.current("没有需要刮削的曲目");
        } else {
          setRunning(true);
          setProgress({
            kind: "track",
            done: 0,
            total,
            songId: "",
            title: "",
            status: "",
            confidence: 0,
            matched: "",
            coverSaved: false,
            finished: false,
            cancelled: false,
            error: "",
          });
        }
      } catch (e) {
        errorRef.current(String((e as { message?: string })?.message ?? e));
      } finally {
        setBusy(false);
      }
    },
    [],
  );

  const accept = useCallback(
    async (item: ReviewItem, index: number) => {
      try {
        const applied = await api.scrapeAccept(item.filePath, index);
        // 说清楚实际写了什么——「采用」之前什么都不做过，
        // 现在要让用户看得见它确实做了事
        const parts = [
          applied.fieldsFilled > 0 ? `补了 ${applied.fieldsFilled} 个字段` : "",
          applied.creditsProtected > 0 ? `${applied.creditsProtected} 条信用标为人工` : "",
          applied.creditsAdded > 0 ? `新增 ${applied.creditsAdded} 条信用` : "",
          applied.coverPath !== null ? "封面已下载" : "",
        ].filter((p) => p !== "");
        setAcceptNote(
          `${item.localTitle || item.filePath}：${parts.length > 0 ? parts.join("，") : "已确认，没有需要补写的"}`,
        );
        await refresh(queue);
        onChanged();
      } catch (e) {
        errorRef.current(String((e as { message?: string })?.message ?? e));
      }
    },
    [queue, refresh, onChanged],
  );

  const skip = useCallback(
    async (item: ReviewItem) => {
      try {
        await api.scrapeSkip(item.filePath, "复核时跳过");
        await refresh(queue);
      } catch (e) {
        errorRef.current(String((e as { message?: string })?.message ?? e));
      }
    },
    [queue, refresh],
  );

  const showHistory = useCallback(
    async (item: ReviewItem) => {
      if (openPath === item.filePath) {
        setOpenPath(null);
        return;
      }
      setOpenPath(item.filePath);
      try {
        setHistory(await api.scrapeAttempts(item.filePath, 20));
      } catch {
        setHistory([]);
      }
    },
    [openPath],
  );

  /** 识别成功但没有封面的歌：用存下来的候选重取，不重新搜索 */
  const backfillCovers = useCallback(async () => {
    setBackfilling(true);
    setCoverNote("");
    try {
      const r = await api.scrapeBackfillCovers();
      setCoverNote(
        r.checked === 0
          ? "每首歌都有封面了"
          : `检查 ${r.checked} 首：补上 ${r.filled}` +
              (r.failed > 0 ? `，还是下不到 ${r.failed}` : "") +
              (r.noCandidate > 0 ? `，${r.noCandidate} 首没有候选（要重刮）` : ""),
      );
    } catch (e) {
      errorRef.current(e instanceof Error ? e.message : String(e));
    } finally {
      setBackfilling(false);
    }
  }, []);

  const startArtists = useCallback(async () => {
    setBusy(true);
    try {
      const total = await api.scrapeArtistsStart(true);
      if (total === 0) {
        errorRef.current("每个歌手都已经有照片了");
      } else {
        setRunning(true);
        setProgress({
          kind: "artist",
          done: 0,
          total,
          songId: "",
          title: "",
          status: "",
          confidence: 0,
          matched: "",
          coverSaved: false,
          finished: false,
          cancelled: false,
          error: "",
        });
      }
    } catch (e) {
      errorRef.current(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy(false);
    }
  }, []);

  const fillAlbums = useCallback(async () => {
    try {
      const n = await api.fillAlbumArtwork();
      setAlbumNote(
        n === 0
          ? "没有可填的：要么专辑都已经有封面，要么它们的曲目还没刮到封面。"
          : `填了 ${n} 张专辑封面（用的是曲目封面，没有额外发请求）。`,
      );
      onChanged();
    } catch (e) {
      errorRef.current(String((e as { message?: string })?.message ?? e));
    }
  }, [onChanged]);

  const missingPhotos = useMemo(
    () => artists.filter((a) => !a.hasImage).length,
    [artists],
  );

  const pct = useMemo(
    () => (progress && progress.total > 0 ? (progress.done / progress.total) * 100 : 0),
    [progress],
  );

  return (
    <div className="page tools">
      <p className="page-lead">
        识别曲目、补齐 metadata、下载封面。置信度不够的不会自动写进库，停在「需确认」等你看一眼。
      </p>

      <div className="card">
        <div className="card-head">
          <h2>状态</h2>
          {summary !== null && (
            <span className="muted small">共 {summary.total} 条记录</span>
          )}
        </div>
        {summary !== null && (
          <div className="stats">
            <Stat label="已匹配" value={summary.success} />
            <Stat label="需确认" value={summary.lowConfidence} />
            <Stat label="失败" value={summary.failed} />
            <Stat label="待刮" value={summary.pending} />
            <Stat label="已跳过" value={summary.skipped} />
          </div>
        )}

        {running && progress !== null && (
          <>
            <div className="seekbar" style={{ margin: "14px 0 6px" }}>
              <div style={{ width: `${pct}%` }} />
            </div>
            <p className="muted small">
              {progress.kind === "artist" ? "歌手 " : ""}
              {progress.done}/{progress.total}
              {progress.title ? ` · ${progress.title}` : ""}
              {progress.matched ? ` → ${progress.matched}` : ""}
            </p>
          </>
        )}
        {progress?.finished && (
          <p className="muted small">
            {progress.cancelled ? "已中断。" : "跑完了。"}
            已处理 {progress.done}/{progress.total}。
          </p>
        )}

        <div className="toolbar">
          <CommandButton
            icon="scrape"
            label={running ? "刮削中…" : "刮未匹配的"}
            onClick={() => void start(true)}
            disabled={busy || running}
            primary
          />
          <CommandButton icon="refresh" label="全部重刮" onClick={() => void start(false)} disabled={busy || running} />
          <CommandButton icon="cancel" label="中断" onClick={() => void api.scrapeCancel()} disabled={!running} />
          <CommandButton
            icon="retry"
            label="重试失败的"
            onClick={() => {
              void (async () => {
                const n = await api.scrapeRetryFailed().catch(() => 0);
                await refresh(queue);
                errorRef.current(`${n} 条已打回待刮`);
              })();
            }}
            disabled={busy || running}
            title="失败的打回待刮；需要确认的不动——那缺的是决策不是重试"
          />
        </div>
      </div>

      <div className="card">
        <div className="card-head">
          <h2>歌手照片</h2>
          <span className="muted small">
            {artists.length - missingPhotos}/{artists.length} 已有照片
          </span>
        </div>
        <p className="muted small">
          MusicBrainz 拿资料和别名，Deezer 拿照片。
          顺序不能反——大量日本歌手在 Deezer 上按罗马字收录
          （ずっと真夜中でいいのに。→ ZUTOMAYO），不带别名去查找不到。
        </p>
        <div className="artist-grid">
          {artists.map((a) => (
            <ArtistChip
              key={a.name}
              artist={a}
              disabled={busy || running}
              onScrape={async () => {
                try {
                  const got = await api.scrapeArtist(a.name);
                  await refresh(queue);
                  if (got.notFound) errorRef.current(`${a.name}：一条都没查到`);
                } catch (e) {
                  errorRef.current(String((e as { message?: string })?.message ?? e));
                }
              }}
            />
          ))}
        </div>
        <div className="toolbar">
          <CommandButton
            icon="photo"
            label={missingPhotos === 0 ? "照片都齐了" : `刮缺照片的 ${missingPhotos} 位`}
            onClick={() => void startArtists()}
            disabled={busy || running || missingPhotos === 0}
          />
          <CommandButton
            icon="album"
            label={backfilling ? "补封面中…" : "补齐缺失封面"}
            onClick={() => void backfillCovers()}
            disabled={busy || running || backfilling}
            title="识别成功但没有封面的歌：用刮削时存下的候选重取（MusicBrainz 的封面在 Cover Art Archive，老版本没去取），不重新搜索"
          />
          {coverNote !== "" && <span className="muted small">{coverNote}</span>}
        </div>
      </div>

      <div className="card">
        <div className="card-head">
          <h2>专辑封面</h2>
        </div>
        <p className="muted small">
          用曲目封面填 <code>albums.artwork_path</code>。
          同一张专辑的曲目封面就是这张专辑的封面，不额外发请求。
        </p>
        <div className="toolbar">
          <button onClick={() => void fillAlbums()} disabled={busy || running}>
            填专辑封面
          </button>
        </div>
        {albumNote !== "" && <p className="muted small">{albumNote}</p>}
      </div>

      <div className="card">
        <div className="chips">
          {QUEUES.map((q) => (
            <button
              key={q.key}
              className={`chip${queue === q.key ? " on" : ""}`}
              onClick={() => setQueue(q.key)}
            >
              {q.label}
            </button>
          ))}
        </div>

        {acceptNote !== "" && <p className="muted small">{acceptNote}</p>}
        {items.length === 0 ? (
          <p className="muted small">
            {queue === "low_confidence"
              ? "没有需要确认的。要么还没刮，要么全都很确定。"
              : "这一类是空的。"}
          </p>
        ) : (
          <div className="list">
            {items.map((item) => (
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
      </div>
    </div>
  );
}

/**
 * 歌手那一格。**没有照片的整格就是按钮**——原来在格子里再塞一个「刮」的小方块，
 * 一排下来全是嵌套的方块，也看不出哪些能点。
 */
function ArtistChip({
  artist,
  disabled,
  onScrape,
}: {
  artist: ArtistRow;
  disabled: boolean;
  onScrape: () => void;
}) {
  const meta = `${artist.trackCount} 首${artist.country ? ` · ${artist.country}` : ""}${artist.formed ? ` · ${artist.formed}` : ""}`;
  if (artist.hasImage) {
    return (
      <div className="artist-chip">
        <span className="chip-name">{artist.name}</span>
        <span className="muted small">{meta}</span>
      </div>
    );
  }
  return (
    <button
      className="artist-chip missing"
      disabled={disabled}
      onClick={onScrape}
      title={`${artist.name}：还没有照片，点一下去刮`}
    >
      <span className="chip-name">{artist.name}</span>
      <span className="muted small">{meta} · 没有照片，点一下刮</span>
    </button>
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
        {item.retryCount > 0 && <span>试过 {item.retryCount} 次</span>}
        <button className="chip" onClick={onToggleHistory}>
          {open ? "收起历史" : "历史"}
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
        <button onClick={onSkip}>跳过这首</button>
      </div>

      {open && (
        <div className="attempts">
          {history.length === 0 ? (
            <p className="muted small">没有历史记录。</p>
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
        <button onClick={onAccept}>采用</button>
      </div>
    </div>
  );
}
