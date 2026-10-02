/**
 * 「振假名不可用：找不到 Sudachi 词典」那条横幅上的下载按钮。
 *
 * 词典 207MB，没打进安装包（打进去的话每次更新都要重下这 50MB，而词典和版本无关）。
 * 所以**要用振假名的时候再下**：点一下，下 43MB 的压缩包、校验、解压进语料库目录，
 * 下好立刻生效——之前算失败的那首歌会自己重算一遍。
 *
 * 只下这一次：词典跟着语料库目录走，以后换版本、换机器搬目录都不用再下。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { api, DICT_DOWNLOAD_BYTES, type DictProgress } from "../api";
import { retryFurigana } from "../lyricsDisplay";

const MB = 1024 * 1024;

interface Props {
  message: string;
  /** 下好了：外壳重新自检、把这条横幅收起来 */
  onDone: () => void;
  onDismiss: () => void;
}

export function SudachiBanner({ message, onDone, onDismiss }: Props) {
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<DictProgress | null>(null);
  const [failed, setFailed] = useState("");
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    const off = listen<DictProgress>("tokenizer://progress", (event) => {
      if (alive.current) setProgress(event.payload);
    });
    return () => {
      alive.current = false;
      void off.then((f) => f());
    };
  }, []);

  const download = useCallback(async () => {
    setBusy(true);
    setFailed("");
    try {
      await api.tokenizerDownload();
      retryFurigana();
      onDone();
    } catch (err) {
      setFailed(err instanceof Error ? err.message : String(err));
    } finally {
      if (alive.current) setBusy(false);
    }
  }, [onDone]);

  const pct =
    progress !== null && progress.total > 0
      ? Math.min(100, Math.round((progress.received / progress.total) * 100))
      : null;
  const stage =
    progress === null
      ? "准备中…"
      : progress.stage === "downloading"
        ? `下载中 ${pct}%（${(progress.received / MB).toFixed(0)} / ${(progress.total / MB).toFixed(0)} MB）`
        : progress.stage === "extracting"
          ? "解压中…（207 MB，要一会儿）"
          : "装上了";

  return (
    <div className="banner error sudachi-banner">
      <span className="sudachi-text">
        {failed === "" ? message : `下载失败：${failed}`}
        {busy && <span className="muted small"> · {stage}</span>}
      </span>
      <button className="sudachi-get" onClick={() => void download()} disabled={busy}>
        {busy ? "下载中…" : `下载词典（${(DICT_DOWNLOAD_BYTES / MB).toFixed(0)} MB，只下这一次）`}
      </button>
      {!busy && (
        <button className="sudachi-close" onClick={onDismiss} title="先不下" aria-label="关闭">
          ✕
        </button>
      )}
    </div>
  );
}
