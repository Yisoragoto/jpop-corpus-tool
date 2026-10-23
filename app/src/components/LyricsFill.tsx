/**
 * 给一首歌补歌词：在线搜，或者自己选一份文件。
 *
 * 为什么要有这个入口：导入只认音频旁边的同名 .lrc，从音乐 App 的下载目录
 * 拷进来的歌往往只有音频，导进来就是一行歌词都没有。设置页里有「全库补齐」，
 * 这里是「就这一首」——挑不出来的那几首总要有地方人工接手。
 *
 * 自动补齐只对没有歌词的歌开放；已有歌词可展开逐字搜索，预览后明确采用，
 * 也可导入文件替换。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { api, LYRICS_SOURCE_LABELS, type AttachedLyrics } from "../api";
import { tr } from "../i18n";
import { TimedLyricsPanel } from "./TimedLyricsPanel";

interface Props {
  songId: string;
  /** 这首歌现在有没有歌词。决定是否显示自动补齐。 */
  hasLyrics: boolean;
  /** 挂上了，外面去重新拉歌词 */
  onAttached: (result: AttachedLyrics) => void;
  onError: (message: string) => void;
}

export function LyricsFill(props: Props) {
  return <LyricsFillForSong key={props.songId} {...props} />;
}

// TimedLyricsPanel 的 onApplied 已触发 onAttached，onChanged 不再重复刷新。
const unchanged = () => {};

function LyricsFillForSong({ songId, hasLyrics, onAttached, onError }: Props) {
  const [busy, setBusy] = useState<"" | "online" | "file">("");
  const [note, setNote] = useState("");
  const [timedOpen, setTimedOpen] = useState(false);
  const [timedMounted, setTimedMounted] = useState(false);
  const pending = useRef(false);
  const alive = useRef(false);
  const callbacks = useRef({ onAttached, onError });
  callbacks.current = { onAttached, onError };
  useEffect(() => {
    alive.current = true;
    return () => { alive.current = false; };
  }, []);

  const fillOnline = useCallback(async () => {
    if (pending.current || hasLyrics) return;
    pending.current = true;
    setBusy("online");
    setNote("");
    try {
      const result = await api.lyricsFillOne(songId);
      if (!alive.current) return;
      if (result === null) {
        // 挑不出来不是错误：同名曲太多，塞错一首的歌词比没有歌词糟得多
        setNote(tr("在线没找到对得上的（曲名、歌手、时长都要像）。可以自己导入一份。"));
        return;
      }
      setNote(describe(result));
      callbacks.current.onAttached(result);
    } catch (err) {
      if (alive.current) callbacks.current.onError(err instanceof Error ? err.message : String(err));
    } finally {
      if (alive.current) { pending.current = false; setBusy(""); }
    }
  }, [songId, hasLyrics]);

  const importFile = useCallback(async () => {
    if (pending.current) return;
    pending.current = true;
    setBusy("file");
    try {
      const picked = await openDialog({
        multiple: false,
        title: tr("选择歌词文件"),
        filters: [{ name: tr("歌词"), extensions: ["lrc", "txt", "yrc", "qrc", "krc", "ttml"] }],
      });
      if (!alive.current || typeof picked !== "string") return;
      setNote("");
      const result = await api.lyricsImportFile(songId, picked);
      if (!alive.current) return;
      setNote(describe(result));
      callbacks.current.onAttached(result);
    } catch (err) {
      if (alive.current) callbacks.current.onError(err instanceof Error ? err.message : String(err));
    } finally {
      if (alive.current) { pending.current = false; setBusy(""); }
    }
  }, [songId]);

  return (
    <div className="lyrics-fill">
      {!hasLyrics && (
        <button className="link-btn" onClick={() => void fillOnline()} disabled={busy !== "" || timedOpen}>
          {busy === "online" ? tr("正在搜…") : tr("在线补齐")}
        </button>
      )}
      <button className="link-btn" onClick={() => void importFile()} disabled={busy !== "" || timedOpen}>
        {busy === "file" ? tr("正在导入…") : hasLyrics ? tr("换一份歌词…") : tr("导入歌词文件…")}
      </button>
      {note !== "" && <span className="muted small">{note}</span>}
      <details className="lyrics-timed-entry" onToggle={(event) => {
        const open = event.currentTarget.open;
        setTimedOpen(open);
        if (open) setTimedMounted(true);
      }}>
        <summary onClick={(event) => { if (busy !== "") event.preventDefault(); }}>{tr("搜索逐字歌词…")}</summary>
        {timedMounted && <TimedLyricsPanel songId={songId} onChanged={unchanged} onError={onError} onApplied={(result) => {
          setNote(describe(result));
          callbacks.current.onAttached(result);
        }} />}
      </details>
    </div>
  );
}

function describe(result: AttachedLyrics): string {
  const parts = [
    tr("{LYRICS_SOURCE_LABELS}：{lyricLines} 行", { LYRICS_SOURCE_LABELS: LYRICS_SOURCE_LABELS[result.source], lyricLines: result.lyricLines }),
    result.tokens > 0 ? tr("{tokens} 词", { tokens: result.tokens }) : tr("没分词（缺 Sudachi 词典）"),
  ];
  if (result.credits > 0) parts.push(tr("{credits} 条署名", { credits: result.credits }));
  if (result.correctionsRestored > 0) parts.push(tr("恢复了 {correctionsRestored} 条分词校正", { correctionsRestored: result.correctionsRestored }));
  if (result.matched !== "") parts.push(`← ${result.matched}`);
  return parts.join(tr("，"));
}
