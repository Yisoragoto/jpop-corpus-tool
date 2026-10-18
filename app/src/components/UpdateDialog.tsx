/**
 * 有新版本时的弹窗：写清楚是哪一版、什么时候发的、改了什么，由人决定。
 *
 * 原来启动时查到新版，开着「下载后自动安装」就直接下载、直接装——装更新会关掉应用，
 * 正在听歌、正在制卡的人一下子就被打断。现在一律先弹这个窗：
 *
 * - **立即更新**：下载、校验 SHA-256、启动安装程序（应用会关闭）。下载中可以看进度；
 * - **稍后**：关掉，下次启动再提醒；
 * - **跳过这个版本**：这一版不再提醒，再出新版照样提醒（记在设置 `skippedUpdateVersion`）。
 *
 * 设置页「更新到 x」那个按钮也走这个窗，两处行为一样。
 */

import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { api, type ReleaseInfo, type UpdateProgress } from "../api";
import { updateSettings } from "../settings";

type Phase = "ask" | "downloading" | "installing" | "failed";

interface Props {
  current: string;
  release: ReleaseInfo;
  /** 启动时自动弹的才给「跳过这个版本」；设置页里自己点的不需要 */
  offerSkip: boolean;
  onClose: () => void;
}

export function UpdateDialog({ current, release, offerSkip, onClose }: Props) {
  const [phase, setPhase] = useState<Phase>("ask");
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [error, setError] = useState("");
  const busy = phase === "downloading" || phase === "installing";

  useEffect(() => {
    const off = listen<UpdateProgress>("update://progress", (event) => setProgress(event.payload));
    return () => {
      void off.then((f) => f());
    };
  }, []);

  // Esc = 稍后（下载中不让关：关了也停不下后台的下载，只会让人以为取消了）
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onClose]);

  const install = useCallback(async () => {
    if (release.installer === null) {
      setPhase("failed");
      setError("这次发布没有可直接安装的安装包，请到发布页手动下载。");
      return;
    }
    setPhase("downloading");
    setError("");
    setProgress(null);
    try {
      await api.updateDownload(release.version);
      setPhase("installing");
      await api.updateInstall();
    } catch (err) {
      setPhase("failed");
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [release]);

  const skip = useCallback(() => {
    updateSettings({ skippedUpdateVersion: release.version });
    onClose();
  }, [release.version, onClose]);

  const pct = progress && progress.total > 0 ? Math.min(100, Math.round((progress.received / progress.total) * 100)) : null;
  const notes = release.notes.trim();

  return (
    <div className="palette-backdrop update-backdrop" onMouseDown={busy ? undefined : onClose}>
      <div
        className="update-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={`新版本 ${release.version}`}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="update-head">
          <span className="update-badge" aria-hidden="true">
            <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
              <path d="M12 3v11" />
              <path d="M8 10.5l4 4 4-4" />
              <path d="M4 18.5h16" />
            </svg>
          </span>
          <div>
            <h2>有新版本 {release.version}</h2>
            <p>
              当前 {current}
              {release.publishedAt && ` · 发布于 ${new Date(release.publishedAt).toLocaleDateString()}`}
              {release.installer && ` · 安装包 ${formatSize(release.installer.size)}`}
            </p>
          </div>
        </header>

        <div className="update-notes">
          {notes === "" ? <p className="muted">这次发布没有写说明。</p> : <pre>{notes}</pre>}
        </div>

        {phase !== "ask" && (
          <div className={`update-status ${phase}`} role="status">
            {phase === "downloading" && (
              <>
                <span>{pct === null ? "正在下载…" : `正在下载 ${pct}%`}</span>
                <span className="update-bar">
                  <i style={{ width: `${pct ?? 6}%` }} className={pct === null ? "indeterminate" : ""} />
                </span>
              </>
            )}
            {phase === "installing" && <span>校验通过，正在启动安装程序，应用马上会关闭…</span>}
            {phase === "failed" && <span>{error}</span>}
          </div>
        )}

        <footer className="update-foot">
          {offerSkip && (
            <button className="link-btn update-skip" onClick={skip} disabled={busy} title="这一版不再提醒，出了更新的版本照样提醒">
              跳过这个版本
            </button>
          )}
          <span className="update-spacer" />
          <button className="btn" onClick={onClose} disabled={busy}>
            稍后
          </button>
          <button className="btn primary" onClick={() => void install()} disabled={busy} autoFocus>
            {phase === "failed" ? "重试" : "立即更新"}
          </button>
        </footer>
        <p className="update-fine">更新会下载安装包并校验，然后关闭应用完成安装。</p>
      </div>
    </div>
  );
}

function formatSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "";
  return bytes >= 1024 * 1024 ? `${(bytes / 1024 / 1024).toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
}
