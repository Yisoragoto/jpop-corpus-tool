/**
 * 设置 → 更新与系统 → 更新。
 *
 * 版本发在仓库的 Releases 里，这里就从那儿问：比版本、下安装包、校验、装上。
 * 下载地址和校验值都由后端再核一遍（见 `jp-app/src/update.rs`），前端只负责按钮和进度。
 *
 * **装更新会关掉应用**（NSIS 要替换正在运行的 exe），所以从不自己装：
 * 「更新到 x」打开的是和启动检查同一个确认窗（`UpdateDialog`），在窗里点了才下载安装。
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { api, type ReleaseInfo, type UpdateStatus } from "../api";
import { CommandButton } from "./CommandButton";
import { SettingCard, SettingNote, Switch } from "./SettingCard";
import { updateSettings, useAppSettings } from "../settings";
import { UpdateDialog } from "./UpdateDialog";
import { tr } from "../i18n";

interface Props {
  icons: { update: React.ReactNode; clock: React.ReactNode; install: React.ReactNode };
  onError: (message: string) => void;
}

export function UpdateCard({ icons, onError }: Props) {
  const settings = useAppSettings();
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [note, setNote] = useState("");
  const [changelog, setChangelog] = useState<ReleaseInfo[] | null>(null);
  const [showLog, setShowLog] = useState(false);
  const [dialog, setDialog] = useState(false);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
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
    setChecking(true);
    setNote("");
    try {
      const next = await api.updateCheck();
      setStatus(next);
      setNote(next.updateAvailable ? tr("有新版本 {v}", { v: next.latest?.version ?? "" }) : tr("已经是最新版（{current}）", { current: next.current }));
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setChecking(false);
    }
  }, [onError]);

  const openLog = useCallback(async () => {
    setShowLog((v) => !v);
    if (changelog !== null) return;
    try {
      setChangelog(await api.updateChangelog(10));
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  }, [changelog, onError]);

  const skipped = settings.skippedUpdateVersion;

  return (
    <>
      <SettingCard
        icon={icons.update}
        title={tr("检查更新")}
        description={
          status === null
            ? tr("从本仓库的 Releases 取版本；只在你点的时候联网。")
            : status.updateAvailable
              ? tr("当前 {current}，有新版本 {version}（{publishedAt}）", { current: status.current, version: status.latest?.version, publishedAt: formatDate(status.latest?.publishedAt) })
              : tr("当前 {current}，已是最新", { current: status.current })
        }
      >
        {status?.updateAvailable && status.latest !== null && (
          <CommandButton
            icon="import"
            label={tr("更新到 {version}…", { version: status.latest.version })}
            onClick={() => setDialog(true)}
            primary
            title={tr("先看这一版改了什么，确认后再下载安装（应用会关闭一下，装好自动重新打开）")}
          />
        )}
        <CommandButton icon="refresh" label={checking ? tr("检查中…") : tr("检查更新")} onClick={() => void check()} disabled={checking} />
      </SettingCard>
      {note !== "" && <SettingNote>{note}</SettingNote>}
      {dialog && status?.latest && (
        <UpdateDialog current={status.current} release={status.latest} offerSkip={false} onClose={() => setDialog(false)} />
      )}

      <SettingCard
        icon={icons.clock}
        title={tr("启动时检查更新")}
        description={tr("只问一次版本号。有新版会弹窗说明改了什么，由你选「立即更新」「稍后」或「跳过这个版本」，不会自己下载安装。")}
      >
        <Switch
          checked={settings.autoCheckUpdates}
          onChange={(v) => updateSettings({ autoCheckUpdates: v })}
          label={tr("启动时检查更新")}
        />
      </SettingCard>

      {skipped !== "" && (
        <SettingCard icon={icons.install} title={tr("跳过的版本")} description={tr("{skipped} 启动时不再提醒；出了更新的版本照样提醒", { skipped })}>
          <button className="link-btn" onClick={() => updateSettings({ skippedUpdateVersion: "" })}>{tr("恢复提醒")}</button>
        </SettingCard>
      )}

      <SettingCard
        icon={icons.clock}
        title={tr("查看更新日志")}
        description={tr("最近几次发布改了什么")}
        onClick={() => void openLog()}
      >
        <span className="setting-chevron" aria-hidden="true">
          {showLog ? "⌄" : "›"}
        </span>
      </SettingCard>
      {showLog && (
        <div className="changelog">
          {changelog === null && <p className="muted small">{tr("读取中…")}</p>}
          {changelog?.map((release) => (
            <article key={release.tag}>
              <h4>
                {release.name || release.version}
                <span className="muted small">{formatDate(release.publishedAt)}</span>
              </h4>
              <pre>{release.notes.trim() || tr("（这次发布没有写说明）")}</pre>
            </article>
          ))}
          {changelog?.length === 0 && <p className="muted small">{tr("还没有发布记录。")}</p>}
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
