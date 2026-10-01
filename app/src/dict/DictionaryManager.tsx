/**
 * 词典管理：导入、从旧版迁移、启用 / 停用、排序、删除。
 *
 * 这一块**住在设置页**，不占着词典页的右边——查词的时候没人在管词典，
 * 管词典的时候也不是在查词。词典页只在一本都没有时给一个导入的入口。
 *
 * 排序可以直接拖（查词结果按这个顺序排），也能用键盘：抓手上按 ↑ / ↓。
 * 拖完立刻写库，没有「保存」。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { CommandButton } from "../components/CommandButton";
import { SettingCard, SettingGroup, Switch } from "../components/SettingCard";
import {
  dictApi,
  type DictImportDone,
  type DictImportProgress,
  type DictionaryInfo,
  type LegacySource,
} from "./api";
import { invalidateDictionaryStyles } from "./LookupResults";

const ICON = {
  viewBox: "0 0 24 24",
  width: 15,
  height: 15,
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.6,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
};

function IconBook() {
  return (
    <svg {...ICON}>
      <path d="M5 5.5A1.5 1.5 0 0 1 6.5 4H19v16H6.5A1.5 1.5 0 0 1 5 18.5z" />
      <path d="M9 4v16" />
    </svg>
  );
}

function IconTrash() {
  return (
    <svg {...ICON}>
      <path d="M5 7h14M10 7V5h4v2M7 7l.8 12h8.4L17 7" />
    </svg>
  );
}

/** 拖动抓手：六个点，和各家列表里的一样 */
function IconGrip() {
  return (
    <svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor" aria-hidden="true">
      <circle cx="9" cy="6" r="1.4" />
      <circle cx="15" cy="6" r="1.4" />
      <circle cx="9" cy="12" r="1.4" />
      <circle cx="15" cy="12" r="1.4" />
      <circle cx="9" cy="18" r="1.4" />
      <circle cx="15" cy="18" r="1.4" />
    </svg>
  );
}

