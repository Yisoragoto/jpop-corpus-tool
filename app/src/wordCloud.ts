/**
 * 词云的纯逻辑：合并词元、排布、按歌分组 KWIC 命中。不碰 DOM，量字宽的函数从外面传进来。
 *
 * 排布是最朴素的那种：从中心沿阿基米德螺线往外找位置，跟已经放下的词做矩形碰撞。
 * 一百来个词、O(n²) 的碰撞检查，几毫秒的事，不值得上四叉树或像素掩膜。
 * 用固定种子的随机数，同样的数据和宽度排出来一模一样——筛选没变就不会每次重渲染都换一种摆法。
 */

import type { FrequencyRow, KwicHit } from "./api";

export interface CloudWord {
  /** 显示的写法：几种写法合并时取次数最多的那种 */
  lemma: string;
  /** 合并进来的所有词元写法（想い出・思い出），显示的那个在最前。点开查歌时都要查 */
  variants: string[];
  /** 同一个词元在不同词性下的次数加起来 */
  freq: number;
  songCount: number;
  /** 只有一种词性时才有：点开查歌时带上，结果更准 */
  pos: string | null;
  /** 次数最多的那个词性（按词性上色用） */
  mainPos: string;
  /** 歌词里实际出现的写法（字幕里标亮用） */
  surfaces: string[];
}

/** 后端给的一个词元的正规化写法和读音（`lemma_forms`） */
export interface LemmaForm {
  lemma: string;
  normalized: string;
  reading: string;
}

const HAS_LATIN = /[A-Za-zＡ-Ｚａ-ｚ]/u;
const KANA_ONLY = /^[ぁ-ゟ゠-ヿー]+$/u;

function toHiragana(text: string): string {
  return text.replace(/[ァ-ヶ]/gu, (ch) => String.fromCharCode(ch.charCodeAt(0) - 0x60));
}

/**
 * 异体写法的合并键：**正规化写法相同、读音也相同**才算同一个词。
 *
 * - 想い出 / 思い出、夕陽 / 夕日、淋しい / 寂しい 合并；
 * - 遠く / 遠い、言える / 言う 的正规化写法相同但读音不同，不合并（词性或意思已经变了）；
 * - 含拉丁字母的不合并（Right 会被正规化成ライト，和 light 撞上）；
 * - 两个字以内的纯假名只和自己的平 / 片假名写法合并：かみ、はな这种对得上好几个汉字词，单看假名认不准。
 */
export function variantKey(lemma: string, form: LemmaForm | undefined): string {
  if (HAS_LATIN.test(lemma)) return `=${lemma}`;
  if (KANA_ONLY.test(lemma) && [...lemma].length <= 2) return `k:${toHiragana(lemma)}`;
  if (form === undefined || form.reading === "") return `=${lemma}`;
  return `n:${form.normalized}|${form.reading}`;
}

/**
 * 统计表是「词元 × 词性」，词云按词元算：同一个词元合成一个。
 * 给了 `keyOf` 时再按它把异体写法合成一个（见 `variantKey`）；显示次数最多的那种写法。
 * 曲数取最大的那一行——几种词性出现的歌会重叠，直接相加会多算。
 */
export function mergeByLemma(rows: FrequencyRow[], limit: number, keyOf: (lemma: string) => string = (l) => l): CloudWord[] {
  interface Acc {
    freq: number;
    songCount: number;
    surfaces: string[];
    byLemma: Map<string, number>;
    byPos: Map<string, number>;
  }
  const groups = new Map<string, Acc>();
  for (const row of rows) {
    const key = keyOf(row.lemma);
    let g = groups.get(key);
    if (g === undefined) {
      g = { freq: 0, songCount: 0, surfaces: [], byLemma: new Map(), byPos: new Map() };
      groups.set(key, g);
    }
    g.freq += row.freq;
    g.songCount = Math.max(g.songCount, row.songCount);
    g.byLemma.set(row.lemma, (g.byLemma.get(row.lemma) ?? 0) + row.freq);
    g.byPos.set(row.pos, (g.byPos.get(row.pos) ?? 0) + row.freq);
    for (const sf of row.surfaces) if (!g.surfaces.includes(sf)) g.surfaces.push(sf);
  }
  const byCount = (m: Map<string, number>) => [...m].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([k]) => k);
  return [...groups.values()]
    .map((g): CloudWord => {
      const variants = byCount(g.byLemma);
      const poses = byCount(g.byPos);
      return {
        lemma: variants[0] ?? "",
        variants,
        freq: g.freq,
        songCount: g.songCount,
        pos: poses.length === 1 ? (poses[0] ?? null) : null,
        mainPos: poses[0] ?? "",
        surfaces: g.surfaces,
      };
    })
    .sort((a, b) => b.freq - a.freq || a.lemma.localeCompare(b.lemma))
    .slice(0, limit);
}

