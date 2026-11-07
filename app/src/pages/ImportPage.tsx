/**
 * 曲库 › 添加音乐（原来的导入页）。要求书第二十条的完整链路在 Tauri 侧的入口：
 * 选目录**或选单曲** → 扫描 → **复核** → 确认导入。选完就扫，拖进窗口也一样（拖放由外壳接，经 `request` 传进来）。
 *
 * 两个入口共用后面的全部流程。单曲导入不是「另一条导入路径」，
 * 只是把选中的文件交给同一个扫描器（`scan_dir` 本来就认单个文件）——
 * 不然「下载目录里新增的那一首」只能靠重扫整个目录、再在几百条计划里找。
 *
 * 复核这一步不能省。导入会往库里写几千行，用户有权在写之前看清
 * 「哪些是新的、哪些被判成重复、哪些读不出曲名」。所以扫描是只读的，
 * 写库要再点一次。
 *
 * 疑似重复默认**不导**。曲名歌手对得上但文件不同，可能是换了音源，
 * 也可能真有两个版本——猜错会把库弄脏，这种事交给人判断。
 */

import { LyricsUpgradePanel } from "../components/LyricsUpgradePanel";
import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "../events";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { CommandButton } from "../components/CommandButton";
import { AUDIO_EXTENSIONS } from "../dropFiles";

import {
  ACTION_LABELS,
  api,
  formatDuration,
  type ImportAction,
  type ImportProgress,
  type ImportReport,
  type PlanItem,
  type ScanResult,
} from "../api";
import { Stat } from "../components/Stat";
import { tr, trBackend } from "../i18n";

/** 「带着这几个文件来复核」：启动横幅上点「查看」时由外壳发过来。`nonce` 让同一批文件能再发一次 */
export interface ImportRequest {
  paths: string[];
  folder?: string;
  nonce: number;
}

interface Props {
  onError: (message: string) => void;
  /** 导入完成后通知外面刷新曲库 */
  onImported: () => void;
  /** 外面要这一页直接扫的文件。用掉之后调 `onRequestConsumed` 清空 */
  request?: ImportRequest | null;
  onRequestConsumed?: () => void;
  /** 全部导进去了：回到曲库，打开第一首新歌 */
  onOpenSong?: (songId: string) => void;
}

type Filter = "all" | ImportAction["kind"];