export function basename(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function describeCounts(d: DictionaryInfo): string {
  const parts: string[] = [];
  const counts = d.counts;
  if (counts.terms) parts.push(`${counts.terms.toLocaleString()} 条`);
  const meta = counts.termMeta ?? {};
  if (meta.freq) parts.push(`词频 ${meta.freq.toLocaleString()}`);
  if (meta.pitch) parts.push(`音高 ${meta.pitch.toLocaleString()}`);
  if (meta.ipa) parts.push(`IPA ${meta.ipa.toLocaleString()}`);
  if (counts.media) parts.push(`图片 ${counts.media.toLocaleString()}`);
  if (d.warnings.length > 0) parts.push(`${d.warnings.length} 条警告`);
  parts.push(d.revision);
  return parts.join(" · ");
}

export function errorMessage(e: unknown): string {
  return String((e as { message?: string })?.message ?? e);
}

/** 挑 zip 导入。词典页的空状态也用它，所以单独放出来 */
export async function pickDictionaryFiles(onError: (message: string) => void): Promise<string[]> {
  try {
    const picked = await openDialog({ multiple: true, filters: [{ name: "Yomitan 词典", extensions: ["zip"] }] });
    if (picked === null) return [];
    return Array.isArray(picked) ? picked : [picked];
  } catch (e) {
    onError(errorMessage(e));
    return [];
  }
}

interface Props {
  onError: (message: string) => void;
}

export function DictionaryManager({ onError }: Props) {
  const [dictionaries, setDictionaries] = useState<DictionaryInfo[]>([]);
  const [legacy, setLegacy] = useState<LegacySource[]>([]);
  const [importing, setImporting] = useState(false);
  const [progress, setProgress] = useState<DictImportProgress | null>(null);
  const [report, setReport] = useState<DictImportDone | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<number | null>(null);
  // 正在拖的那一行，和它现在悬在谁身上
  const [dragging, setDragging] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);
  const errorRef = useRef(onError);
  errorRef.current = onError;

  const reload = useCallback(async () => {
    try {
      const [list, sources] = await Promise.all([dictApi.list(), dictApi.legacySources()]);
      setDictionaries(list);
      setLegacy(sources);
    } catch (e) {
      errorRef.current(errorMessage(e));
    }
  }, []);

  useEffect(() => {
    void reload();
    void dictApi.isImporting().then(setImporting).catch(() => undefined);
  }, [reload]);

  useEffect(() => {
    const offProgress = listen<DictImportProgress>("dict://progress", (event) => {
      setImporting(true);
      setProgress(event.payload);
    });
    const offDone = listen<DictImportDone>("dict://done", (event) => {
      setImporting(false);
      setProgress(null);
      setReport(event.payload);
      if (event.payload.error !== null) errorRef.current(event.payload.error);
      invalidateDictionaryStyles();
      void reload();
    });
    return () => {
      void offProgress.then((f) => f());
      void offDone.then((f) => f());
    };
  }, [reload]);

  const act = useCallback(
    async (action: () => Promise<void>) => {
      try {
        await action();
        invalidateDictionaryStyles();
        await reload();
      } catch (e) {
        errorRef.current(errorMessage(e));
      }
    },
    [reload],
  );

  const startImport = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0) return;
      try {
        setReport(null);
        await dictApi.importStart(paths);
        setImporting(true);
      } catch (e) {
        errorRef.current(errorMessage(e));
      }
    },
    [],
  );

  /** 把第 from 本挪到第 to 本的位置，立刻写库 */
  const moveTo = useCallback(
    (from: number, to: number) => {
      if (from === to || to < 0 || to >= dictionaries.length) return;
      const next = [...dictionaries];
      const [moved] = next.splice(from, 1);
      if (moved === undefined) return;
      next.splice(to, 0, moved);
      setDictionaries(next); // 先动界面，别等一圈往返
      void act(() => dictApi.setOrder(next.map((d) => d.id)));
    },
    [dictionaries, act],
  );

  const migratable = legacy.filter((s) => s.exists && !s.imported);

  return (
    <>
      <SettingGroup title="词典">
        <SettingCard icon={<IconBook />} title="导入词典" description="Yomitan 格式的 zip：词条、词频、音调都认">
          <CommandButton
            icon="import"
            label="导入 .zip"
            onClick={() => void pickDictionaryFiles(onError).then(startImport)}
            disabled={importing}
            primary
          />
        </SettingCard>
        {migratable.length > 0 && (
          <SettingCard
            icon={<IconBook />}
            title="从旧版迁移"
            description={`PyQt 版登记过的包还在，${migratable.length} 本可以直接重新导入`}
          >
            <CommandButton
              icon="folder"
              label={`迁移 ${migratable.length} 本`}
              onClick={() => void startImport(migratable.map((s) => s.path))}
              disabled={importing}
              title={migratable.map((s) => s.name).join("\n")}
            />
          </SettingCard>
        )}
        {importing && (
          <SettingCard
            title="正在导入"
            description={
              progress === null
                ? "准备导入…"
                : `第 ${progress.index + 1} / ${progress.total} 本：${basename(progress.path)}　${progress.doneFiles} / ${progress.totalFiles} ${progress.file}`
            }
          >
            <button className="tc-btn" onClick={() => void dictApi.importCancel()}>
              取消导入
            </button>
          </SettingCard>
        )}
      </SettingGroup>

      {report !== null && <ImportReport done={report} />}

      <SettingGroup title={dictionaries.length === 0 ? "已安装" : `已安装（${dictionaries.length} 本）· 拖动排序`}>
        {dictionaries.length === 0 && !importing && (
          <SettingCard
            icon={<IconBook />}
            title="还没有导入词典"
            description="支持 Yomitan 格式的 zip（Jitendex、大辞泉、JPDB 词频、アクセント辞典等）"
          />
        )}
        {dictionaries.map((d, i) => (
          <div
            key={d.id}
            className={`dict-row ${dragging === i ? "dragging" : ""} ${over === i && dragging !== null && dragging !== i ? (dragging < i ? "drop-after" : "drop-before") : ""}`}
            draggable={!importing}
            onDragStart={(event) => {
              setDragging(i);
              event.dataTransfer.effectAllowed = "move";
              // Firefox 要有数据才认这次拖动
              event.dataTransfer.setData("text/plain", String(d.id));
            }}
            onDragOver={(event) => {
              if (dragging === null) return;
              event.preventDefault();
              event.dataTransfer.dropEffect = "move";
              setOver(i);
            }}
            onDrop={(event) => {
              event.preventDefault();
              if (dragging !== null) moveTo(dragging, i);
              setDragging(null);
              setOver(null);
            }}
            onDragEnd={() => {
              setDragging(null);
              setOver(null);
            }}
          >
            <SettingCard
              icon={
                <button
                  className="dict-grip"
                  title="拖动排序（也可以按 ↑ / ↓）"
                  aria-label={`调整 ${d.title} 的顺序`}
                  disabled={importing}
                  onKeyDown={(event) => {
                    if (event.key === "ArrowUp") {
                      event.preventDefault();
                      moveTo(i, i - 1);
                    } else if (event.key === "ArrowDown") {
                      event.preventDefault();
                      moveTo(i, i + 1);
                    }
                  }}
                >
                  <IconGrip />
                </button>
              }
              title={d.title}
              description={describeCounts(d)}
            >
              <Switch
                checked={d.enabled}
                disabled={importing}
                onChange={() => void act(() => dictApi.setEnabled(d.id, !d.enabled))}
                label={`启用 ${d.title}`}
              />
              {confirmDelete === d.id ? (
                <button
                  className="tc-btn danger"
                  onClick={() => {
                    setConfirmDelete(null);
                    void act(() => dictApi.remove(d.id));
                  }}
                  onBlur={() => setConfirmDelete(null)}
                  autoFocus
                >
                  确认删除
                </button>
              ) : (
                <button
                  className="dict-delete"
                  onClick={() => setConfirmDelete(d.id)}
                  disabled={importing}
                  title="只删词典库里的数据，不动原始 zip"
                  aria-label={`删除 ${d.title}`}
                >
                  <IconTrash />
                </button>
              )}
            </SettingCard>
          </div>
        ))}
      </SettingGroup>

      {legacy.length > 0 && (
        <details className="small muted dict-legacy">
          <summary>旧版登记的词典包（{legacy.length}）</summary>
          <ul>
            {legacy.map((s) => (
              <li key={s.path} title={s.path}>
                {s.name} · {s.kind} · {s.imported ? "已导入" : s.exists ? "可迁移" : "原始文件不在了"}
              </li>
            ))}
          </ul>
        </details>
      )}
    </>
  );
}

function ImportReport({ done }: { done: DictImportDone }) {
  const ok = done.results.filter((r) => r.summary !== null);
  const failed = done.results.filter((r) => r.error !== null);
  return (
    <div className="dict-report">
      <b>
        导入{done.cancelled ? "已取消" : "完成"}：成功 {ok.length} 本
        {failed.length > 0 ? `，失败 ${failed.length} 本` : ""}
      </b>
      <ul>
        {ok.map((r) =>
          r.summary === null ? null : (
            <li key={r.path}>
              {r.summary.title}：{r.summary.terms.toLocaleString()} 条，{(r.summary.elapsedMs / 1000).toFixed(1)} 秒
              {r.summary.warningCount > 0 ? `，${r.summary.warningCount} 条警告` : ""}
            </li>
          ),
        )}
        {failed.map((r) => (
          <li key={r.path} className="failed" title={r.path}>
            {basename(r.path)}：{r.error}
          </li>
        ))}
      </ul>
    </div>
  );
}