/**
 * 词云里该不该出现的「虚词」。分词器把它们标成名词，但单独拿出来没有意思：
 *
 * - **单个假名**：僕「ら」、一「つ」、「お」花、「る」——是后缀、量词、前缀被切出来的碎片；
 * - **形式名词**：こと・もの・まま・よう・とき・ところ……在句子里只起语法作用；
 * - **一个字母、数字都没有的**：「~」「＂」这类符号。
 *
 * 只认词元，不看词性：同一个词元在别的词性下也一样没意思。
 */
const FUNCTION_NOUNS = new Set([
  "こと", "事", "もの", "物", "まま", "儘", "よう", "様", "とき", "ところ", "所", "ため", "為",
  "はず", "筈", "わけ", "訳", "うち", "ほう", "つもり", "くらい", "ぐらい", "ほど", "たび", "ふり",
  "たち", "達", "さん", "ちゃん", "くん", "等", "など", "なんか", "みたい", "ごと", "さぁ", "さあ",
]);

const SINGLE_KANA = /^[ぁ-ゟ゠-ヿー]$/u;
const HAS_LETTER = /[\p{L}\p{N}]/u;

/**
 * 词性本身就是虚词的：助词、助动词、接续词、连体词、标点。
 * 词性选「全部」时，不去掉它们云里就只剩「から・って・だけ」。
 */
const FUNCTION_POS = new Set(["ADP", "AUX", "SCONJ", "CCONJ", "PART", "DET", "PUNCT", "SYM", "X"]);

export function isFunctionPos(pos: string): boolean {
  return FUNCTION_POS.has(pos);
}

export function isFunctionWord(lemma: string): boolean {
  const w = lemma.trim();
  return w === "" || FUNCTION_NOUNS.has(w) || SINGLE_KANA.test(w) || !HAS_LETTER.test(w);
}

/** 进云之前先筛：去虚词（开着时）、去用户自己拿掉的词 */
export function filterCloudRows<T extends { lemma: string; pos?: string }>(
  rows: T[],
  hideFunctionWords: boolean,
  hidden: ReadonlySet<string>,
): T[] {
  return rows.filter(
    (r) => !hidden.has(r.lemma) && !(hideFunctionWords && (isFunctionWord(r.lemma) || isFunctionPos(r.pos ?? ""))),
  );
}

export interface Placed {
  word: CloudWord;
  /** 0 起的名次（按次数） */
  rank: number;
  /** 词框左上角和大小，px */
  x: number;
  y: number;
  w: number;
  h: number;
  fontSize: number;
  vertical: boolean;
}

export interface LayoutOptions {
  width: number;
  height: number;
  minFont: number;
  maxFont: number;
  /** 两到四个字的纯日文词里，大约多少比例竖排。前三名永远横排 */
  verticalRatio: number;
  /** 量一段文字在某个字号下的宽度 */
  measure: (text: string, fontSize: number) => number;
  seed?: number;
}

