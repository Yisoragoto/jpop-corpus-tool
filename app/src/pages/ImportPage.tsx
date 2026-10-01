/**
 * 导入页。要求书第二十条的完整链路在 Tauri 侧的入口：
 * 选目录 → 扫描 → **复核** → 确认导入。
 *
 * 复核这一步不能省。导入会往库里写几千行，用户有权在写之前看清
 * 「哪些是新的、哪些被判成重复、哪些读不出曲名」。所以扫描是只读的，
 * 写库要再点一次。
 *
 * 疑似重复默认**不导**。曲名歌手对得上但文件不同，可能是换了音源，
 * 也可能真有两个版本——猜错会把库弄脏，这种事交给人判断。
 */

import { useCallback, useMemo, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { CommandButton } from "../components/CommandButton";

import {
  ACTION_LABELS,
  api,
  formatDuration,
  type ImportAction,
  type ImportReport,
  type PlanItem,
  type ScanResult,
} from "../api";
import { Stat } from "../components/Stat";

interface Props {
  onError: (message: string) => void;
  /** 导入完成后通知外面刷新曲库 */
  onImported: () => void;
}

type Filter = "all" | ImportAction["kind"];

export function ImportPage({ onError, onImported }: Props) {
  const [path, setPath] = useState("");
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [busy, setBusy] = useState<"" | "scanning" | "importing">("");
  const [tokenize, setTokenize] = useState(true);
  const [filter, setFilter] = useState<Filter>("all");

  const pick = useCallback(async () => {
    try {
      const chosen = await openDialog({ directory: true, multiple: false });
      if (typeof chosen === "string") setPath(chosen);
    } catch (e) {
      // 选目录失败不该让页面卡住——还能手敲路径
      onError(`打不开目录选择框：${String((e as Error)?.message ?? e)}`);
    }
  }, [onError]);

  const doScan = useCallback(async () => {
    const target = path.trim();
    if (!target) {
      onError("先选一个目录");
      return;
    }
    setBusy("scanning");
    setReport(null);
    try {
      const result = await api.scanFolder(target);
      setScan(result);
      setFilter(result.summary.new > 0 ? "new" : "all");
    } catch (e) {
      setScan(null);
      onError(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy("");
    }
  }, [path, onError]);

  const doImport = useCallback(async () => {
    setBusy("importing");
    try {
      const result = await api.runImport(tokenize);
      setReport(result);
      const csv = result.csv;
      if (csv !== undefined && !["updated", "noFile", "unchanged"].includes(csv)) onError(csv);
      setScan(null);
      onImported();
    } catch (e) {
      onError(String((e as { message?: string })?.message ?? e));
    } finally {
      setBusy("");
    }
  }, [tokenize, onError, onImported]);

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
        选一个装着音频的目录。扫描只读取，不写库；看过计划再决定导不导。
      </p>

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
          <CommandButton icon="folder" label="浏览…" onClick={() => void pick()} disabled={busy !== ""} />
          <CommandButton
            icon="scan"
            label={busy === "scanning" ? "扫描中…" : "扫描"}
            onClick={() => void doScan()}
            disabled={busy !== "" || path.trim() === ""}
            primary
            title="只读：扫描目录并和现有曲库比对，什么都不写"
          />
        </div>
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
              label={busy === "importing" ? "导入中…" : `导入 ${scan.summary.new} 首`}
              onClick={() => void doImport()}
              disabled={busy !== "" || scan.summary.new === 0}
              primary
              title="写库就在这一步"
            />
            <CommandButton icon="discard" label="丢弃" onClick={() => void doCancel()} disabled={busy !== ""} />
          </div>
          {scan.summary.new === 0 && (
            <p className="muted small">这个目录里没有新东西，不需要导入。</p>
          )}
        </div>
      )}

      {report !== null && (
        <div className="card">
          <div className="card-head">
            <h2>导入结果</h2>
          </div>
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
    case "new":
      return `将写入 id ${a.songId}`;
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
