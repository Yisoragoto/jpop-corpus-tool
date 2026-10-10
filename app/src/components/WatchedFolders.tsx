/**
 * 导入页上的「语料库文件夹」：启动时去哪些文件夹找新歌、现在里面有没有还没导入的。
 *
 * 看哪些文件夹是后端从库里的歌所在的位置推出来的，用户没有亲手配过——所以**必须摆出来**：
 * 「我放进去的歌为什么没被发现」「为什么老提示那个下载目录」都只能在这里找到答案，
 * 也只能在这里改（不再检查某个文件夹、把忽略过的恢复回来）。
 */

import { useCallback, useEffect, useState } from "react";

import { api, type NewAudio, type WatchedFolder } from "../api";
import { messageOf } from "../errors";
import { NO_IGNORES, pathKey, setIgnores, useNewSongIgnores, withIgnored, withoutIgnoredFolder } from "../newSongs";
import { CommandButton } from "./CommandButton";

interface Props {
  /** 把这些文件交给下面的扫描 → 复核 → 导入 */
  onReview: (paths: string[]) => void;
  onError: (message: string) => void;
  disabled: boolean;
  /** 变了就重新查一遍（导入完成之后） */
  refreshKey: number;
}

/** 为什么看这个文件夹、看多深。用户没有亲手配过这些，所以每一行都要说得出理由 */
function why(folder: WatchedFolder): string {
  switch (folder.reason) {
    case "library":
      return folder.depth <= 1 ? "语料库目录 · 只看直接放在这里的文件" : "语料库目录里放音频的地方";
    case "libraryAudio":
      return `库里有 ${folder.songs} 首歌在这里 · 只看这一层`;
    case "parent":
      return `库里有 ${folder.songs} 首歌在这下面 · 连子文件夹一起看`;
  }
}

export function WatchedFolders({ onReview, onError, disabled, refreshKey }: Props) {
  const ignores = useNewSongIgnores();
  const [found, setFound] = useState<NewAudio | null>(null);
  const [busy, setBusy] = useState(false);

  const check = useCallback(async () => {
    setBusy(true);
    try {
      setFound(await api.libraryNewAudio(ignores.folders, ignores.files));
    } catch (err) {
      onError(`查不了语料库文件夹：${messageOf(err)}`);
    } finally {
      setBusy(false);
    }
  }, [ignores, onError]);

  // 进页面、改了忽略清单、导完一批之后各查一次
  useEffect(() => {
    void check();
  }, [check, refreshKey]);

  const unknown = found?.scan.items ?? [];
  const fresh = found?.scan.summary.new ?? 0;
  const others = unknown.length - fresh;

  return (
    <div className="card">
      <div className="card-head">
        <h2>语料库文件夹</h2>
        <span className="muted small">启动时会看这些地方有没有新歌</span>
      </div>
      {found !== null && (
        <ul className="watched-folders">
          {found.folders.map((folder) => (
            <li key={folder.path}>
              <span className="watched-path" title={folder.path}>
                {folder.path}
              </span>
              <span className="muted small">
                {why(folder)}
                {folder.unknown > 0 && ` · ${folder.unknown} 个文件不在库里`}
                {folder.new > 0 && `（${folder.new} 首新歌）`}
              </span>
              <button
                className="link-btn"
                onClick={() => setIgnores(withIgnored(ignores, { folders: [folder.path] }))}
                title="以后不看这个文件夹和它下面的"
              >
                不再检查
              </button>
            </li>
          ))}
          {ignores.folders.map((folder) => (
            <li key={`off-${pathKey(folder)}`} className="off">
              <span className="watched-path" title={folder}>
                {folder}
              </span>
              <span className="muted small">已设为不检查</span>
              <button className="link-btn" onClick={() => setIgnores(withoutIgnoredFolder(ignores, folder))}>
                恢复
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="toolbar">
        <CommandButton
          icon="refresh"
          label={busy ? "正在查…" : "重新检查"}
          onClick={() => void check()}
          disabled={busy || disabled}
          title="只读：看看这些文件夹里有没有还没导入的音频"
        />
        {unknown.length > 0 && (
          <CommandButton
            icon="scan"
            label={fresh > 0 ? `查看这 ${fresh} 首新歌` : `查看这 ${unknown.length} 个文件`}
            onClick={() => onReview(unknown.map((item) => item.path))}
            disabled={busy || disabled}
            primary={fresh > 0}
            title="把它们放进下面的计划里，看完再决定导不导"
          />
        )}
        <span className="muted small">
          {found === null
            ? ""
            : unknown.length === 0
              ? "这些文件夹里的音频都已经在库里了。"
              : fresh === 0
                ? `有 ${others} 个文件不在库里，但都不算新歌（疑似重复、格式放不了或者读不出曲名）。`
                : others > 0
                  ? `另有 ${others} 个不算新歌（疑似重复、格式放不了或者读不出曲名）。`
                  : ""}
        </span>
      </div>
      {ignores.files.length > 0 && (
        <p className="muted small">
          有 {ignores.files.length} 个文件设成了「不再提示」。{" "}
          <button className="link-btn" onClick={() => setIgnores({ ...ignores, files: NO_IGNORES.files })}>
            恢复提示
          </button>
        </p>
      )}
    </div>
  );
}