export function ImportPage({ onError, onImported, request = null, onRequestConsumed, onOpenSong }: Props) {
  const [path, setPath] = useState("");
  /** 选中的具体文件。非空时扫这些，忽略上面的路径框。 */
  const [files, setFiles] = useState<string[]>([]);
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [busy, setBusy] = useState<"" | "scanning" | "importing">("");
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [tokenize, setTokenize] = useState(true);
  const [filter, setFilter] = useState<Filter>("all");

  /** 扫这些文件（`chosen` 非空）或者这个目录，把计划摆出来等复核 */
  const scanTargets = useCallback(
    async (chosen: string[], folder: string) => {
      setBusy("scanning");
      setReport(null);
      try {
        const result = chosen.length > 0 ? await api.scanFiles(chosen) : await api.scanFolder(folder);
        setScan(result);
        setFilter(result.summary.new > 0 ? "new" : "all");
      } catch (e) {
        setScan(null);
        onError(String((e as { message?: string })?.message ?? e));
      } finally {
        setBusy("");
      }
    },
    [onError],
  );

  // 选完就扫：中间不再多一下「扫描」——扫描只读，看过计划才写库
  const pick = useCallback(async () => {
    try {
      const chosen = await openDialog({ directory: true, multiple: false });
      if (typeof chosen === "string") {
        setPath(chosen);
        setFiles([]);
        void scanTargets([], chosen);
      }
    } catch (e) {
      onError(tr("打不开目录选择框：{String}", { String: String((e as Error)?.message ?? e) }));
    }
  }, [onError, scanTargets]);

  const pickFiles = useCallback(async () => {
    try {
      const chosen = await openDialog({
        multiple: true,
        title: tr("选择音频文件"),
        filters: [{ name: tr("音频"), extensions: AUDIO_EXTENSIONS }],
      });
      const picked = chosen === null ? [] : Array.isArray(chosen) ? chosen : [chosen];
      if (picked.length > 0) {
        setFiles(picked);
        setPath("");
        void scanTargets(picked, "");
      }
    } catch (e) {
      onError(tr("打不开文件选择框：{String}", { String: String((e as Error)?.message ?? e) }));
    }
  }, [onError, scanTargets]);

  /** 别处交过来的一批文件（启动横幅的「查看」、上面那张卡片的「查看这几首」）：当成选中的单曲，直接扫 */
  const reviewFiles = useCallback(
    (paths: string[]) => {
      if (paths.length === 0) return;
      setFiles(paths);
      setPath("");
      void scanTargets(paths, "");
    },
    [scanTargets],
  );
  useEffect(() => {
    if (request === null) return;
    if (request.folder !== undefined) {
      setFiles([]);
      setPath(request.folder);
      void scanTargets([], request.folder);
    } else reviewFiles(request.paths);
    onRequestConsumed?.();
  }, [request, reviewFiles, scanTargets, onRequestConsumed]);

  // 导入要好几分钟，进度走 `import://progress` 事件
  useEffect(() => {
    const unlisten = listen<ImportProgress>("import://progress", (event) => {
      setProgress(event.payload.finished ? null : event.payload);
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);

  const [fetchLyrics, setFetchLyrics] = useState(true);
  const doImport = useCallback(async () => {
    setBusy("importing");
    setProgress(null);
    try {
      const result = await api.runImport(tokenize, fetchLyrics);
      setReport(result);
      const csv = result.csv;
      if (csv !== undefined && !["updated", "noFile", "unchanged"].includes(csv)) onError(csv);
      setScan(null);
      setFiles([]);
      onImported();
      // 一首不落地导进去了就直接去听；有失败的留在这里，把原因摆出来
      const first = result.tracks.find((t) => t.outcome.kind === "imported");
      if (first !== undefined && !result.cancelled && result.tracks.every((t) => t.outcome.kind === "imported")) {
        onOpenSong?.(first.songId);
      }
    } catch (e) {
      onError(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy("");
      setProgress(null);
    }
  }, [tokenize, fetchLyrics, onError, onImported, onOpenSong]);

  /** 丢弃还没导的那批；导入进行中则是「停止」——后端每导完一首看一次标志。 */
  const doCancel = useCallback(async () => {
    await api.cancelImport().catch(() => {});
    setScan(null);
    setFilter("all");
  }, []);

  const shown = useMemo(
    () =>
      scan === null
        ? []
        : filter === "all"
          ? scan.items
          : scan.items.filter((i) => i.action.kind === filter),
    [scan, filter],
  );

  const failures = report?.tracks.filter((t) => t.outcome.kind === "failed") ?? [];
  const imported = report?.tracks.filter((t) => t.outcome.kind === "imported") ?? [];

  return (
    <div className="page tools add-music" data-scanning={busy === "scanning"} data-import-plan={scan !== null}>
      <div className="add-start">
        <div className="add-start-actions">
          <CommandButton icon="folder" label={tr("选文件夹…")} onClick={() => void pick()} disabled={busy !== ""} primary={scan === null} />
          <CommandButton icon="import" label={tr("选几首歌…")} onClick={() => void pickFiles()} disabled={busy !== ""} />
        </div>
        <p className="muted small">
          {busy === "scanning"
            ? tr("正在扫描…")
            : files.length > 0
              ? tr("{length} 个文件", { length: files.length })
              : path !== ""
                ? path
                : tr("也可以直接拖进窗口")}
        </p>
      </div>
      <LyricsUpgradePanel allowStart={false} onChanged={onImported} onError={onError} />

      {scan !== null && (
        <div className="card">
          <div className="card-head">
            <h2>{tr("计划")}</h2>
            <span className="muted small">{tr("共 {total} 个文件", { total: scan.summary.total })}</span>
          </div>

          <div className="stats">
            <Stat label={tr("新增")} value={scan.summary.new} />
            <Stat label={tr("已在库中")} value={scan.summary.alreadyImported} />
            <Stat label={tr("疑似重复")} value={scan.summary.possibleDuplicates} />
            <Stat label={tr("本批重复")} value={scan.summary.duplicatesInBatch} />
            <Stat label={tr("跳过")} value={scan.summary.skipped} />
          </div>

          {scan.summary.possibleDuplicates > 0 && (
            <p className="warn">{tr("{n} 首疑似重复，不导入", { n: scan.summary.possibleDuplicates })}</p>
          )}
          {scan.ignored.length > 0 && (
            <p className="warn">{tr("不是音频，跳过：{ignored}", { ignored: scan.ignored.join(tr("、")) })}</p>
          )}
          {!scan.tokenizerReady && (
            <p className="warn">{tr("还没有分词词典，这次不分词")}</p>
          )}

          <div className="chips">
            {(["all", "new", "alreadyImported", "possibleDuplicate", "duplicateInBatch", "skipped"] as Filter[]).map(
              (key) => {
                const n =
                  key === "all"
                    ? scan.items.length
                    : scan.items.filter((i) => i.action.kind === key).length;
                if (n === 0 && key !== "all") return null;
                return (
                  <button
                    key={key}
                    className={`chip${filter === key ? " on" : ""}`}
                    onClick={() => setFilter(key)}
                  >
                    {key === "all" ? tr("全部") : ACTION_LABELS[key]} {n}
                  </button>
                );
              },
            )}
          </div>

          <PlanTable items={shown} />

          <div className="toolbar import-go">
            <label className="check">
              <input
                type="checkbox"
                checked={tokenize}
                onChange={(e) => setTokenize(e.target.checked)}
                disabled={!scan.tokenizerReady}
              />{tr("同时分词")}</label>
            <label className="check import-lyrics-option">
              <input type="checkbox" checked={fetchLyrics} disabled={busy !== ""} onChange={(e) => setFetchLyrics(e.target.checked)} />{tr("补歌词")}</label>
            <CommandButton
              icon="import"
              label={
                busy === "importing"
                  ? progress === null
                    ? tr("导入中…")
                    : tr("导入中 {done}/{total}", { done: progress.done, total: progress.total })
                  : scan.summary.new === 0 && (scan.summary.filesChanged ?? 0) > 0
                    ? tr("刷新 {n} 个变了的文件", { n: scan.summary.filesChanged ?? 0 })
                    : tr("导入 {new} 首", { new: scan.summary.new })
              }
              onClick={() => void doImport()}
              disabled={busy !== "" || (scan.summary.new === 0 && (scan.summary.filesChanged ?? 0) === 0)}
              primary
            />
            {/* 导入中这个按钮是「停止」：一首歌一个事务，停在哪儿哪儿之前是真导进去了 */}
            <CommandButton
              icon="discard"
              label={busy === "importing" ? tr("停止") : tr("丢弃")}
              onClick={() => void doCancel()}
              disabled={busy === "scanning"}
            />
          </div>
          {progress !== null && progress.title !== "" && (
            <p className="muted small">{tr("正在导：{title}", { title: progress.title })}</p>
          )}
          {(scan.summary.filesChanged ?? 0) > 0 && (
            <p className="muted small">{tr("{n} 个文件变了，导入时刷新", { n: scan.summary.filesChanged ?? 0 })}</p>
          )}
          {scan.summary.new === 0 && (scan.summary.filesChanged ?? 0) === 0 && scan.summary.total > 0 && (
            <p className="muted small">{tr("没有新歌。")}</p>
          )}
        </div>
      )}

      {report !== null && (
        <div className="card">
          <div className="card-head">
            <h2>{tr("导入结果")}</h2>
          </div>
          {(report.filesRefreshed ?? 0) > 0 && (
            <p className="muted small">{tr("刷新了 {n} 个文件。", { n: report.filesRefreshed ?? 0 })}</p>
          )}
          {report.cancelled && (
            <p className="muted small">{tr("中途停了：{length} 首已经导进去，其余没写。再导一次会接着往下走。", { length: report.tracks.length })}</p>
          )}
          <div className="stats">
            <Stat label={tr("成功")} value={imported.length} />
            <Stat label={tr("失败")} value={failures.length} />
            <Stat
              label={tr("歌词行")}
              value={imported.reduce(
                (n, t) => n + (t.outcome.kind === "imported" ? t.outcome.lyricLines : 0),
                0,
              )}
            />
            <Stat
              label={tr("词")}
              value={imported.reduce(
                (n, t) => n + (t.outcome.kind === "imported" ? t.outcome.tokens : 0),
                0,
              )}
            />
            {/* 删歌再导入时才会有；平时是 0，不占位置 */}
            {imported.some(
              (t) => t.outcome.kind === "imported" && t.outcome.correctionsRestored > 0,
            ) && (
              <Stat
                label={tr("分词校正已恢复")}
                value={imported.reduce(
                  (n, t) =>
                    n + (t.outcome.kind === "imported" ? t.outcome.correctionsRestored : 0),
                  0,
                )}
              />
            )}
          </div>
          {failures.length > 0 ? (
            <>
              {/* 失败要逐条说清是哪首、为什么。只报一个数字等于没报。 */}
              <p className="warn">{tr("这些没能导入，其余已进库。")}</p>
              <ul className="list">
                {failures.map((t) => (
                  <li key={t.path}>
                    <strong>{t.title || t.path}</strong>
                    <div className="muted small">
                      {t.outcome.kind === "failed" ? t.outcome.error : ""}
                    </div>
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <p className="muted small">{tr("全部成功。")}</p>
          )}
        </div>
      )}
    </div>
  );
}

function PlanTable({ items }: { items: PlanItem[] }) {
  if (items.length === 0) {
    return <p className="muted small">{tr("这一类没有条目。")}</p>;
  }
  // 只渲染前 300 条。复核是抽查，不是逐条读完两千行；
  // 真要全看，筛选器能把范围缩小。
  const cut = items.slice(0, 300);
  return (
    <>
      <div className="list">
        {cut.map((item) => (
          <div className="track" key={item.path}>
            <div className="track-title">
              {item.title || <span className="muted">{tr("（读不出曲名）")}</span>}
            </div>
            <div className="track-meta">
              <span>{item.artist || "—"}</span>
              <span>{item.album || "—"}</span>
              <span>{formatDuration(item.durationSec)}</span>
              <span className={item.hasLyrics ? "" : "muted"}>
                {item.hasLyrics ? tr("有歌词") : tr("无歌词")}
              </span>
              <span className={`tag action-${item.action.kind}`}>
                {ACTION_LABELS[item.action.kind]}
              </span>
            </div>
            <div className="muted small">{explain(item)}</div>
          </div>
        ))}
      </div>
      {items.length > cut.length && (
        <p className="muted small">{tr("还有 {v} 条未显示。", { v: items.length - cut.length })}</p>
      )}
    </>
  );
}

/** 每条处置都要能说出理由——「为什么这首没导进去」是最常问的问题。 */
function explain(item: PlanItem): string {
  const a = item.action;
  switch (a.kind) {
    case "new": {
      // 新歌要写库，拿不准的地方在这里说出来：复核看的就是这几句
      const notes: string[] = [];
      if (a.versionOf) notes.push(tr("是库里 {songId} 的另一个版本：{reason}", { songId: a.versionOf.songId, reason: trBackend(a.versionOf.reason) }));
      if (item.warning !== null) notes.push(item.warning);
      if (item.artistSource === "folder") notes.push(tr("歌手是从文件夹名猜的"));
      if (item.artistSource === "none") notes.push(tr("定不出歌手"));
      return notes.join(" · ");
    }
    case "alreadyImported":
      return a.fileChanged
        ? tr("已经是库里的 {songId}；文件变了，导入时刷新文件信息", { songId: a.songId })
        : tr("已经是库里的 {songId}", { songId: a.songId });
    case "possibleDuplicate":
      return a.reason
        ? tr("和库里的 {songId}（{existingPath}）是同一个录音：{reason}。不会导入", { songId: a.songId, existingPath: a.existingPath, reason: trBackend(a.reason) })
        : tr("曲名歌手对上了库里的 {songId}（{existingPath}），不会导入", { songId: a.songId, existingPath: a.existingPath });
    case "duplicateInBatch":
      return tr("本批里 {firstPath} 已经占了这个位置", { firstPath: a.firstPath });
    case "skipped":
      return a.reason;
  }
  return item.warning ?? "";
}
