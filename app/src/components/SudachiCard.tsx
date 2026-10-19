/**
 * 设置 → 歌词 → 分词词典（Sudachi）。
 *
 * 振假名、导入时的分词、实时分词都要这份 207MB 的 `system.dic`。它没打进安装包
 * （打进去的话每次更新都要重下 50MB，而词典和版本无关），所以**用到的时候再下**：
 * 这里一个按钮，歌词上那条横幅上也有一个。
 *
 * 手上已经有 0.1.x 的 venv 的话，「从目录导入」比下载快得多，所以两条路都留着。
 * 下好 / 导完立刻生效，不用重启。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen } from "../events";

import { api, DICT_DOWNLOAD_BYTES, type DictProgress, type TokenizerStatus } from "../api";
import { backfillNote } from "../backfill";
import { retryFurigana } from "../lyricsDisplay";
import { CommandButton } from "./CommandButton";
import { SettingCard, SettingNote } from "./SettingCard";
import { tr } from "../i18n";

interface Props {
  icon: React.ReactNode;
  onError: (message: string) => void;
  /** 装上之后外壳重新自检（歌词页那条红条要消失） */
  onChanged: () => void;
}

const MB = 1024 * 1024;

const SOURCE_LABELS: Record<string, string> = {
  app: tr("随程序自带"),
  library: tr("语料库目录里的"),
  venv: tr("0.1.x 的 venv"),
};

export function SudachiCard({ icon, onError, onChanged }: Props) {
  const [status, setStatus] = useState<TokenizerStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");
  const [progress, setProgress] = useState<DictProgress | null>(null);
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

  const refresh = useCallback(() => {
    void api
      .tokenizerStatus()
      .then(setStatus)
      .catch(() => undefined);
  }, []);

  useEffect(refresh, [refresh]);

  const install = useCallback(async () => {
    const picked = await openDialog({
      directory: true,
      title: tr("选一个有 Sudachi 词典的目录（0.1.x 的项目目录、它的 venv，或另一个语料库目录）"),
    });
    if (typeof picked !== "string") return;
    setBusy(true);
    setNote(tr("正在复制…（207MB，要几秒）"));
    try {
      const done = await api.tokenizerInstall(picked);
      setNote(
        tr("装好了：{toFixed} MB → {dir}。{tokenized}", { toFixed: (done.bytes / MB).toFixed(0), dir: done.dir, tokenized: backfillNote(done.tokenized, done.tokenizeError) }),
      );
      retryFurigana();
      refresh();
      onChanged();
    } catch (err) {
      setNote("");
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [onChanged, onError, refresh]);

  const description =
    status === null
      ? tr("读取中…")
      : status.ready
        ? `${SOURCE_LABELS[status.source] ?? status.source}${tr("：")}${status.dictPath}${tr("（")}${(status.dictBytes / MB).toFixed(0)} MB${tr("）")}`
        : tr("还没有。振假名、导入时的分词和实时分词要用它——歌、歌词、查词、播放不受影响。下一次就一直在了。");

  const download = useCallback(async () => {
    setBusy(true);
    setNote("");
    setProgress(null);
    try {
      const done = await api.tokenizerDownload();
      setNote(
        tr("装好了：{toFixed} MB → {dir}。{tokenized}", { toFixed: (done.bytes / MB).toFixed(0), dir: done.dir, tokenized: backfillNote(done.tokenized, done.tokenizeError) }),
      );
      retryFurigana();
      refresh();
      onChanged();
    } catch (err) {
      setNote("");
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      if (alive.current) setBusy(false);
    }
  }, [onChanged, onError, refresh]);

  const stage =
    progress === null
      ? tr("准备中…")
      : progress.stage === "downloading"
        ? tr("下载中 {toFixed} / {toFixed2} MB", { toFixed: (progress.received / MB).toFixed(0), toFixed2: (progress.total / MB).toFixed(0) })
        : progress.stage === "extracting"
          ? tr("解压中…（207 MB）")
          : tr("装上了");

  return (
    <>
      <SettingCard icon={icon} title={tr("分词词典（Sudachi）")} description={description}>
        {busy && <span className="muted small">{stage}</span>}
        {status?.ready !== true && (
          <CommandButton
            icon="import"
            label={tr("下载（{toFixed} MB）", { toFixed: (DICT_DOWNLOAD_BYTES / MB).toFixed(0) })}
            onClick={() => void download()}
            disabled={busy}
            primary
            title={tr("从本仓库的发布里下一份，校验后解压进当前语料库目录")}
          />
        )}
        <CommandButton
          icon="folder"
          label={busy ? tr("处理中…") : status?.ready === true ? tr("换一份…") : tr("从目录导入…")}
          onClick={() => void install()}
          disabled={busy}
          title={tr("手上已经有 0.1.x 的 venv 的话，从那儿复制比下载快得多")}
        />
      </SettingCard>
      {note !== "" && <SettingNote>{note}</SettingNote>}
      {status !== null && status.source !== "app" && (
        <SettingNote>{tr("指 0.1.x 的项目目录（里面有")}{" "}<code>venv</code>{tr("）、它的")}{" "}<code>venv</code>{" "}{tr("本身，或者另一个语料库目录的{v}", { v: " " })}<code>sudachi</code>{" "}{tr("文件夹都行。复制过来之后放在")}{" "}<code>{status.bundledDir}</code>{tr("， 跟着语料库目录走；删掉它就回到随程序自带的那一份。")}</SettingNote>
      )}
    </>
  );
}
