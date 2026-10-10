/**
 * 导入页。要求书第二十条的完整链路在 Tauri 侧的入口：
 * 选目录**或选单曲** → 扫描 → **复核** → 确认导入。
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

import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { CommandButton } from "../components/CommandButton";
import { WatchedFolders } from "../components/WatchedFolders";

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

/** 「带着这几个文件来复核」：启动横幅上点「查看」时由外壳发过来。`nonce` 让同一批文件能再发一次 */
export interface ImportRequest {
  paths: string[];
  nonce: number;
}

interface Props {
  onError: (message: string) => void;
  /** 导入完成后通知外面刷新曲库 */
  onImported: () => void;
  /** 外面要这一页直接扫的文件。用掉之后调 `onRequestConsumed` 清空 */
  request?: ImportRequest | null;
  onRequestConsumed?: () => void;
}

type Filter = "all" | ImportAction["kind"];

/** Rust 侧 `jp_import::scan` 的 `AUDIO_EXTS` 加上 `UNSUPPORTED_AUDIO`。只用来给文件选择框当筛选，
 *  真正算不算音频由后端判断——两边不一致时，后端会把选错的文件报在 `ignored` 里。
 *
 *  opus 和 wma 放不了、不会入库，但**故意留在这里**：选得到，导入计划里才能告诉用户
 *  「这种格式不支持」；筛掉的话文件在选择框里凭空消失，用户只会以为没扫到。 */
const AUDIO_EXTENSIONS = ["flac", "mp3", "wav", "m4a", "ogg", "aac", "opus", "wma"];

/** 路径只显示文件名。完整路径太长，一行塞不下，而用户认的是歌名。 */
function fileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

