/**
 * 曲库 › 音乐文件夹：语料库里的歌来自哪些文件夹，逐个重新扫描对账，移除，恢复移出的歌。
 *
 * 对账只读（后端只记扫描时间和文件的大小、修改时间）；看过每一组再确认才写库。
 * 新文件不在这里导入，交给导入页的复核。确认时后端会逐项重新核对，期间情况变了的跳过。
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";

import { sourcesApi, type RemovedSong, type SourcePlanView, type SourceSuggestions, type SourceView } from "../librarySources";
import { messageOf } from "../errors";
import { libraryApi } from "../libraryAdmin";
import { CommandButton } from "./CommandButton";
import { tr } from "../i18n";

interface Props {
  /** 把这些新文件交给导入页复核 */
  onReview: (paths: string[]) => void;
  /** 库变了（移出、恢复、刷新、删除）：曲库要重拉 */
  onChanged: () => void;
  onError: (message: string) => void;
  /** 变了就重新拉（导入、刮削之后） */
  version: number;
}

type Group = "changed" | "returned" | "moved" | "gone";

/** 正在看的对账：哪个文件夹、对出来什么、每组勾没勾 */
interface Review {
  plan: SourcePlanView;
  on: Record<Group, boolean>;
}

/** 移除前的确认，内联在那一行下面，不弹框 */
interface Removing {
  id: number;
  /** 第二步：彻底删除要再点一次 */
  hard: boolean;
}

function when(stamp: string | null): string {
  if (stamp === null) return tr("还没扫描过");
  const date = new Date(stamp.includes("T") || stamp.endsWith("Z") ? stamp : `${stamp.replace(" ", "T")}Z`);
  return Number.isNaN(date.getTime()) ? stamp : tr("上次扫描 {time}", { time: date.toLocaleString() });
}

function status(source: SourceView): string {
  const parts = [tr("{songs} 首", { songs: source.songs })];
  if (source.removed > 0) parts.push(tr("{removed} 首已移出", { removed: source.removed }));
  parts.push(source.recursive ? tr("含子文件夹") : tr("只看这一层"));
  if (!source.present) parts.push(tr("找不到这个文件夹（盘没插上？）"));
  else if (!source.enabled) parts.push(tr("已停用：启动时不检查"));
  else parts.push(when(source.lastScannedAt));
  return parts.join(" · ");
}

const PREVIEW = 6;

/** 一组对账结果：一个勾、一个标题、前几首 */
function GroupBlock({ title, note, on, onToggle, rows }: {
  title: string;
  note: string;
  on: boolean;
  onToggle: (on: boolean) => void;
  rows: { key: string; main: string; sub: string }[];
}) {
  const [all, setAll] = useState(false);
  if (rows.length === 0) return null;
  const shown = all ? rows : rows.slice(0, PREVIEW);
  return (
    <div className={`sync-group ${on ? "" : "off"}`}>
      <label className="check">
        <input type="checkbox" checked={on} onChange={(e) => onToggle(e.target.checked)} />
        <b>{title}</b>
        <span className="muted small">{note}</span>
      </label>
      <ul className="sync-rows">
        {shown.map((row) => (
          <li key={row.key}>
            <span>{row.main}</span>
            <span className="muted small" title={row.sub}>{row.sub}</span>
          </li>
        ))}
      </ul>
      {rows.length > PREVIEW && (
        <button className="link-btn" onClick={() => setAll(!all)}>
          {all ? tr("收起") : tr("还有 {more} 首", { more: rows.length - PREVIEW })}
        </button>
      )}
    </div>
  );
}

function song(title: string, artist: string): string {
  return artist ? `${title} · ${artist}` : title;
}

