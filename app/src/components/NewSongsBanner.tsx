import { Icon } from "./Icon";
/**
 * 「语料库的文件夹里有新歌」那条横幅。启动时后端查到了才出现（见 `App.tsx`）。
 *
 * 一条横幅走完整件事：说清是哪几首 → 导入 → 说清归到了谁名下 → 缺歌词的给一个补齐的按钮。
 * 导入走的是导入页那条路（`scan_files` → `run_import`），没有第二套写库的逻辑；
 * 这里只是替用户省掉「去导入页、重选一遍文件夹」那几步。
 *
 * **「导入」不是每次都给**：有读不出标签的、歌手靠文件夹名猜的、或者一下子一大批，
 * 就只给「查看」——进导入页看完计划再导（判据在 `newSongs.ts` 的 `reviewReason`）。
 * 疑似重复的从来不算新歌，不会出现在这里。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "../events";

import { api, type ImportProgress, type LyricsProgress, type NewAudio } from "../api";
import { messageOf } from "../errors";
import {
  describeNewSongs,
  foldersOf,
  newItems,
  reviewReason,
  summarizeImport,
  type ImportOutcomeSummary,
} from "../newSongs";
import { tr } from "../i18n";

interface Props {
  found: NewAudio;
  /** 导进去了（或者补上歌词了），外面重新拉曲库和总览 */
  onImported: () => void;
  /** 带着这些文件去导入页复核 */
  onReview: (paths: string[]) => void;
  /** 这几个文件以后别再提 */
  onIgnore: (paths: string[]) => void;
  onDismiss: () => void;
  onError: (message: string) => void;
}

type Stage =
  | { kind: "found" }
  | { kind: "importing"; progress: ImportProgress | null }
  | { kind: "done"; summary: ImportOutcomeSummary; lyrics: "" | "running" | string };