export function ImportPage({ onError, onImported, request = null, onRequestConsumed }: Props) {
  const [path, setPath] = useState("");
  /** 选中的具体文件。非空时扫这些，忽略上面的路径框。 */
  const [files, setFiles] = useState<string[]>([]);
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [busy, setBusy] = useState<"" | "scanning" | "importing">("");
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [tokenize, setTokenize] = useState(true);
  const [filter, setFilter] = useState<Filter>("all");
  /** 导完一批加一：语料库文件夹那张卡片跟着重新查 */
  const [importedCount, setImportedCount] = useState(0);

  const pick = useCallback(async () => {
    try {
      const chosen = await openDialog({ directory: true, multiple: false });
      if (typeof chosen === "string") {
        setPath(chosen);
        setFiles([]);
      }
    } catch (e) {
      // 选目录失败不该让页面卡住——还能手敲路径
      onError(`打不开目录选择框：${String((e as Error)?.message ?? e)}`);
    }
  }, [onError]);

  const pickFiles = useCallback(async () => {
    try {
      const chosen = await openDialog({
        multiple: true,
        title: "选择音频文件",
        filters: [{ name: "音频", extensions: AUDIO_EXTENSIONS }],
      });
      const picked = chosen === null ? [] : Array.isArray(chosen) ? chosen : [chosen];
      if (picked.length > 0) {
        setFiles(picked);
        setPath("");
      }
    } catch (e) {
      onError(`打不开文件选择框：${String((e as Error)?.message ?? e)}`);
    }
  }, [onError]);

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

  const doScan = useCallback(async () => {
    const target = path.trim();
    if (files.length === 0 && target === "") {
      onError("先选一个目录，或者选几个音频文件");
      return;
    }
    await scanTargets(files, target);
  }, [path, files, onError, scanTargets]);

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
    reviewFiles(request.paths);
    onRequestConsumed?.();
  }, [request, reviewFiles, onRequestConsumed]);

  // 导入要好几分钟，进度走 `import://progress` 事件
  useEffect(() => {
    const unlisten = listen<ImportProgress>("import://progress", (event) => {
      setProgress(event.payload.finished ? null : event.payload);
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);

  const doImport = useCallback(async () => {
    setBusy("importing");
    setProgress(null);
    try {
      const result = await api.runImport(tokenize);
      setReport(result);
      const csv = result.csv;
      if (csv !== undefined && !["updated", "noFile", "unchanged"].includes(csv)) onError(csv);
      setScan(null);
      setFiles([]);
      setImportedCount((n) => n + 1);
      onImported();
    } catch (e) {
      onError(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy("");
      setProgress(null);
    }
  }, [tokenize, onError, onImported]);

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
    <div className="page tools">
      <p className="page-lead">
        选一个装着音频的目录，或者直接选几首歌。扫描只读取，不写库；看过计划再决定导不导。
      </p>

      <WatchedFolders
        onReview={reviewFiles}
        onError={onError}
        disabled={busy !== ""}
        refreshKey={importedCount}
      />

      <div className="card">
        <div className="toolbar">
          <input
            className="search"
            value={path}
            placeholder="音频目录，例如 D:\jp_corpus\raw\audio"
            onChange={(e) => setPath(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void doScan();
            }}
          />
          <CommandButton icon="folder" label="选目录…" onClick={() => void pick()} disabled={busy !== ""} />
          <CommandButton
            icon="import"
            label="选单曲…"
            onClick={() => void pickFiles()}
            disabled={busy !== ""}
            title="直接选一首或几首音频文件，不用先把它们挪进一个目录"
          />
          <CommandButton
            icon="scan"
            label={busy === "scanning" ? "扫描中…" : "扫描"}
            onClick={() => void doScan()}
            disabled={busy !== "" || (files.length === 0 && path.trim() === "")}
            primary
            title="只读：和现有曲库比对，什么都不写"
          />
        </div>
        {files.length > 0 && (
          <div className="chosen-files">
            <span className="muted small">选中 {files.length} 个文件</span>
            <ul className="list compact">
              {files.map((f) => (
                <li key={f}>{fileName(f)}</li>
              ))}
            </ul>
            <button className="link-btn" onClick={() => setFiles([])}>
              清空，改回按目录扫
            </button>
          </div>
        )}
      </div>

      {scan !== null && (
        <div className="card">
          <div className="card-head">
            <h2>计划</h2>
            <span className="muted small">共 {scan.summary.total} 个文件</span>
          </div>

          <div className="stats">
            <Stat label="新增" value={scan.summary.new} />
            <Stat label="已在库中" value={scan.summary.alreadyImported} />
            <Stat label="疑似重复" value={scan.summary.possibleDuplicates} />
            <Stat label="本批重复" value={scan.summary.duplicatesInBatch} />
            <Stat label="跳过" value={scan.summary.skipped} />
          </div>

          {scan.summary.possibleDuplicates > 0 && (
            <p className="warn">
              有 {scan.summary.possibleDuplicates} 首曲名歌手对得上、但文件不同。
              这类<strong>不会</strong>被导入——可能是换了音源，也可能真有两个版本，
              需要你自己判断。
            </p>
          )}
          {scan.ignored.length > 0 && (
            <p className="warn">
              这 {scan.ignored.length} 个不是音频文件，没有进计划：{scan.ignored.join("、")}。
              歌词文件不用在这里选——导入时会自动认音频旁边的同名 .lrc，
              也可以在曲库里给某一首单独导入歌词。
            </p>
          )}
          {!scan.tokenizerReady && (
            <p className="warn">
              找不到 Sudachi 词典，这次导入不会写分词。歌和歌词照常入库，
              之后补上词典再重新分词即可。
            </p>
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
                    {key === "all" ? "全部" : ACTION_LABELS[key]} {n}
                  </button>
                );
              },
            )}
          </div>

          <PlanTable items={shown} />

          <div className="toolbar">
            <label className="check">
              <input
                type="checkbox"
                checked={tokenize}
                onChange={(e) => setTokenize(e.target.checked)}
                disabled={!scan.tokenizerReady}
              />
              同时分词（慢一些，但语料立刻可检索）
            </label>
            <CommandButton
              icon="import"
              label={
                busy === "importing"
                  ? progress === null
                    ? "导入中…"
                    : `导入中 ${progress.done}/${progress.total}`
                  : `导入 ${scan.summary.new} 首`
              }
              onClick={() => void doImport()}
              disabled={busy !== "" || scan.summary.new === 0}
              primary
              title="写库就在这一步"
            />
            {/* 导入中这个按钮是「停止」：一首歌一个事务，停在哪儿哪儿之前是真导进去了 */}
            <CommandButton
              icon="discard"
              label={busy === "importing" ? "停止" : "丢弃"}
              onClick={() => void doCancel()}
              disabled={busy === "scanning"}
            />
          </div>
          {progress !== null && progress.title !== "" && (
            <p className="muted small">正在导：{progress.title}</p>
          )}
          {scan.summary.new === 0 && scan.summary.total > 0 && (
            <p className="muted small">这一批里没有新东西，不需要导入。</p>
          )}
        </div>
      )}

      {report !== null && (
        <div className="card">
          <div className="card-head">
            <h2>导入结果</h2>
          </div>
          {report.cancelled && (
            <p className="muted small">
              你中途停了。下面这 {report.tracks.length} 首是<strong>真的导进去了</strong>
              （一首一个事务），剩下的一行都没写——再点一次「导入」会接着往下走。
            </p>
          )}
          <div className="stats">
            <Stat label="成功" value={imported.length} />
            <Stat label="失败" value={failures.length} />
            <Stat
              label="歌词行"
              value={imported.reduce(
                (n, t) => n + (t.outcome.kind === "imported" ? t.outcome.lyricLines : 0),
                0,
              )}
            />
            <Stat
              label="词"
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
                label="分词校正已恢复"
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
              <p className="warn">
                下面这些没能导入，其余的已经进库了（每首歌一个事务，
                失败的那首一行都没留下）。重新扫描一次可以再试。
              </p>
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
            <p className="muted small">全部成功。</p>
          )}
        </div>
      )}
    </div>
  );
}

function PlanTable({ items }: { items: PlanItem[] }) {
  if (items.length === 0) {
    return <p className="muted small">这一类没有条目。</p>;
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
              {item.title || <span className="muted">（读不出曲名）</span>}
            </div>
            <div className="track-meta">
              <span>{item.artist || "—"}</span>
              <span>{item.album || "—"}</span>
              <span>{formatDuration(item.durationSec)}</span>
              <span className={item.hasLyrics ? "" : "muted"}>
                {item.hasLyrics ? "有歌词" : "无歌词"}
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
        <p className="muted small">还有 {items.length - cut.length} 条未显示。</p>
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
      const notes = [`将写入 id ${a.songId}`];
      if (item.warning !== null) notes.push(item.warning);
      if (item.artistSource === "folder") notes.push("歌手是从文件夹名猜的");
      if (item.artistSource === "none") notes.push("定不出歌手");
      return notes.join(" · ");
    }
    case "alreadyImported":
      return `已经是库里的 ${a.songId}`;
    case "possibleDuplicate":
      return `曲名歌手对上了库里的 ${a.songId}（${a.existingPath}），不会导入`;
    case "duplicateInBatch":
      return `本批里 ${a.firstPath} 已经占了这个位置`;
    case "skipped":
      return a.reason;
  }
  return item.warning ?? "";
}
