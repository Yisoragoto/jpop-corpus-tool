import { Icon } from "./Icon";
/**
 * 有新版本时的弹窗：写清楚是哪一版、什么时候发的、改了什么，由人决定。
 *
 * 原来启动时查到新版，开着「下载后自动安装」就直接下载、直接装——装更新会关掉应用，
 * 正在听歌、正在制卡的人一下子就被打断。现在一律先弹这个窗：
 *
 * - **立即更新**：下载、校验 SHA-256，启动安装程序覆盖安装（`/P /UPDATE /R`：不问问题、不先卸载，
 *   装完自动重新打开）。下载中可以看进度；
 * - **稍后**：关掉，下次启动再提醒；
 * - **跳过这个版本**：这一版不再提醒，再出新版照样提醒（记在设置 `skippedUpdateVersion`）。
 *
 * 设置页「更新到 x」那个按钮也走这个窗，两处行为一样。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../events";
import { useModalFocus } from "../modalFocus";

import { api, type ReleaseInfo, type UpdateProgress } from "../api";
import { tr } from "../i18n";

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
      setError(tr("这次发布没有可直接安装的安装包，请到发布页手动下载。"));
      return;
    }
    setPhase("downloading");
    setError("");
    setProgress(null);
    try {
      await api.updateDownload(release.buildId ?? release.version);
      setPhase("installing");
      await api.updateInstall();
    } catch (err) {
      setPhase("failed");
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [release]);

  const skip = useCallback(() => {
    void api.updateSkip(release.buildId ? "debug" : "stable", release.buildId ?? release.version)
      .then(onClose).catch((err: unknown) => { setError(String(err)); setPhase("failed"); });
  }, [release, onClose]);

  // 焦点关在弹窗里，关掉后还给原来的控件
  const dialogRef = useRef<HTMLDivElement>(null);
  useModalFocus(dialogRef);

  const pct = progress && progress.total > 0 ? Math.min(100, Math.round((progress.received / progress.total) * 100)) : null;
  const notes = release.notes.trim();

  return (
    <div className="palette-backdrop update-backdrop" onMouseDown={busy ? undefined : onClose}>
      <div
        className="update-dialog"
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label={tr("新版本 {version}", { version: release.version })}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="update-head">
          <span className="update-badge" aria-hidden="true">
            <Icon name="import" />
          </span>
          <div>
            <h2>{tr("有新版本 {version}", { version: release.version })}</h2>
            {release.buildId && <p>{tr("构建 {buildId}", { buildId: release.buildId })}</p>}
            <p>{tr("当前 {current}", { current })}{release.publishedAt && tr(" · 发布于 {publishedAt}", { publishedAt: new Date(release.publishedAt).toLocaleDateString() })}
              {release.installer && tr(" · 安装包 {size}", { size: formatSize(release.installer.size) })}
            </p>
          </div>
        </header>

        <div className="update-notes">
          {notes === "" ? <p className="muted">{tr("这次发布没有写说明。")}</p> : <pre>{notes}</pre>}
        </div>

        {phase !== "ask" && (
          <div className={`update-status ${phase}`} role="status">
            {phase === "downloading" && (
              <>
                <span>{pct === null ? tr("正在下载…") : tr("正在下载 {pct}%", { pct })}</span>
                <span className="update-bar" role="progressbar" aria-label={tr("正在下载…")} aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct ?? undefined}>
                  <i style={{ width: `${pct ?? 6}%` }} className={pct === null ? "indeterminate" : ""} />
                </span>
              </>
            )}
            {phase === "installing" && <span>{tr("校验通过，正在安装。应用会关闭一下，装好后自动重新打开…")}</span>}
            {phase === "failed" && <span>{error}</span>}
          </div>
        )}

        <footer className="update-foot">
          {offerSkip && (
            <button className="link-btn update-skip" onClick={skip} disabled={busy} title={tr("这一版不再提醒，出了更新的版本照样提醒")}>{tr("跳过这个版本")}</button>
          )}
          <span className="update-spacer" />
          <button className="btn" onClick={onClose} disabled={busy}>{tr("稍后")}</button>
          <button className="btn primary" onClick={() => void install()} disabled={busy} autoFocus>
            {phase === "failed" ? tr("重试") : tr("立即更新")}
          </button>
        </footer>
        <p className="update-fine">{tr("下载、校验后直接覆盖安装，不用先卸载；装好自动重新打开。")}</p>
      </div>
    </div>
  );
}

function formatSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "";
  return bytes >= 1024 * 1024 ? `${(bytes / 1024 / 1024).toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
}
