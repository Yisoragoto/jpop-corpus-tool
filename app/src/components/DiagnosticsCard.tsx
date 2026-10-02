/**
 * 设置 → 系统 → 导出诊断信息。
 *
 * 出事的时候要问的永远是那几样：哪一版、库在哪、库里有多少、词典有没有、
 * 音频行不行、最近报了什么错。以前只能来回问；现在一段纯文本全在里面。
 *
 * **不联网、不自动上报**：攒出来给你，发不发、发给谁你自己定。
 * 里面不含歌名、歌手、歌词；路径里会带 Windows 用户名（库就在 `C:\Users\…` 下面），
 * 所以卡片上写明白了。
 */

import { useCallback, useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";

import { api } from "../api";
import { CommandButton } from "./CommandButton";
import { SettingCard, SettingNote } from "./SettingCard";

interface Props {
  icon: React.ReactNode;
  onError: (message: string) => void;
}

export function DiagnosticsCard({ icon, onError }: Props) {
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);

  const copy = useCallback(async () => {
    setBusy(true);
    try {
      const text = await api.diagnosticsReport();
      await navigator.clipboard.writeText(text);
      setNote(`复制好了，${text.split("\n").length} 行。粘贴到问题反馈里就行。`);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [onError]);

  const saveToFile = useCallback(async () => {
    try {
      const picked = await saveDialog({
        title: "保存诊断信息",
        defaultPath: "jpop-corpus-diagnostics.txt",
        filters: [{ name: "文本", extensions: ["txt"] }],
      });
      if (typeof picked !== "string") return;
      setBusy(true);
      setNote(`存好了：${await api.diagnosticsSave(picked)}`);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [onError]);

  return (
    <>
      <SettingCard
        icon={icon}
        title="导出诊断信息"
        description="版本、语料库目录、曲目数、分词词典和音频状态、最近 200 行日志。不联网、不自动上报；不含歌名和歌词，但路径里有你的用户名。"
      >
        <CommandButton icon="export" label="复制" onClick={() => void copy()} disabled={busy} />
        <CommandButton icon="folder" label="存成文件…" onClick={() => void saveToFile()} disabled={busy} />
      </SettingCard>
      {note !== "" && <SettingNote>{note}</SettingNote>}
    </>
  );
}
