/**
 * 给一首歌补歌词：在线搜，或者自己选一份文件。
 *
 * 为什么要有这个入口：导入只认音频旁边的同名 .lrc，从音乐 App 的下载目录
 * 拷进来的歌往往只有音频，导进来就是一行歌词都没有。设置页里有「全库补齐」，
 * 这里是「就这一首」——挑不出来的那几首总要有地方人工接手。
 *
 * **在线补齐只对还没有歌词的歌开放**（后端同样会拒）：已经有歌词时按钮是
 * 「换一份歌词…」，必须由用户明确给出文件，不会悄悄用网上搜到的覆盖掉
 * 用户手上那一份。
 */

import { useCallback, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { api, LYRICS_SOURCE_LABELS, type AttachedLyrics } from "../api";

interface Props {
  songId: string;
  /** 这首歌现在有没有歌词。决定显示「在线补齐」还是「换一份」 */
  hasLyrics: boolean;
  /** 挂上了，外面去重新拉歌词 */
  onAttached: (result: AttachedLyrics) => void;
  onError: (message: string) => void;
}

export function LyricsFill({ songId, hasLyrics, onAttached, onError }: Props) {
  const [busy, setBusy] = useState<"" | "online" | "file">("");
  const [note, setNote] = useState("");

  const fillOnline = useCallback(async () => {
    setBusy("online");
    setNote("");
    try {
      const result = await api.lyricsFillOne(songId);
      if (result === null) {
        // 挑不出来不是错误：同名曲太多，塞错一首的歌词比没有歌词糟得多
        setNote("在线没找到对得上的（曲名、歌手、时长都要像）。可以自己导入一份。");
        return;
      }
      setNote(describe(result));
      onAttached(result);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy("");
    }
  }, [songId, onAttached, onError]);

  const importFile = useCallback(async () => {
    try {
      const picked = await openDialog({
        multiple: false,
        title: "选择歌词文件",
        filters: [{ name: "歌词", extensions: ["lrc", "txt"] }],
      });
      if (typeof picked !== "string") return;
      setBusy("file");
      setNote("");
      const result = await api.lyricsImportFile(songId, picked);
      setNote(describe(result));
      onAttached(result);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy("");
    }
  }, [songId, onAttached, onError]);

  return (
    <div className="lyrics-fill">
      {!hasLyrics && (
        <button className="link-btn" onClick={() => void fillOnline()} disabled={busy !== ""}>
          {busy === "online" ? "正在搜…" : "在线补齐"}
        </button>
      )}
      <button className="link-btn" onClick={() => void importFile()} disabled={busy !== ""}>
        {busy === "file" ? "正在导入…" : hasLyrics ? "换一份歌词…" : "导入歌词文件…"}
      </button>
      {note !== "" && <span className="muted small">{note}</span>}
    </div>
  );
}

function describe(result: AttachedLyrics): string {
  const parts = [
    `${LYRICS_SOURCE_LABELS[result.source]}：${result.lyricLines} 行`,
    result.tokens > 0 ? `${result.tokens} 词` : "没分词（缺 Sudachi 词典）",
  ];
  if (result.credits > 0) parts.push(`${result.credits} 条署名`);
  if (result.correctionsRestored > 0) parts.push(`恢复了 ${result.correctionsRestored} 条分词校正`);
  if (result.matched !== "") parts.push(`← ${result.matched}`);
  return parts.join("，");
}
