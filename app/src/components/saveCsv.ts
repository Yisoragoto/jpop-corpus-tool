import { save } from "@tauri-apps/plugin-dialog";

import { api } from "../api";
import { tr } from "../i18n";

/** 弹「另存为」写一份 CSV。取消了返回 null，出错交给 onError */
export async function saveCsv(csv: string, defaultPath: string, onError: (message: string) => void): Promise<string | null> {
  try {
    const path = await save({
      title: tr("保存 CSV"),
      defaultPath,
      filters: [{ name: tr("CSV 文件"), extensions: ["csv"] }],
    });
    if (!path) return null;
    await api.exportText(path, csv);
    return path;
  } catch (err) {
    onError(err instanceof Error ? err.message : String(err));
    return null;
  }
}
