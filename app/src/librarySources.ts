/**
 * 曲库 › 音乐文件夹。大部分命令在 `rust/crates/jp-api/src/sources.rs`（类型是生成的）；
 * 建议文件夹和移除文件夹要用到项目目录、封面和变调缓存，留在 `jp-app`，类型在这里手写。
 */

import { call, command } from "./api";
import type { ApplyArgs, RemovedSong, SourcePlanView, SourceView } from "./generated/jp-api";

export type { RemovedSong, SourcePlanView, SourceView };

/** 和 `jp-library/src/sources.rs` 的 `Suggestions` 对应 */
export interface SourceSuggestions {
  folders: { path: string; recursive: boolean; songs: number }[];
  /** 不在任何建议文件夹里的歌 */
  looseSongs: number;
}

/** 和 `jp-library/src/sources.rs` 的 `Removed` 对应 */
export interface SourceRemoved {
  movedOut: number;
  deleted: number;
  sources: SourceView[];
}

export const sourcesApi = {
  list: () => command("library_sources"),
  add: (folders: { path: string; recursive: boolean }[]) => command("library_sources_add", { folders }),
  setEnabled: (id: number, enabled: boolean) => command("library_source_set_enabled", { id, enabled }),
  plan: (id: number) => command("library_source_plan", { id }),
  apply: (args: ApplyArgs) => command("library_source_apply", args),
  removedSongs: () => command("library_removed_songs"),
  restore: (ids: string[]) => command("library_restore_songs", { ids }),
  suggestions: () => call<SourceSuggestions>("library_source_suggestions"),
  remove: (id: number, mode: "moveOut" | "delete") => call<SourceRemoved>("library_source_remove", { id, mode }),
};
