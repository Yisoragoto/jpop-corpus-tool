/**
 * 设置 → 歌词 → 分词词典（Sudachi）。
 *
 * 振假名、导入时的分词、实时分词都要这份 207MB 的 `system.dic`。0.1.x 把它装在
 * 项目目录的 `venv` 里，新装的程序把库建在 `%LOCALAPPDATA%`，那里没有——
 * 于是歌词上面常驻一条「振假名不可用：找不到 Sudachi 词典」。
 *
 * 这张卡只做两件事：照实说现在有没有，以及让用户指一个有词典的目录复制过来。
 * 复制完立刻生效，不用重启。
 */

import { useCallback, useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { api, type TokenizerStatus } from "../api";
import { CommandButton } from "./CommandButton";
import { SettingCard, SettingNote } from "./SettingCard";

interface Props {
  icon: React.ReactNode;
  onError: (message: string) => void;
  /** 装上之后外壳重新自检（歌词页那条红条要消失） */
  onChanged: () => void;
}

const MB = 1024 * 1024;

export function SudachiCard({ icon, onError, onChanged }: Props) {
  const [status, setStatus] = useState<TokenizerStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");

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
      title: "选一个有 Sudachi 词典的目录（0.1.x 的项目目录、它的 venv，或另一个语料库目录）",
    });
    if (typeof picked !== "string") return;
    setBusy(true);
    setNote("正在复制…（207MB，要几秒）");
    try {
      const done = await api.tokenizerInstall(picked);
      setNote(`装好了：${(done.bytes / MB).toFixed(0)} MB → ${done.dir}`);
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
      ? "读取中…"
      : status.ready
        ? `${status.bundled ? "语料库目录自带" : "用的是 0.1.x 的 venv"}：${status.dictPath}（${(status.dictBytes / MB).toFixed(0)} MB）`
        : "没找到。振假名、导入时的分词和实时分词都不可用——歌、歌词、查词、播放照常。";

  return (
    <>
      <SettingCard icon={icon} title="分词词典（Sudachi）" description={description}>
        <CommandButton
          icon="folder"
          label={busy ? "复制中…" : status?.ready === true ? "换一份…" : "导入词典…"}
          onClick={() => void install()}
          disabled={busy}
          {...(status?.ready === true ? {} : { primary: true })}
          title="从别的目录复制一份进当前语料库目录，复制完立刻生效"
        />
      </SettingCard>
      {note !== "" && <SettingNote>{note}</SettingNote>}
      {status !== null && !status.ready && (
        <SettingNote>
          指 0.1.x 的项目目录（里面有 <code>venv</code>）、它的 <code>venv</code> 本身，或者另一个语料库目录的{" "}
          <code>sudachi</code> 文件夹都行。复制过来之后放在 <code>{status.bundledDir}</code>，
          以后换语料库目录也带着走。
        </SettingNote>
      )}
    </>
  );
}
