/**
 * 词典管理：导入、从旧版迁移、启用 / 停用、排序、删除。住在设置页「词典」那一页。
 *
 * 按内容分成释义 / 词频 / 发音几类（`dictKinds.ts`），一次看一类，顶上能按名字筛。
 * 每一行点开看作者、来源、警告，删除也在里面——删除不该和开关挨在一起。
 *
 * 排序用**指针事件**自己拖，不用 HTML5 拖放：Tauri 在 Windows 上为了接文件拖放，
 * 把 WebView2 的 HTML5 拖放事件吃掉了，原来那版 `draggable` 在应用里根本拖不动。
 * 拖的那一行跟手走，别的行让位时滑过去；松手后先滑进空位再写库。键盘：抓手上按 ↑ / ↓。
 */

import { useCallback, useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { listen } from "../events";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { CommandButton } from "../components/CommandButton";
import { Switch } from "../components/SettingCard";
import { prefersReducedMotion } from "../motion";
import {
  dictApi,
  type DictImportDone,
  type DictImportProgress,
  type DictionaryInfo,
  type LegacySource,
} from "./api";
import { DICT_KINDS, dictKind, dropIndex, reorderWithin, type DictKind } from "./dictKinds";
import { invalidateDictionaryStyles } from "./LookupResults";
import { tr } from "../i18n";

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

function IconTrash() {
  return (
    <svg {...ICON}>
      <path d="M5 7h14M10 7V5h4v2M7 7l.8 12h8.4L17 7" />
    </svg>
  );
}

function IconChevron() {
  return (
    <svg {...ICON}>
      <path d="m9 6 6 6-6 6" />
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
  if (counts.terms) parts.push(tr("{terms} 条", { terms: counts.terms.toLocaleString() }));
  const meta = counts.termMeta ?? {};
  if (meta.freq) parts.push(tr("词频 {freq}", { freq: meta.freq.toLocaleString() }));
  if (meta.pitch) parts.push(tr("音高 {pitch}", { pitch: meta.pitch.toLocaleString() }));
  if (meta.ipa) parts.push(`IPA ${meta.ipa.toLocaleString()}`);
  if (counts.media) parts.push(tr("图片 {media}", { media: counts.media.toLocaleString() }));
  if (d.warnings.length > 0) parts.push(tr("{length} 条警告", { length: d.warnings.length }));
  parts.push(d.revision);
  return parts.join(" · ");
}

export function errorMessage(e: unknown): string {
  return String((e as { message?: string })?.message ?? e);
}

/** 挑 zip 导入。词典页的空状态也用它，所以单独放出来 */
export async function pickDictionaryFiles(onError: (message: string) => void): Promise<string[]> {
  try {
    const picked = await openDialog({ multiple: true, filters: [{ name: tr("Yomitan 词典"), extensions: ["zip"] }] });
    if (picked === null) return [];
    return Array.isArray(picked) ? picked : [picked];
  } catch (e) {
    onError(errorMessage(e));
    return [];
  }
}

/** 上次看的是哪一类：离开设置页再回来还在 */
let lastKind: DictKind = "terms";

interface Props {
  onError: (message: string) => void;
}

export function DictionaryManager({ onError }: Props) {
  const [dictionaries, setDictionaries] = useState<DictionaryInfo[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [legacy, setLegacy] = useState<LegacySource[]>([]);
  const [importing, setImporting] = useState(false);
  const [progress, setProgress] = useState<DictImportProgress | null>(null);
  const [report, setReport] = useState<DictImportDone | null>(null);
  const [kind, setKind] = useState<DictKind>(lastKind);
  const [query, setQuery] = useState("");
  const errorRef = useRef(onError);
  errorRef.current = onError;

  useEffect(() => {
    lastKind = kind;
  }, [kind]);

  const reload = useCallback(async () => {
    try {
      const [list, sources] = await Promise.all([dictApi.list(), dictApi.legacySources()]);
      setDictionaries(list);
      setLegacy(sources);
      setLoaded(true);
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

  const startImport = useCallback(async (paths: string[]) => {
    if (paths.length === 0) return;
    try {
      setReport(null);
      await dictApi.importStart(paths);
      setImporting(true);
    } catch (e) {
      errorRef.current(errorMessage(e));
    }
  }, []);

  /** 这一类里第 from 本挪到第 to 本：先动界面，再写库 */
  const move = useCallback(
    (from: number, to: number) => {
      const next = reorderWithin(dictionaries, kind, from, to);
      if (next === dictionaries) return;
      setDictionaries(next);
      void act(() => dictApi.setOrder(next.map((d) => d.id)));
    },
    [dictionaries, kind, act],
  );

  const counts = useMemo(() => {
    const c: Record<DictKind, number> = { terms: 0, freq: 0, pitch: 0, other: 0 };
    for (const d of dictionaries) c[dictKind(d)] += 1;
    return c;
  }, [dictionaries]);
  const tabs = DICT_KINDS.filter((k) => k.kind !== "other" || counts.other > 0);
  const inKind = useMemo(() => dictionaries.filter((d) => dictKind(d) === kind), [dictionaries, kind]);
  const q = query.trim().toLowerCase();
  const filtering = q !== "";
  const shown = filtering ? inKind.filter((d) => d.title.toLowerCase().includes(q)) : inKind;
  const enabled = dictionaries.filter((d) => d.enabled).length;
  const migratable = legacy.filter((s) => s.exists && !s.imported);
  const hint = DICT_KINDS.find((k) => k.kind === kind)?.hint ?? "";

  return (
    <div className="dm">
      <div className="dm-head">
        <div className="dm-stats">
          <b>{dictionaries.length}</b>
          <span>{tr("本词典")}</span>
          <span className="dm-sep" />
          <b>{enabled}</b>
          <span>{tr("本启用")}</span>
        </div>
        <div className="dm-actions">
          {migratable.length > 0 && (
            <CommandButton
              icon="folder"
              label={tr("从旧版迁移 {length} 本", { length: migratable.length })}
              onClick={() => void startImport(migratable.map((s) => s.path))}
              disabled={importing}
              title={migratable.map((s) => s.name).join("\n")}
            />
          )}
          <CommandButton
            icon="import"
            label={tr("导入 .zip")}
            onClick={() => void pickDictionaryFiles(onError).then(startImport)}
            disabled={importing}
            primary
            title={tr("Yomitan 格式：词条、词频、音调都认，一次可以选好几本")}
          />
        </div>
      </div>

      {importing && (
        <div className="dm-progress" role="status">
          <span className="dm-spinner" aria-hidden="true" />
          <span className="dm-progress-text">
            {progress === null
              ? tr("准备导入…")
              : tr("第 {v} / {total} 本：{path}　{doneFiles} / {totalFiles}", { v: progress.index + 1, total: progress.total, path: basename(progress.path), doneFiles: progress.doneFiles, totalFiles: progress.totalFiles })}
          </span>
          {progress !== null && progress.totalFiles > 0 && (
            <span className="dm-progress-bar">
              <i style={{ width: `${Math.round((progress.doneFiles / progress.totalFiles) * 100)}%` }} />
            </span>
          )}
          <button className="tc-btn" onClick={() => void dictApi.importCancel()}>{tr("取消")}</button>
        </div>
      )}
      {report !== null && <ImportReport done={report} onClose={() => setReport(null)} />}

      {loaded && dictionaries.length === 0 && !importing ? (
        <div className="dm-empty">
          <p>{tr("还没有导入词典。")}</p>
          <p className="muted small">{tr("支持 Yomitan 格式的 zip：Jitendex、大辞泉、JPDB 词频、アクセント辞典等。")}</p>
        </div>
      ) : (
        <>
          <div className="dm-bar">
            <div className="dm-tabs" role="tablist" aria-label={tr("词典类别")}>
              {tabs.map((t) => (
                <button
                  key={t.kind}
                  role="tab"
                  aria-selected={kind === t.kind}
                  className={kind === t.kind ? "on" : ""}
                  onClick={() => setKind(t.kind)}
                >
                  {t.label}
                  <span className="dm-count">{counts[t.kind]}</span>
                </button>
              ))}
            </div>
            <input
              className="dm-search"
              type="search"
              placeholder={tr("按名字筛")}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          <p className="dm-hint">
            {filtering ? tr("筛选时不能拖动排序，清空筛选再拖。") : tr("{hint}。按住左边的抓手拖动，或者选中抓手按 ↑ / ↓。", { hint })}
          </p>
          <SortableDictionaries
            key={kind}
            items={shown}
            sortable={!filtering && !importing}
            importing={importing}
            onMove={move}
            onToggle={(d) => void act(() => dictApi.setEnabled(d.id, !d.enabled))}
            onDelete={(d) => void act(() => dictApi.remove(d.id))}
          />
          {shown.length === 0 && <p className="muted small dm-none">{filtering ? tr("没有名字对得上的") : tr("这一类还没有词典")}</p>}
        </>
      )}

      {legacy.length > 0 && (
        <details className="small muted dict-legacy">
          <summary>{tr("旧版登记的词典包（{length}）", { length: legacy.length })}</summary>
          <ul>
            {legacy.map((s) => (
              <li key={s.path} title={s.path}>
                {s.name} · {s.kind} · {s.imported ? tr("已导入") : s.exists ? tr("可迁移") : tr("原始文件不在了")}
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}

/* ── 拖动排序的列表 ── */

interface Drag {
  from: number;
  to: number;
  /** 被拖那一行挪了多少（含拖动中容器自己滚的那段） */
  dy: number;
  /** 松手后正在滑进空位：这一帧起不再跟手 */
  settling: boolean;
}

/** 开始拖时量下来的东西。放 ref 里：pointermove 一秒几十次，不该每次都重算 */
interface DragGeometry {
  rects: { top: number; height: number }[];
  startY: number;
  pointerId: number;
  scroller: HTMLElement | null;
  startScroll: number;
  lastY: number;
  frame: number;
}

const GAP = 3;
/** 指针离滚动容器上下沿这么近时自动滚 */
const EDGE = 48;

function SortableDictionaries({
  items,
  sortable,
  importing,
  onMove,
  onToggle,
  onDelete,
}: {
  items: DictionaryInfo[];
  sortable: boolean;
  importing: boolean;
  onMove: (from: number, to: number) => void;
  onToggle: (d: DictionaryInfo) => void;
  onDelete: (d: DictionaryInfo) => void;
}) {
  const [drag, setDrag] = useState<Drag | null>(null);
  const [frozen, setFrozen] = useState(false);
  const [open, setOpen] = useState<number | null>(null);
  const rows = useRef<(HTMLDivElement | null)[]>([]);
  const geo = useRef<DragGeometry | null>(null);
  const dragRef = useRef<Drag | null>(null);
  dragRef.current = drag;

  const update = useCallback((clientY: number) => {
    const g = geo.current;
    const d = dragRef.current;
    if (g === null || d === null || d.settling) return;
    g.lastY = clientY;
    const scrolled = (g.scroller?.scrollTop ?? 0) - g.startScroll;
    const dy = clientY - g.startY + scrolled;
    setDrag({ ...d, dy, to: dropIndex(g.rects, d.from, dy) });
  }, []);

  // 拖到滚动容器边上就跟着滚
  const autoScroll = useCallback(() => {
    const g = geo.current;
    if (g === null || g.scroller === null || dragRef.current === null || dragRef.current.settling) return;
    const box = g.scroller.getBoundingClientRect();
    const speed = g.lastY < box.top + EDGE ? -(box.top + EDGE - g.lastY) : g.lastY > box.bottom - EDGE ? g.lastY - (box.bottom - EDGE) : 0;
    if (speed !== 0) {
      g.scroller.scrollTop += Math.max(-18, Math.min(18, speed / 3));
      update(g.lastY);
    }
    g.frame = requestAnimationFrame(autoScroll);
  }, [update]);

  const begin = useCallback(
    (index: number, event: ReactPointerEvent<HTMLButtonElement>) => {
      if (!sortable || event.button !== 0) return;
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      const scroller = event.currentTarget.closest(".page");
      geo.current = {
        rects: rows.current.slice(0, items.length).map((el) => {
          const r = el?.getBoundingClientRect();
          return { top: r?.top ?? 0, height: r?.height ?? 0 };
        }),
        startY: event.clientY,
        pointerId: event.pointerId,
        scroller: scroller instanceof HTMLElement ? scroller : null,
        startScroll: scroller instanceof HTMLElement ? scroller.scrollTop : 0,
        lastY: event.clientY,
        frame: 0,
      };
      const next = { from: index, to: index, dy: 0, settling: false };
      dragRef.current = next;
      setDrag(next);
      geo.current.frame = requestAnimationFrame(autoScroll);
    },
    [sortable, items.length, autoScroll],
  );

  const end = useCallback(() => {
    const g = geo.current;
    const d = dragRef.current;
    if (g === null || d === null || d.settling) return;
    cancelAnimationFrame(g.frame);
    if (d.to === d.from || prefersReducedMotion()) {
      setDrag(null);
      geo.current = null;
      if (d.to !== d.from) onMove(d.from, d.to);
      return;
    }
    // 先滑进空位，滑到了再真的换顺序（换完各行都回到 transform: none，不会闪）。
    // 偏移按内容坐标算：拖的途中容器滚过也一样，行和空位是跟着内容一起滚的
    setDrag({ ...d, dy: slotOffset(g.rects, d.from, d.to), settling: true });
    window.setTimeout(() => {
      setFrozen(true);
      setDrag(null);
      geo.current = null;
      onMove(d.from, d.to);
      // 两帧之后新顺序已经画上去了，再把过渡还回来
      requestAnimationFrame(() => requestAnimationFrame(() => setFrozen(false)));
    }, 200);
  }, [onMove]);

  useEffect(() => () => cancelAnimationFrame(geo.current?.frame ?? 0), []);

  const shiftOf = (i: number): number => {
    if (drag === null || i === drag.from) return 0;
    const h = (geo.current?.rects[drag.from]?.height ?? 0) + GAP;
    if (drag.from < drag.to && i > drag.from && i <= drag.to) return -h;
    if (drag.to < drag.from && i >= drag.to && i < drag.from) return h;
    return 0;
  };

  return (
    <div className={`dm-list ${drag !== null ? "sorting" : ""} ${frozen ? "frozen" : ""}`}>
      {items.map((d, i) => {
        const dragged = drag !== null && drag.from === i;
        const y = dragged ? drag.dy : shiftOf(i);
        const expanded = open === d.id;
        return (
          <div
            key={d.id}
            ref={(el) => {
              rows.current[i] = el;
            }}
            className={`dm-row ${d.enabled ? "" : "off"} ${dragged ? "lifted" : ""} ${dragged && drag.settling ? "settling" : ""} ${expanded ? "open" : ""}`}
            style={{ transform: y === 0 ? undefined : `translateY(${y}px)`, ["--n" as string]: i }}
          >
            <div className="dm-main">
              <button
                className="dict-grip"
                title={sortable ? tr("按住拖动排序（也可以按 ↑ / ↓）") : tr("筛选时不能排序")}
                aria-label={tr("调整 {title} 的顺序", { title: d.title })}
                disabled={!sortable}
                onPointerDown={(e) => begin(i, e)}
                onPointerMove={(e) => {
                  if (geo.current?.pointerId === e.pointerId) update(e.clientY);
                }}
                onPointerUp={end}
                onPointerCancel={end}
                onKeyDown={(event) => {
                  if (!sortable) return;
                  if (event.key === "ArrowUp") {
                    event.preventDefault();
                    onMove(i, i - 1);
                  } else if (event.key === "ArrowDown") {
                    event.preventDefault();
                    onMove(i, i + 1);
                  }
                }}
              >
                <IconGrip />
              </button>
              <span className="dm-rank">{i + 1}</span>
              <button className="dm-title" onClick={() => setOpen(expanded ? null : d.id)} aria-expanded={expanded}>
                <span className="dm-name">{d.title}</span>
                <span className="dm-meta">{describeCounts(d)}</span>
              </button>
              <Switch checked={d.enabled} disabled={importing} onChange={() => onToggle(d)} label={tr("启用 {title}", { title: d.title })} />
              <button
                className={`dm-expand ${expanded ? "on" : ""}`}
                onClick={() => setOpen(expanded ? null : d.id)}
                aria-label={expanded ? tr("收起") : tr("详情")}
              >
                <IconChevron />
              </button>
            </div>
            <div className="dm-detail" aria-hidden={!expanded}>
              <div>
                <DictionaryDetail d={d} importing={importing} onDelete={() => onDelete(d)} />
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}

/** 松手后被拖那一行要滑到的位置（相对原位） */
function slotOffset(rects: { top: number; height: number }[], from: number, to: number): number {
  const self = rects[from];
  const target = rects[to];
  if (self === undefined || target === undefined) return 0;
  return to > from ? target.top + target.height - self.height - self.top : target.top - self.top;
}

function DictionaryDetail({ d, importing, onDelete }: { d: DictionaryInfo; importing: boolean; onDelete: () => void }) {
  const [confirm, setConfirm] = useState(false);
  const facts: [string, string | null][] = [
    [tr("作者"), d.author],
    [tr("版本"), d.revision],
    [tr("语言"), [d.sourceLanguage, d.targetLanguage].filter(Boolean).join(" → ") || null],
    [tr("词频方式"), d.frequencyMode === "rank-based" ? tr("按名次（数字越小越常用）") : d.frequencyMode === "occurrence-based" ? tr("按出现次数") : d.frequencyMode],
    [tr("导入于"), d.importedAt > 0 ? new Date(d.importedAt).toLocaleString() : null],
    [tr("原始文件"), d.sourcePath || null],
  ];
  return (
    <div className="dm-detail-body">
      <dl>
        {facts
          .filter(([, v]) => v !== null && v !== "")
          .map(([k, v]) => (
            <div key={k}>
              <dt>{k}</dt>
              <dd title={v ?? ""}>{v}</dd>
            </div>
          ))}
        {d.url && (
          <div>
            <dt>{tr("主页")}</dt>
            <dd>{d.url}</dd>
          </div>
        )}
      </dl>
      {d.description && <p className="dm-desc">{d.description}</p>}
      {d.attribution && <p className="dm-desc muted">{d.attribution}</p>}
      {d.warnings.length > 0 && (
        <ul className="dm-warn">
          {d.warnings.slice(0, 6).map((w, i) => (
            <li key={i}>{w}</li>
          ))}
          {d.warnings.length > 6 && <li>{tr("……还有 {v} 条", { v: d.warnings.length - 6 })}</li>}
        </ul>
      )}
      <div className="dm-detail-actions">
        {confirm ? (
          <>
            <span className="small muted">{tr("只删词典库里的数据，原始 zip 不动。")}</span>
            <button className="tc-btn" onClick={() => setConfirm(false)}>{tr("取消")}</button>
            <button className="tc-btn danger" onClick={onDelete} autoFocus>{tr("确认删除")}</button>
          </>
        ) : (
          <button className="tc-btn dm-delete" onClick={() => setConfirm(true)} disabled={importing}>
            <IconTrash />{" "}{tr("删除这本")}</button>
        )}
      </div>
    </div>
  );
}

function ImportReport({ done, onClose }: { done: DictImportDone; onClose: () => void }) {
  const ok = done.results.filter((r) => r.summary !== null);
  const failed = done.results.filter((r) => r.error !== null);
  return (
    <div className="dict-report">
      <button className="dm-report-close" onClick={onClose} aria-label={tr("关闭")}>
        ×
      </button>
      <b>{tr("导入")}{done.cancelled ? tr("已取消") : tr("完成")}{tr("：成功 {length} 本", { length: ok.length })}{failed.length > 0 ? tr("，失败 {length} 本", { length: failed.length }) : ""}
      </b>
      <ul>
        {ok.map((r) =>
          r.summary === null ? null : (
            <li key={r.path}>{tr("{title}：{terms} 条，{toFixed} 秒", { title: r.summary.title, terms: r.summary.terms.toLocaleString(), toFixed: (r.summary.elapsedMs / 1000).toFixed(1) })}{r.summary.warningCount > 0 ? tr("，{warningCount} 条警告", { warningCount: r.summary.warningCount }) : ""}
            </li>
          ),
        )}
        {failed.map((r) => (
          <li key={r.path} className="failed" title={r.path}>
            {basename(r.path)}{tr("：")}{r.error}
          </li>
        ))}
      </ul>
    </div>
  );
}
