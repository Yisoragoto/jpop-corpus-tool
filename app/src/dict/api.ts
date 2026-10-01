/**
 * 词典（Yomitan 格式）的 IPC 与类型。
 *
 * 查词结果的形状和 Yomitan `translator.findTerms` 的输出逐字段一致（Rust 侧已拿 Yomitan 的期望结果对账），
 * 所以这里的类型也照 Yomitan 的 `dictionary.d.ts` 写。
 */

import { call } from "../api";

export interface DictionaryCounts {
  terms?: number;
  /** `total` 加各 mode（freq / pitch / ipa） */
  termMeta?: Record<string, number>;
  tagMeta?: number;
  media?: number;
  kanjiSkipped?: number;
  glossaryBytes?: number;
  compressedBytes?: number;
}

export interface DictionaryInfo {
  id: number;
  title: string;
  revision: string;
  version: number;
  sequenced: boolean;
  frequencyMode: string | null;
  author: string | null;
  url: string | null;
  description: string | null;
  attribution: string | null;
  sourceLanguage: string | null;
  targetLanguage: string | null;
  counts: DictionaryCounts;
  warnings: string[];
  hasStyles: boolean;
  sourcePath: string;
  enabled: boolean;
  priority: number;
  /** Unix 毫秒 */
  importedAt: number;
}

/** 旧版（PyQt）登记过的词典包 */
export interface LegacySource {
  name: string;
  path: string;
  /** terms / freq / pitch */
  kind: string;
  exists: boolean;
  imported: boolean;
}

export interface ImportSummary {
  dictionaryId: number;
  title: string;
  revision: string;
  version: number;
  terms: number;
  termMeta: Record<string, number>;
  tags: number;
  media: number;
  kanjiSkipped: number;
  glossaryBytes: number;
  compressedBytes: number;
  warnings: string[];
  warningCount: number;
  elapsedMs: number;
}

/** `dict://progress` */
export interface DictImportProgress {
  index: number;
  total: number;
  path: string;
  file: string;
  doneFiles: number;
  totalFiles: number;
}

export interface DictImportResult {
  path: string;
  summary: ImportSummary | null;
  error: string | null;
}

/** `dict://done` */
export interface DictImportDone {
  results: DictImportResult[];
  cancelled: boolean;
  error: string | null;
}

// ── 查词结果（Yomitan dictionary.d.ts） ──

export interface Tag {
  name: string;
  category: string;
  order: number;
  score: number;
  content: string[];
  dictionaries: string[];
  redundant: boolean;
}

export interface TermSource {
  originalText: string;
  transformedText: string;
  deinflectedText: string;
  matchType: string;
  matchSource: "term" | "reading";
  isPrimary: boolean;
}

export interface TermHeadword {
  index: number;
  headwordIndex: number;
  term: string;
  reading: string;
  sources: TermSource[];
  tags: Tag[];
  wordClasses: string[];
}

/** 结构化内容（Yomitan structured-content 模式） */
export type StructuredContent = string | StructuredContent[] | StructuredElement;

export interface StructuredElement {
  tag: string;
  content?: StructuredContent;
  data?: Record<string, string>;
  style?: Record<string, unknown>;
  lang?: string;
  title?: string;
  open?: boolean;
  colSpan?: number;
  rowSpan?: number;
  href?: string;
  path?: string;
  width?: number;
  height?: number;
  preferredWidth?: number;
  preferredHeight?: number;
  alt?: string;
  description?: string;
  pixelated?: boolean;
  imageRendering?: string;
  appearance?: string;
  background?: boolean;
  collapsed?: boolean;
  collapsible?: boolean;
  verticalAlign?: string;
  border?: string;
  borderRadius?: string;
  sizeUnits?: string;
}

/** 释义里的图片（`{type: "image"}`）。和结构化内容里的 `img` 元素属性相同，但没有 `tag`、`content`。 */
export type GlossaryImage = Omit<StructuredElement, "tag" | "content"> & {
  type: "image";
  path: string;
};

export interface GlossaryStructuredContent {
  type: "structured-content";
  content: StructuredContent;
}

export type GlossaryEntry = string | GlossaryImage | GlossaryStructuredContent;

export interface TermDefinition {
  index: number;
  headwordIndices: number[];
  dictionary: string;
  dictionaryIndex: number;
  dictionaryAlias: string;
  id: number;
  score: number;
  frequencyOrder: number;
  sequences: number[];
  isPrimary: boolean;
  tags: Tag[];
  entries: GlossaryEntry[];
}

export type Pronunciation =
  | {
      type: "pitch-accent";
      positions: number | string;
      nasalPositions: number[];
      devoicePositions: number[];
      tags: Tag[];
    }
  | { type: "phonetic-transcription"; ipa: string; tags: Tag[] };

export interface TermPronunciation {
  index: number;
  headwordIndex: number;
  dictionary: string;
  dictionaryIndex: number;
  dictionaryAlias: string;
  pronunciations: Pronunciation[];
}

export interface TermFrequency {
  index: number;
  headwordIndex: number;
  dictionary: string;
  frequencyMode: string | null;
  dictionaryIndex: number;
  dictionaryAlias: string;
  hasReading: boolean;
  frequency: number;
  displayValue: string | null;
  displayValueParsed: boolean;
}

export interface InflectionRule {
  name: string;
  description?: string;
}

export interface InflectionChain {
  source: "algorithm" | "dictionary" | "both";
  inflectionRules: InflectionRule[];
}

export interface TermDictionaryEntry {
  type: "term";
  isPrimary: boolean;
  textProcessorRuleChainCandidates: string[][];
  inflectionRuleChainCandidates: InflectionChain[];
  score: number;
  frequencyOrder: number;
  dictionaryIndex: number;
  dictionaryAlias: string;
  sourceTermExactMatchCount: number;
  matchPrimaryReading: boolean;
  maxOriginalTextLength: number;
  headwords: TermHeadword[];
  definitions: TermDefinition[];
  pronunciations: TermPronunciation[];
  frequencies: TermFrequency[];
}

export interface FindTermsResult {
  dictionaryEntries: TermDictionaryEntry[];
  /** 命中的原文长度，按 Unicode 字符计（不是 UTF-16 码元） */
  originalTextLength: number;
}

export type LookupMode = "group" | "split" | "term" | "simple";

export interface DictionaryStyles {
  title: string;
  styles: string;
}

export const dictApi = {
  list: () => call<DictionaryInfo[]>("dict_list"),
  legacySources: () => call<LegacySource[]>("dict_legacy_sources"),
  /** 后台导入。进度走 `dict://progress`，结束发 `dict://done`。 */
  importStart: (paths: string[]) => call<number>("dict_import_start", { paths }),
  importCancel: () => call<void>("dict_import_cancel"),
  isImporting: () => call<boolean>("dict_is_importing"),
  setEnabled: (id: number, enabled: boolean) => call<void>("dict_set_enabled", { id, enabled }),
  setOrder: (ids: number[]) => call<void>("dict_set_order", { ids }),
  remove: (id: number) => call<void>("dict_delete", { id }),
  /** 从 text 开头查词（后端最多看 16 个字、返回 32 条） */
  lookup: (text: string, mode: LookupMode = "group") => call<FindTermsResult>("dict_lookup", { text, mode }),
  /** 图片原始字节。IPC 退回 postMessage 通道时是数字数组，用 `media.ts` 的 `toBytes` 统一 */
  media: (dictionary: string, path: string) => call<ArrayBuffer | number[]>("dict_media", { dictionary, path }),
  styles: () => call<DictionaryStyles[]>("dict_styles"),
};
