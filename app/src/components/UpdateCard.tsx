/**
 * 设置 → 系统 → 更新。
 *
 * 版本发在仓库的 Releases 里，这里就从那儿问：比版本、下安装包、校验、装上。
 * 下载地址和校验值都由后端再核一遍（见 `jp-app/src/update.rs`），前端只负责按钮和进度。
 *
 * **装更新会关掉应用**（NSIS 要替换正在运行的 exe），所以每个会触发安装的地方都写明这一点，
 * 「下载后自动安装」默认关着。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { api, type ReleaseInfo, type UpdateProgress, type UpdateStatus } from "../api";
import { CommandButton } from "./CommandButton";
import { SettingCard, SettingNote, Switch } from "./SettingCard";
import { updateSettings, useAppSettings } from "../settings";

interface Props {
  icons: { update: React.ReactNode; clock: React.ReactNode; install: React.ReactNode };
  onError: (message: string) => void;
}

type Phase = "" | "checking" | "downloading" | "installing";

export function UpdateCard({ icons, onError }: Props) {
  const settings = useAppSettings();
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [phase, setPhase] = useState<Phase>("");
  const [note, setNote] = useState("");
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [changelog, setChangelog] = useState<ReleaseInfo[] | null>(null);
  const [showLog, setShowLog] = useState(false);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    const off = listen<UpdateProgress>("update://progress", (event) => setProgress(event.payload));
    return () => {
      alive.current = false;
      void off.then((f) => f());
    };
  }, []);

  // 进来先显示当前版本（不联网）：检查失败时用户至少知道自己在用哪一版
  useEffect(() => {
    void api
      .updateCheck()
      .then((s) => alive.current && setStatus(s))
      .catch(() => undefined);
  }, []);

  const check = useCallback(async () => {
    setPhase("checking");
    setNote("");
    try {
      const next = await api.updateCheck();
      setStatus(next);
      setNote(
        next.updateAvailable
          ? `有新版本 ${next.latest?.version ?? ""}`
          : `已经是最新版（${next.current}）`,
      );
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setPhase("");
    }
  }, [onError]);

  const install = useCallback(async () => {
    const asset = status?.latest?.installer;
    if (!asset) {
      setNote("这次发布没有可直接安装的安装包，请到发布页手动下载。");
      return;
    }
    setPhase("downloading");
    setNote("");
    setProgress(null);
    try {
      const path = await api.updateDownload(asset);
      setPhase("installing");
      setNote("校验通过，正在启动安装程序…应用会关闭。");
      await api.updateInstall(path);
    } catch (err) {
      setPhase("");
      onError(err instanceof Error ? err.message : String(err));
    }
  }, [status, onError]);

  const openLog = useCallback(async () => {
    setShowLog((v) => !v);
    if (changelog !== null) return;
    try {
      setChangelog(await api.updateChangelog(10));
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  }, [changelog, onError]);

  const busy = phase !== "";
  const pct =
    progress && progress.total > 0 ? Math.min(100, Math.round((progress.received / progress.total) * 100)) : null;

  return (
    <>
      <SettingCard
        icon={icons.update}
        title="检查更新"
        description={
          status === null
            ? "从本仓库的 Releases 取版本；只在你点的时候联网。"
            : status.updateAvailable
              ? `当前 ${status.current}，有新版本 ${status.latest?.version}（${formatDate(status.latest?.publishedAt)}）`
              : `当前 ${status.current}，已是最新`
        }
      >
        {status?.updateAvailable && (
          <CommandButton
            icon="import"
            label={
              phase === "downloading"
                ? pct === null
                  ? "下载中…"
                  : `下载中 ${pct}%`
                : phase === "installing"
                  ? "正在安装…"
                  : `更新到 ${status.latest?.version}`
            }
            onClick={() => void install()}
            disabled={busy}
            primary
            title="下载安装包、校验 SHA-256、启动安装程序。应用会关闭"
          />
        )}
        <CommandButton
          icon="refresh"
          label={phase === "checking" ? "检查中…" : "检查更新"}
          onClick={() => void check()}
          disabled={busy}
        />
      </SettingCard>
      {note !== "" && <SettingNote>{note}</SettingNote>}

      <SettingCard
        icon={icons.clock}
        title="启动时检查更新"
        description="只问一次版本号，不会自动下载。"
      >
        <Switch
          checked={settings.autoCheckUpdates}
          onChange={(v) => updateSettings({ autoCheckUpdates: v })}
          label="启动时检查更新"
        />
      </SettingCard>

      <SettingCard
        icon={icons.install}
        title="下载后自动安装"
        description="开了之后，启动时检查到新版本就直接下载并安装——安装会关闭应用。关着就只提示，什么时候更新由你决定。"
      >
        <Switch
          checked={settings.autoInstallUpdates}
          onChange={(v) => updateSettings({ autoInstallUpdates: v })}
          label="下载后自动安装"
          disabled={!settings.autoCheckUpdates}
        />
      </SettingCard>

      <SettingCard
        icon={icons.clock}
        title="查看更新日志"
        description="最近几次发布改了什么"
        onClick={() => void openLog()}
      >
        <span className="setting-chevron" aria-hidden="true">
          {showLog ? "⌄" : "›"}
        </span>
      </SettingCard>
      {showLog && (
        <div className="changelog">
          {changelog === null && <p className="muted small">读取中…</p>}
          {changelog?.map((release) => (
            <article key={release.tag}>
              <h4>
                {release.name || release.version}
                <span className="muted small">{formatDate(release.publishedAt)}</span>
              </h4>
              <pre>{release.notes.trim() || "（这次发布没有写说明）"}</pre>
            </article>
          ))}
          {changelog?.length === 0 && <p className="muted small">还没有发布记录。</p>}
        </div>
      )}
    </>
  );
}

/** 2026-10-01T16:33:42Z → 2026-10-02（本地时区） */
function formatDate(iso: string | undefined): string {
  if (!iso) return "";
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? "" : date.toLocaleDateString();
}
