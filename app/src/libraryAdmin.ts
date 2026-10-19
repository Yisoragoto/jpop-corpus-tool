/**
 * 曲库维护：编辑曲目信息、删除曲目、找回丢了的音频。对应 Python 版的曲目编辑 / 删除和「音声修復」。
 * 库里怎么改见 `jp-import/src/maintain.rs`，盘上的封面、变调缓存、metadata/songs.csv 见 `jp-app/src/library_admin.rs`。
 */

import { call } from "./api";
import { tr } from "./i18n";

export interface SongEdit {
  title: string;
  artist: string;
  year: string;
  album: string;
  genre: string;
}

/** updated：清单改了；noFile：没有 metadata/songs.csv（不建）；unchanged：没有要改的；其余是出错原因（库已经改好了） */
export type CsvSync = string;

export interface EditReport {
  changed: (keyof SongEdit)[];
  performers: number;
  albumId: number | null;
  correctionsRenamed: number;
  albumsRemoved: number;
  peopleRemoved: number;
  csv: CsvSync;
}

export interface DeleteResult {
  title: string;
  artist: string;
  audioPath: string;
  coverPath: string;
  lyricLines: number;
  tokens: number;
  credits: number;
  plays: number;
  correctionsKept: number;
  albumsRemoved: number;
  peopleRemoved: number;
  coverRemoved: boolean;
  pitchCacheRemoved: number;
  csv: CsvSync;
}

export interface MissingAudio {
  songId: string;
  title: string;
  artist: string;
  audioPath: string;
}

export interface RelinkSuggestion {
  songId: string;
  path: string;
  reason: string;
}

export interface RelinkResult {
  songId: string;
  /** 空串表示成功 */
  error: string;
  durationSec: number | null;
}

export interface RelinkBatch {
  results: RelinkResult[];
  csv: CsvSync;
}

/** 和 jp-import 的 AUDIO_EXTS 一致：只列放得出声的。opus、wma 没有解码器，补音频时选了也会被后端拒掉 */
export const AUDIO_EXTENSIONS = ["flac", "mp3", "wav", "m4a", "ogg", "aac"];

export const FIELD_LABELS: Record<keyof SongEdit, string> = {
  title: tr("歌名"),
  artist: tr("歌手"),
  year: tr("年份"),
  album: tr("专辑"),
  genre: tr("流派"),
};

export const libraryApi = {
  edit: (songId: string, edit: SongEdit) => call<EditReport>("library_edit_song", { songId, edit }),
  remove: (songId: string) => call<DeleteResult>("library_delete_song", { songId }),
  missingAudio: () => call<MissingAudio[]>("library_missing_audio"),
  suggestRelinks: (folder: string) => call<RelinkSuggestion[]>("library_suggest_relinks", { folder }),
  relink: (links: { songId: string; path: string }[]) => call<RelinkBatch>("library_relink_audio", { links }),
};

/** 清单同步出了问题时的提示；正常（改了 / 没有清单 / 不用改）返回 null */
export function csvProblem(csv: CsvSync | undefined): string | null {
  return csv === undefined || csv === "updated" || csv === "noFile" || csv === "unchanged" ? null : csv;
}

/** 保存后给人看的一句话 */
export function describeEdit(report: EditReport): string {
  if (report.changed.length === 0) return tr("没有改动");
  const parts = [tr("改了{map}", { map: report.changed.map((f) => FIELD_LABELS[f]).join(tr("、")) })];
  if (report.performers > 0) parts.push(tr("演唱署名重写为 {performers} 人", { performers: report.performers }));
  if (report.correctionsRenamed > 0) parts.push(tr("{correctionsRenamed} 条分词校正跟着改了歌名歌手", { correctionsRenamed: report.correctionsRenamed }));
  if (report.csv === "updated") parts.push(tr("songs.csv 已同步"));
  return parts.join(tr("，"));
}

export function describeDelete(result: DeleteResult): string {
  const parts = [tr("已删除《{title}》：{lyricLines} 行歌词、{tokens} 个词", { title: result.title, lyricLines: result.lyricLines, tokens: result.tokens })];
  if (result.plays > 0) parts.push(tr("{plays} 条收听记录", { plays: result.plays }));
  if (result.albumsRemoved > 0) parts.push(tr("{albumsRemoved} 张空专辑", { albumsRemoved: result.albumsRemoved }));
  if (result.peopleRemoved > 0) parts.push(tr("{peopleRemoved} 个没有作品了的人", { peopleRemoved: result.peopleRemoved }));
  if (result.coverRemoved) parts.push(tr("下载的封面"));
  const kept = result.correctionsKept > 0 ? tr("；{correctionsKept} 条分词校正留着，重新导入时恢复", { correctionsKept: result.correctionsKept }) : "";
  return tr("{parts}{kept}。音频文件没有动。", { parts: parts.join(tr("、")), kept });
}