export function LibrarySources({ onReview, onChanged, onError, version }: Props) {
  const [sources, setSources] = useState<SourceView[] | null>(null);
  const [suggested, setSuggested] = useState<SourceSuggestions | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [removed, setRemoved] = useState<RemovedSong[]>([]);
  const [chosenRemoved, setChosenRemoved] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState<"" | "adding" | `scan:${number}` | "applying" | "removing" | "restoring">("");
  const [review, setReview] = useState<Review | null>(null);
  const [removing, setRemoving] = useState<Removing | null>(null);
  const [note, setNote] = useState("");
  const [dropping, setDropping] = useState(false);
  /** 已移出的歌里点了「彻底删除」，等第二次确认 */
  const [purging, setPurging] = useState(false);

  const reload = useCallback(async () => {
    try {
      const [list, gone] = await Promise.all([sourcesApi.list(), sourcesApi.removedSongs()]);
      setSources(list);
      setRemoved(gone);
      setChosenRemoved((prev) => new Set([...prev].filter((id) => gone.some((s) => s.id === id))));
      if (list.length === 0) {
        const s = await sourcesApi.suggestions();
        setSuggested(s);
        setPicked(new Set(s.folders.map((f) => f.path)));
      } else setSuggested(null);
    } catch (err) {
      onError(tr("读不了音乐文件夹：{err}", { err: messageOf(err) }));
    }
  }, [onError]);

  useEffect(() => {
    void reload();
  }, [reload, version]);

  const add = useCallback(async (folders: { path: string; recursive: boolean }[]) => {
    if (folders.length === 0) return;
    setBusy("adding");
    setNote("");
    try {
      setSources(await sourcesApi.add(folders));
      setSuggested(null);
      setNote(folders.length === 1
        ? tr("加好了：{path}。点「扫描」看看里面有没有新歌、变动", { path: folders[0]!.path })
        : tr("加好了 {length} 个文件夹", { length: folders.length }));
      onChanged();
    } catch (err) {
      onError(messageOf(err));
    } finally {
      setBusy("");
    }
  }, [onChanged, onError]);

  const pickFolder = async () => {
    try {
      const chosen = await openDialog({ directory: true, multiple: true, title: tr("选放音乐的文件夹") });
      const paths = typeof chosen === "string" ? [chosen] : (chosen ?? []);
      await add(paths.map((path) => ({ path, recursive: true })));
    } catch (err) {
      onError(tr("打不开目录选择框：{String}", { String: messageOf(err) }));
    }
  };

  // 把文件夹拖进来就是添加。拖进来的是文件也交给后端判断（会报「找不到这个文件夹」）
  useEffect(() => {
    let alive = true;
    const off = getCurrentWebview().onDragDropEvent((event) => {
      if (!alive) return;
      const kind = event.payload.type;
      if (kind === "enter" || kind === "over") setDropping(true);
      else if (kind === "leave") setDropping(false);
      else if (kind === "drop") {
        setDropping(false);
        void add(event.payload.paths.map((path) => ({ path, recursive: true })));
      }
    });
    return () => {
      alive = false;
      void off.then((f) => f());
    };
  }, [add]);

  const scan = async (source: SourceView) => {
    setBusy(`scan:${source.id}`);
    setNote("");
    setReview(null);
    setRemoving(null);
    try {
      const plan = await sourcesApi.plan(source.id);
      // 一个音频都没扫到：多半是盘换了内容、挂错了盘，「不见了」那组默认不勾
      setReview({ plan, on: { changed: true, returned: true, moved: true, gone: !plan.nothingFound } });
    } catch (err) {
      onError(messageOf(err));
    } finally {
      setBusy("");
      void reload();
    }
  };

  const apply = async () => {
    if (review === null) return;
    const { plan, on } = review;
    setBusy("applying");
    try {
      const done = await sourcesApi.apply({
        id: plan.source.id,
        refresh: on.changed ? plan.changed.map((c) => c.path) : [],
        restore: on.returned ? plan.returned.map((r) => r.songId) : [],
        relink: on.moved ? plan.moved.map((m) => ({ songId: m.songId, to: m.to })) : [],
        moveOut: on.gone ? plan.gone.map((g) => g.songId) : [],
      });
      const parts: string[] = [];
      if (done.refreshed > 0) parts.push(tr("刷新 {n} 首", { n: done.refreshed }));
      if (done.restored > 0) parts.push(tr("恢复 {n} 首", { n: done.restored }));
      if (done.relinked > 0) parts.push(tr("改路径 {n} 首", { n: done.relinked }));
      if (done.movedOut > 0) parts.push(tr("移出 {n} 首", { n: done.movedOut }));
      let text = parts.length > 0 ? parts.join(tr("，")) : tr("没有要写的");
      if (done.skipped > 0) text += tr("；{n} 项在扫描之后又变了，没写，再扫一次看看", { n: done.skipped });
      setNote(text);
      setReview(null);
      onChanged();
    } catch (err) {
      onError(messageOf(err));
    } finally {
      setBusy("");
      void reload();
    }
  };

  const toggle = async (source: SourceView) => {
    try {
      setSources(await sourcesApi.setEnabled(source.id, !source.enabled));
    } catch (err) {
      onError(messageOf(err));
    }
  };

  const remove = async (id: number, mode: "moveOut" | "delete") => {
    setBusy("removing");
    try {
      const done = await sourcesApi.remove(id, mode);
      setSources(done.sources);
      setRemoving(null);
      if (review?.plan.source.id === id) setReview(null);
      setNote(mode === "delete"
        ? tr("文件夹已移除，删掉了 {n} 首（音频文件没动）", { n: done.deleted })
        : tr("文件夹已移除，{n} 首移出了语料库，可以在下面恢复", { n: done.movedOut }));
      onChanged();
    } catch (err) {
      onError(messageOf(err));
    } finally {
      setBusy("");
      void reload();
    }
  };

  const restore = async (ids: string[]) => {
    if (ids.length === 0) return;
    setBusy("restoring");
    try {
      const n = await sourcesApi.restore(ids);
      setNote(tr("恢复了 {n} 首", { n }));
      onChanged();
    } catch (err) {
      onError(messageOf(err));
    } finally {
      setBusy("");
      void reload();
    }
  };

  /** 一首一首删（和曲库里删一首一样）；删到一半出错，已经删掉的就是删掉了 */
  const purge = async (ids: string[]) => {
    setBusy("removing");
    let done = 0;
    try {
      for (const id of ids) {
        await libraryApi.remove(id);
        done += 1;
      }
      setNote(tr("彻底删除了 {n} 首（音频文件没动）", { n: done }));
    } catch (err) {
      onError(messageOf(err));
    } finally {
      setPurging(false);
      setBusy("");
      if (done > 0) onChanged();
      void reload();
    }
  };

  const plan = review?.plan ?? null;
  const pending = useMemo(() => {
    if (review === null) return 0;
    const { plan: p, on } = review;
    return (on.changed ? p.changed.length : 0) + (on.returned ? p.returned.length : 0)
      + (on.moved ? p.moved.length : 0) + (on.gone ? p.gone.length : 0);
  }, [review]);
  const setOn = (group: Group, value: boolean) =>
    setReview((r) => (r === null ? r : { ...r, on: { ...r.on, [group]: value } }));

  const removingSource = removing === null ? null : sources?.find((s) => s.id === removing.id) ?? null;

  return (
    <div className={`library-sources ${dropping ? "dropping" : ""}`}>
      <div className="section-head">
        <h2>{tr("音乐文件夹")}</h2>
        <span className="muted small">{tr("语料库里的歌来自这些文件夹。扫描只对账，看过再确认才写库")}</span>
      </div>

      {sources !== null && sources.length === 0 && (
        <div className="sources-empty">
          {suggested !== null && suggested.folders.length > 0 ? (
            <>
              <p>{tr("还没有设音乐文件夹。按库里的歌所在的位置，建议加这几个：")}</p>
              <ul className="sync-rows">
                {suggested.folders.map((f) => (
                  <li key={f.path}>
                    <label className="check">
                      <input
                        type="checkbox"
                        checked={picked.has(f.path)}
                        onChange={(e) => setPicked((prev) => {
                          const next = new Set(prev);
                          if (e.target.checked) next.add(f.path);
                          else next.delete(f.path);
                          return next;
                        })}
                      />
                      <span>{f.path}</span>
                    </label>
                    <span className="muted small">
                      {tr("{songs} 首", { songs: f.songs })}{" · "}{f.recursive ? tr("含子文件夹") : tr("只看这一层")}
                    </span>
                  </li>
                ))}
              </ul>
              {suggested.looseSongs > 0 && (
                <p className="muted small">{tr("另有 {n} 首不在这些文件夹里，不归任何文件夹管，照常在库里。", { n: suggested.looseSongs })}</p>
              )}
              <div className="toolbar">
                <CommandButton
                  icon="add"
                  label={tr("加这 {n} 个", { n: picked.size })}
                  primary
                  disabled={busy !== "" || picked.size === 0}
                  onClick={() => void add(suggested.folders.filter((f) => picked.has(f.path)).map((f) => ({ path: f.path, recursive: f.recursive })))}
                />
                <CommandButton icon="folder" label={tr("选别的文件夹…")} disabled={busy !== ""} onClick={() => void pickFolder()} />
              </div>
            </>
          ) : (
            <button className="drop-zone" onClick={() => void pickFolder()} disabled={busy !== ""}>
              <b>{tr("添加音乐文件夹")}</b>
              <span className="muted small">{tr("点这里选，或者把文件夹拖进窗口")}</span>
            </button>
          )}
        </div>
      )}

      {sources !== null && sources.length > 0 && (
        <>
          <ul className="folder-list">
            {sources.map((source) => (
              <li key={source.id} className={source.enabled && source.present ? "" : "off"}>
                <div className="folder-row">
                  <div className="folder-text">
                    <span className="folder-path" title={source.rootPath}>{source.rootPath}</span>
                    <span className="muted small">{status(source)}</span>
                    {source.lastScanError && <span className="error-text small">{source.lastScanError}</span>}
                  </div>
                  <div className="folder-actions">
                    <CommandButton
                      icon="scan"
                      label={busy === `scan:${source.id}` ? tr("正在扫描…") : tr("扫描")}
                      disabled={busy !== "" || !source.present}
                      onClick={() => void scan(source)}
                      title={tr("只读：和库里属于这个文件夹的歌对一遍")}
                    />
                    <button className="link-btn" disabled={busy !== ""} onClick={() => void toggle(source)}>
                      {source.enabled ? tr("停用") : tr("启用")}
                    </button>
                    <button className="link-btn" disabled={busy !== ""} onClick={() => setRemoving({ id: source.id, hard: false })}>{tr("移除…")}</button>
                  </div>
                </div>

                {removing?.id === source.id && removingSource !== null && (
                  <div className="inline-confirm">
                    {!removing.hard ? (
                      <>
                        <span>{source.songs > 0
                          ? tr("移除这个文件夹：它下面的 {n} 首会移出语料库，可以在下面「已移出的歌」里恢复。音频文件不动。", { n: source.songs })
                          : tr("移除这个文件夹？库里没有歌属于它。")}</span>
                        <div className="toolbar">
                          <CommandButton icon="check" label={tr("移除")} primary disabled={busy !== ""} onClick={() => void remove(source.id, "moveOut")} />
                          {source.songs + source.removed > 0 && (
                            <button className="link-btn danger" disabled={busy !== ""} onClick={() => setRemoving({ id: source.id, hard: true })}>{tr("改为彻底删除…")}</button>
                          )}
                          <button className="link-btn" onClick={() => setRemoving(null)}>{tr("取消")}</button>
                        </div>
                      </>
                    ) : (
                      <>
                        <span>{tr("彻底删除这 {n} 首：歌词、分词、收听记录都删掉，不能恢复（分词校正保留，音频文件不动）。", { n: source.songs + source.removed })}</span>
                        <div className="toolbar">
                          <CommandButton icon="discard" label={tr("彻底删除")} disabled={busy !== ""} onClick={() => void remove(source.id, "delete")} />
                          <button className="link-btn" onClick={() => setRemoving({ id: source.id, hard: false })}>{tr("返回")}</button>
                        </div>
                      </>
                    )}
                  </div>
                )}

                {plan !== null && review !== null && plan.source.id === source.id && (
                  <div className="sync-review">
                    {plan.nothingFound && (
                      <p className="error-text">{tr("这个文件夹里一个音频都没扫到。如果是盘换了内容或挂错了，别确认移出。")}</p>
                    )}
                    {plan.newFiles.length > 0 && (
                      <div className="sync-group">
                        <b>{tr("{n} 个新文件", { n: plan.newFiles.length })}</b>
                        <span className="muted small">
                          {plan.newSongs > 0
                            ? tr("其中 {n} 首是新歌，其余疑似重复或读不出曲名", { n: plan.newSongs })
                            : tr("都不算新歌（疑似重复、格式放不了或者读不出曲名）")}
                        </span>
                        <button className="link-btn" onClick={() => onReview(plan.newFiles)}>{tr("去导入页复核")}</button>
                      </div>
                    )}
                    <GroupBlock
                      title={tr("{n} 首文件变了", { n: plan.changed.length })}
                      note={tr("刷新文件信息；你改过的、刮削来的不动")}
                      on={review.on.changed}
                      onToggle={(v) => setOn("changed", v)}
                      rows={plan.changed.map((c) => ({ key: c.path, main: song(c.title, c.artist), sub: c.path }))}
                    />
                    <GroupBlock
                      title={tr("{n} 首挪了位置", { n: plan.moved.length })}
                      note={tr("改到新路径")}
                      on={review.on.moved}
                      onToggle={(v) => setOn("moved", v)}
                      rows={plan.moved.map((m) => ({ key: m.songId, main: song(m.title, m.artist), sub: `${m.to}（${m.reason}）` }))}
                    />
                    <GroupBlock
                      title={tr("{n} 首回来了", { n: plan.returned.length })}
                      note={tr("移出过，文件又在了：恢复")}
                      on={review.on.returned}
                      onToggle={(v) => setOn("returned", v)}
                      rows={plan.returned.map((r) => ({ key: r.songId, main: song(r.title, r.artist), sub: r.path }))}
                    />
                    <GroupBlock
                      title={tr("{n} 首不见了", { n: plan.gone.length })}
                      note={tr("移出语料库，可以恢复")}
                      on={review.on.gone}
                      onToggle={(v) => setOn("gone", v)}
                      rows={plan.gone.map((g) => ({ key: g.songId, main: song(g.title, g.artist), sub: g.path }))}
                    />
                    <div className="toolbar">
                      {pending > 0 && (
                        <CommandButton icon="check" label={busy === "applying" ? tr("正在写…") : tr("确认这 {n} 项", { n: pending })} primary disabled={busy !== ""} onClick={() => void apply()} />
                      )}
                      <button className="link-btn" onClick={() => setReview(null)}>{pending > 0 ? tr("取消") : tr("关闭")}</button>
                      <span className="muted small">
                        {plan.changed.length + plan.moved.length + plan.returned.length + plan.gone.length === 0 && plan.newFiles.length === 0
                          ? tr("{n} 首都没变，没有要做的", { n: plan.unchanged })
                          : tr("另有 {n} 首没变", { n: plan.unchanged })}
                      </span>
                    </div>
                  </div>
                )}
              </li>
            ))}
          </ul>
          <div className="toolbar">
            <CommandButton icon="add" label={tr("添加文件夹…")} disabled={busy !== ""} onClick={() => void pickFolder()} title={tr("也可以把文件夹拖进窗口")} />
          </div>
        </>
      )}

      {note && <p className="maintain-note">{note}</p>}

      {removed.length > 0 && (
        <div className="removed-songs">
          <div className="section-head">
            <h2>{tr("已移出的歌")}</h2>
            <span className="muted small">{tr("不算进语料、不出现在曲库里，数据都还在，随时可以恢复")}</span>
          </div>
          <ul className="sync-rows">
            {removed.map((s) => (
              <li key={s.id}>
                <label className="check">
                  <input
                    type="checkbox"
                    checked={chosenRemoved.has(s.id)}
                    onChange={(e) => setChosenRemoved((prev) => {
                      const next = new Set(prev);
                      if (e.target.checked) next.add(s.id);
                      else next.delete(s.id);
                      return next;
                    })}
                  />
                  <span>{song(s.title, s.artist)}</span>
                </label>
                <span className="muted small" title={s.audioPath}>
                  {s.audioPresent ? s.audioPath : tr("音频文件找不到了")}
                </span>
              </li>
            ))}
          </ul>
          <div className="toolbar">
            <CommandButton
              icon="refresh"
              label={chosenRemoved.size > 0 ? tr("恢复选中的 {n} 首", { n: chosenRemoved.size }) : tr("全部恢复")}
              disabled={busy !== ""}
              onClick={() => void restore(chosenRemoved.size > 0 ? [...chosenRemoved] : removed.map((s) => s.id))}
            />
            {chosenRemoved.size > 0 && !purging && (
              <button className="link-btn danger" disabled={busy !== ""} onClick={() => setPurging(true)}>{tr("彻底删除选中的…")}</button>
            )}
          </div>
          {purging && chosenRemoved.size > 0 && (
            <div className="inline-confirm">
              <span>{tr("彻底删除这 {n} 首：歌词、分词、收听记录都删掉，不能恢复（分词校正保留，音频文件不动）。", { n: chosenRemoved.size })}</span>
              <div className="toolbar">
                <CommandButton icon="discard" label={tr("彻底删除")} disabled={busy !== ""} onClick={() => void purge([...chosenRemoved])} />
                <button className="link-btn" onClick={() => setPurging(false)}>{tr("取消")}</button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