/** mulberry32：够用的确定性随机数 */
export function seeded(seed: number): () => number {
  let s = seed | 0;
  return () => {
    s = (s + 0x6d2b79f5) | 0;
    let t = Math.imul(s ^ (s >>> 15), 1 | s);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const JAPANESE_ONLY = /^[぀-ヿ㐀-鿿々ー]+$/u;

/** 字号：按次数的 0.55 次方插值。线性的话第一名太大，后面全挤成最小号 */
export function fontSizeFor(freq: number, min: number, max: number, minFont: number, maxFont: number): number {
  const t = max === min ? 1 : (freq - min) / (max - min);
  return Math.round(minFont + (maxFont - minFont) * Math.pow(Math.max(0, t), 0.55));
}

export function layoutCloud(words: CloudWord[], opt: LayoutOptions): Placed[] {
  const { width: W, height: H, minFont, maxFont, verticalRatio, measure } = opt;
  if (words.length === 0 || W <= 0 || H <= 0) return [];
  const rand = seeded((opt.seed ?? 7) * 31 + Math.round(W));
  const fmax = words[0]?.freq ?? 1;
  const fmin = words[words.length - 1]?.freq ?? 0;
  const cx = W / 2;
  const cy = H / 2;
  const stretch = Math.max(1, W / H); // 宽的画布上螺线拉成椭圆，词才铺得满
  const placed: Placed[] = [];
  const margin = 4;

  words.forEach((word, rank) => {
    const chars = [...word.lemma].length;
    const vertical =
      rank > 2 && chars >= 2 && chars <= 4 && JAPANESE_ONLY.test(word.lemma) && rand() < verticalRatio;
    let size = fontSizeFor(word.freq, fmin, fmax, minFont, maxFont);
    const start = rand() * Math.PI * 2;

    // 放不下就缩小再试，最多三次；还放不下就算了（只会是最尾巴的小词）
    for (let attempt = 0; attempt < 4; attempt++) {
      const w = vertical ? size * 1.1 : measure(word.lemma, size) + size * 0.14;
      const h = vertical ? chars * size + size * 0.12 : size * 1.18;
      const pad = Math.max(2, size * 0.06);
      for (let step = 0; step < 2600; step++) {
        const angle = start + step * 0.17;
        const r = 1.6 * (angle - start);
        const x = cx + r * Math.cos(angle) * stretch - w / 2;
        const y = cy + r * Math.sin(angle) - h / 2;
        if (x < margin || y < margin || x + w > W - margin || y + h > H - margin) {
          if (r > Math.max(W, H)) break;
          continue;
        }
        const hit = placed.some(
          (p) => x < p.x + p.w + pad && x + w + pad > p.x && y < p.y + p.h + pad && y + h + pad > p.y,
        );
        if (!hit) {
          placed.push({ word, rank, x, y, w, h, fontSize: size, vertical });
          return;
        }
      }
      size = Math.max(Math.round(minFont * 0.8), Math.round(size * 0.78));
    }
  });
  return placed;
}

export interface SongHits {
  songId: string;
  title: string;
  artist: string;
  /** 这首歌里出现几次（KWIC 折叠的重复行也算上） */
  count: number;
  /** 列表里显示、点进去跳到的那一处：有在句首的就用句首那一处，否则第一处 */
  first: KwicHit;
  /** 这首歌里有一行就是以这个词开头的：从这一行开始放，听到的第一个词就是它 */
  atStart: boolean;
}

/**
 * 这一处是不是在句首：**这一行从这个词开始**（左边除了空白什么都没有）。
 * 点歌是从这一行开头放的，只有这样放出来听到的第一个词才是它；
 * 「本当なんだ　夜みたいで」里的夜在空格后面，放出来先听到的是「本当なんだ」，不算。
 */
export function isSentenceStart(left: string): boolean {
  return left.trim() === "";
}

/**
 * KWIC 命中按歌分组。**有一行以这个词开头的歌排前面**，每首歌显示的也是那一行；
 * 同一档里次数多的在前，再一样按命中顺序（也就是曲库顺序）。
 */
export function groupHitsBySong(hits: KwicHit[]): SongHits[] {
  const bySong = new Map<string, SongHits>();
  for (const hit of hits) {
    const song = bySong.get(hit.songId);
    const n = Math.max(1, hit.repeatCount);
    const start = isSentenceStart(hit.left);
    if (song) {
      song.count += n;
      if (start && !song.atStart) {
        song.first = hit;
        song.atStart = true;
      }
    } else {
      bySong.set(hit.songId, { songId: hit.songId, title: hit.title, artist: hit.artist, count: n, first: hit, atStart: start });
    }
  }
  return [...bySong.values()].sort((a, b) => Number(b.atStart) - Number(a.atStart) || b.count - a.count);
}

/** 按词性上色时每种词性的颜色（深色底上都看得清，互相分得开） */
export const POS_COLORS: Record<string, string> = {
  NOUN: "rgb(159, 216, 255)",
  PROPN: "rgb(196, 170, 255)",
  VERB: "rgb(255, 160, 190)",
  ADJ: "rgb(255, 205, 120)",
  ADV: "rgb(140, 230, 170)",
};
export const OTHER_POS_COLOR = "rgb(220, 220, 228)";
