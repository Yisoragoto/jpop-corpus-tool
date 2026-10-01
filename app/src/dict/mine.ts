/**
 * 一键制卡：设置、IPC、类型。
 *
 * 设置只关乎这台机器上的这个用户，存 localStorage；存不了（隐私模式之类）就用默认值。
 * 默认用自己的「Lyrics」笔记类型（照 Lapis 做的，封面旁边有歌名歌手，第一次制卡时自动建）、JPOP 牌组、词+句卡。
 */

import { useSyncExternalStore } from "react";

import { call } from "../api";

export type LapisCardKind = "vocab" | "wordAndSentence" | "click" | "sentence" | "audio";

export const LAPIS_CARD_KIND_LABELS: Record<LapisCardKind, string> = {
  vocab: "单词卡",
  wordAndSentence: "词+句卡",
  click: "点击卡",
  sentence: "句子卡",
  audio: "音频卡",
};

export interface MineSettings {
  deck: string;
  /** 放进「牌组::歌手」子牌组，没有就建（合作曲按第一位歌手） */
  artistSubdeck: boolean;
  model: string;
  /** 首选释义词典（MainDefinition）；null 表示用排在最前、有释义的那本 */
  mainDictionary: string | null;
  kind: LapisCardKind;
}

/** 应用自己建的笔记类型 */
export const LYRICS_MODEL = "Lyrics";

export const DEFAULT_MINE_SETTINGS: MineSettings = {
  deck: "JPOP",
  artistSubdeck: false,
  model: LYRICS_MODEL,
  mainDictionary: null,
  kind: "wordAndSentence",
};

const STORAGE_KEY = "jp.mine.settings";
const KINDS = Object.keys(LAPIS_CARD_KIND_LABELS) as LapisCardKind[];

function load(): MineSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === null) return DEFAULT_MINE_SETTINGS;
    const parsed = JSON.parse(raw) as Partial<Record<keyof MineSettings, unknown>>;
    return {
      deck: typeof parsed.deck === "string" && parsed.deck !== "" ? parsed.deck : DEFAULT_MINE_SETTINGS.deck,
      artistSubdeck: parsed.artistSubdeck === true,
      model: typeof parsed.model === "string" && parsed.model !== "" ? parsed.model : DEFAULT_MINE_SETTINGS.model,
      mainDictionary: typeof parsed.mainDictionary === "string" ? parsed.mainDictionary : null,
      kind: KINDS.includes(parsed.kind as LapisCardKind) ? (parsed.kind as LapisCardKind) : DEFAULT_MINE_SETTINGS.kind,
    };
  } catch {
    return DEFAULT_MINE_SETTINGS;
  }
}

let settings = load();
const listeners = new Set<() => void>();

export function updateMineSettings(patch: Partial<MineSettings>) {
  settings = { ...settings, ...patch };
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // 存不了就只在这次会话里生效
  }
  for (const listener of listeners) listener();
}

export function useMineSettings(): MineSettings {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => settings,
    () => settings,
  );
}

/** 这张卡从哪来：查的哪段文字、哪首歌哪一行第几个词 */
export interface MineContext {
  lookupText: string;
  songId?: string | undefined;
  utteranceId?: number | undefined;
  tokenIndex?: number | undefined;
  /** 这首歌的歌手。只用来在按钮提示里说会放进哪个子牌组；后端按曲库里的歌手自己算，不看这个 */
  artist?: string | undefined;
}

/**
 * 按歌手放的子牌组名，和后端 `jp_anki::mine::artist_subdeck` 同一个规则（测试用的是同一组例子）：
 * 合作曲取第一位歌手，歌手名里的 `::` 换成单个冒号（免得多出一层），歌手是空的返回 null。
 */
export function artistSubdeck(parent: string, artist: string): string | null {
  let name = (artist.split("/")[0] ?? "").trim();
  while (name.includes("::")) name = name.replaceAll("::", ":");
  name = name.trim();
  if (name === "" || parent.trim() === "") return null;
  return `${parent.trim()}::${name}`;
}

export type MineOutcome =
  | {
      kind: "added";
      noteId: number;
      /** 实际放进的牌组 */
      deck: string;
      /** 子牌组是这次新建的 */
      deckCreated: boolean;
      skippedFields: string[];
      warnings: string[];
    }
  | { kind: "duplicate"; noteIds: number[] };

export interface MineCheck {
  connected: boolean;
  message: string;
  /** 和传入的词头一一对应 */
  notes: number[][];
}

export const mineApi = {
  mine: (context: MineContext, term: string, reading: string, s: MineSettings) =>
    call<MineOutcome>("anki_mine", {
      request: {
        lookupText: context.lookupText,
        term,
        reading,
        songId: context.songId ?? null,
        utteranceId: context.utteranceId ?? null,
        tokenIndex: context.tokenIndex ?? null,
        deck: s.deck,
        artistSubdeck: s.artistSubdeck,
        model: s.model,
        mainDictionary: s.mainDictionary,
        kind: s.kind,
      },
    }),
  check: (model: string, expressions: string[]) => call<MineCheck>("anki_mine_check", { model, expressions }),
  browse: (noteIds: number[]) => call<void>("anki_browse_notes", { noteIds }),
  modelNames: () => call<string[]>("anki_model_names"),
};