export function NewSongsBanner({ found, onImported, onReview, onIgnore, onDismiss, onError }: Props) {
  const items = useMemo(() => newItems(found.scan), [found]);
  const paths = useMemo(() => items.map((item) => item.path), [items]);
  const review = useMemo(() => reviewReason(items), [items]);
  const folders = useMemo(() => foldersOf(found, items), [found, items]);
  const [stage, setStage] = useState<Stage>({ kind: "found" });
  const alive = useRef(true);
  /** 这条横幅自己发起的补歌词还在跑。设置页也可能在补全库的，那边的进度不归这里管 */
  const fillingLyrics = useRef(false);

  useEffect(() => {
    alive.current = true;
    const offImport = listen<ImportProgress>("import://progress", (event) => {
      if (!alive.current || event.payload.finished) return;
      setStage((s) => (s.kind === "importing" ? { kind: "importing", progress: event.payload } : s));
    });
    const offLyrics = listen<LyricsProgress>("lyrics://progress", (event) => {
      if (!alive.current || !fillingLyrics.current || !event.payload.finished) return;
      fillingLyrics.current = false;
      const p = event.payload;
      const note =
        p.error !== ""
          ? tr("补歌词中断：{error}", { error: p.error })
          : tr("补上 {filled} 首", { filled: p.filled }) +
            (p.notFound > 0 ? tr("，{notFound} 首没找到对得上的（可以在曲库里选中它，自己导入一份）", { notFound: p.notFound }) : "") +
            (p.failed > 0 ? tr("，{failed} 首失败", { failed: p.failed }) : "");
      setStage((s) => (s.kind === "done" ? { ...s, lyrics: note } : s));
      onImported();
    });
    return () => {
      alive.current = false;
      void offImport.then((f) => f());
      void offLyrics.then((f) => f());
    };
  }, [onImported]);

  const doImport = useCallback(async () => {
    setStage({ kind: "importing", progress: null });
    try {
      // 重新扫一遍这几个文件：横幅可能已经挂了一阵子，以现在的盘面和现在的库为准
      const scan = await api.scanFiles(paths);
      const planned = newItems(scan);
      if (planned.length === 0) {
        await api.cancelImport().catch(() => undefined);
        onError(tr("这几个文件现在已经不算新歌了（可能刚在导入页导过），没有再导一遍"));
        onDismiss();
        return;
      }
      const report = await api.runImport(true);
      if (report.csv !== undefined && !["updated", "noFile", "unchanged"].includes(report.csv)) onError(report.csv);
      onImported();
      if (alive.current) setStage({ kind: "done", summary: summarizeImport(report, planned), lyrics: "" });
    } catch (err) {
      onError(messageOf(err));
      if (alive.current) setStage({ kind: "found" });
    }
  }, [paths, onImported, onDismiss, onError]);

  const fillLyrics = useCallback(
    async (songIds: string[]) => {
      fillingLyrics.current = true;
      setStage((s) => (s.kind === "done" ? { ...s, lyrics: "running" } : s));
      try {
        await api.lyricsFillStart(songIds);
      } catch (err) {
        fillingLyrics.current = false;
        setStage((s) => (s.kind === "done" ? { ...s, lyrics: "" } : s));
        onError(messageOf(err));
      }
    },
    [onError],
  );

  if (items.length === 0) return null;

  if (stage.kind === "importing") {
    const p = stage.progress;
    return (
      <div className="banner new-songs">
        <span className="new-songs-text">
          {p === null ? tr("正在导入…") : tr("正在导入 {done}/{total}：{title}", { done: p.done, total: p.total, title: p.title })}
        </span>
      </div>
    );
  }

  if (stage.kind === "done") {
    const { summary, lyrics } = stage;
    const missing = summary.withoutLyrics.length;
    return (
      <div className="banner new-songs">
        <span className="new-songs-text" title={summary.failed.map((f) => `${f.title}${tr("：")}${f.error}`).join("\n")}>
          <strong>{tr("已导入 {imported} 首", { imported: summary.imported })}</strong>
          {summary.artists.length > 0 && tr("，归在 {artists} 名下", { artists: summary.artists.join(tr("、")) })}
          {summary.failed.length > 0 && tr("；{length} 首没导进去（{error}）", { length: summary.failed.length, error: summary.failed[0]?.error })}
          {lyrics !== "" && lyrics !== "running" ? `${tr("。")}${lyrics}` : missing > 0 ? tr("。其中 {missing} 首还没有歌词", { missing }) : tr("。")}
        </span>
        {missing > 0 && lyrics === "" && (
          <button
            className="new-songs-go"
            onClick={() => void fillLyrics(summary.withoutLyrics)}
            title={tr("先看音频旁边有没有同名 .lrc，没有再上网搜（网易云）。挑不准的宁可留空")}
          >{tr("在线补齐歌词")}</button>
        )}
        {lyrics === "running" && <span className="muted small">{tr("正在补歌词…")}</span>}
        {lyrics !== "running" && (
          <button className="new-songs-close" onClick={onDismiss} title={tr("关闭")} aria-label={tr("关闭")}>
            <Icon name="close" />
          </button>
        )}
      </div>
    );
  }

  const detail = [
    ...items.map((item) => `${item.title}${item.artist.trim() === "" ? "" : ` — ${item.artist}`}`),
    "",
    ...folders.map((folder) => tr("在 {folder}", { folder })),
  ].join("\n");
  return (
    <div className="banner new-songs">
      <span className="new-songs-text" title={detail}>
        <strong>{tr("检测到 {length} 首新歌", { length: items.length })}</strong>{tr("：")}{describeNewSongs(items)}
        {folders.length > 0 && (
          <span className="muted small">{tr("{v}· 在 {folders}", { v: " ", folders: folders[0] })}{folders.length > 1 ? tr(" 等 {length} 个文件夹", { length: folders.length }) : ""}
          </span>
        )}
        {review !== null && <span className="muted small"> · {review}</span>}
      </span>
      {review === null ? (
        <>
          <button className="new-songs-go" onClick={() => void doImport()} title={tr("导入并分词，归到各自的歌手和专辑名下")}>{tr("导入 {length} 首", { length: items.length })}</button>
          <button className="link-btn" onClick={() => onReview(paths)} title={tr("去导入页看完整的计划再决定")}>{tr("先看看")}</button>
        </>
      ) : (
        <button className="new-songs-go" onClick={() => onReview(paths)} title={tr("去导入页看完整的计划再导")}>{tr("查看并导入")}</button>
      )}
      <button className="link-btn" onClick={() => onIgnore(paths)} title={tr("这几个文件以后不再提示。可以在导入页恢复")}>{tr("不再提示")}</button>
      <button className="new-songs-close" onClick={onDismiss} title={tr("这次先不管，下次启动再提")} aria-label={tr("关闭")}>
        <Icon name="close" />
      </button>
    </div>
  );
}
