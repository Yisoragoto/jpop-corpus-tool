/*
 * 日语注音分配与音高工具。
 *
 * Yomitan `ext/js/language/ja/japanese.js` 相应函数的 TypeScript 移植：
 * Copyright (C) 2024-2026  Yomitan Authors；Copyright (C) 2020-2022  Yomichan Authors。
 * GPL-3.0-or-later。
 *
 * 同是 JS 字符串，长度、下标、substring 都按 UTF-16 码元，和原版一模一样，所以逐行照抄即可。
 * 对账基准 `__fixtures__/yomitan-japanese-util.json` 由 Yomitan 源码直接算出
 * （Yomitan 自带用例 + 1500 个真实词典词头）。
 */

export interface FuriganaSegment {
  text: string;
  reading: string;
}

export type PitchCategory = "heiban" | "kifuku" | "atamadaka" | "odaka" | "nakadaka";

type Range = [number, number];

const KATAKANA_SMALL_KA_CODE_POINT = 0x30f5;
const KATAKANA_SMALL_KE_CODE_POINT = 0x30f6;
const KANA_PROLONGED_SOUND_MARK_CODE_POINT = 0x30fc;

const HIRAGANA_RANGE: Range = [0x3040, 0x309f];
const KATAKANA_RANGE: Range = [0x30a0, 0x30ff];
const HIRAGANA_CONVERSION_RANGE: Range = [0x3041, 0x3096];
const KATAKANA_CONVERSION_RANGE: Range = [0x30a1, 0x30f6];
const KANA_RANGES: Range[] = [HIRAGANA_RANGE, KATAKANA_RANGE];

const SMALL_KANA_SET = new Set("ぁぃぅぇぉゃゅょゎァィゥェォャュョヮ");

const VOWEL_TO_KANA_MAPPING: [string, string][] = [
  ["a", "ぁあかがさざただなはばぱまゃやらゎわヵァアカガサザタダナハバパマャヤラヮワヵヷ"],
  ["i", "ぃいきぎしじちぢにひびぴみりゐィイキギシジチヂニヒビピミリヰヸ"],
  ["u", "ぅうくぐすずっつづぬふぶぷむゅゆるゥウクグスズッツヅヌフブプムュユルヴ"],
  ["e", "ぇえけげせぜてでねへべぺめれゑヶェエケゲセゼテデネヘベペメレヱヶヹ"],
  ["o", "ぉおこごそぞとどのほぼぽもょよろをォオコゴソゾトドノホボポモョヨロヲヺ"],
  ["", "のノ"],
];

const KANA_TO_VOWEL_MAPPING = new Map<string, string>();
for (const [vowel, characters] of VOWEL_TO_KANA_MAPPING) {
  for (const character of characters) {
    KANA_TO_VOWEL_MAPPING.set(character, vowel);
  }
}

const DIACRITIC_KANA =
  "うゔ-かが-きぎ-くぐ-けげ-こご-さざ-しじ-すず-せぜ-そぞ-ただ-ちぢ-つづ-てで-とど-はばぱひびぴふぶぷへべぺほぼぽワヷ-ヰヸ-ウヴ-ヱヹ-ヲヺ-カガ-キギ-クグ-ケゲ-コゴ-サザ-シジ-スズ-セゼ-ソゾ-タダ-チヂ-ツヅ-テデ-トド-ハバパヒビピフブプヘベペホボポ";

export type DiacriticType = "dakuten" | "handakuten";

const DIACRITIC_MAPPING = new Map<string, { character: string; type: DiacriticType }>();
for (let i = 0, ii = DIACRITIC_KANA.length; i < ii; i += 3) {
  const character = DIACRITIC_KANA.charAt(i);
  const dakuten = DIACRITIC_KANA.charAt(i + 1);
  const handakuten = DIACRITIC_KANA.charAt(i + 2);
  DIACRITIC_MAPPING.set(dakuten, { character, type: "dakuten" });
  if (handakuten !== "-") {
    DIACRITIC_MAPPING.set(handakuten, { character, type: "handakuten" });
  }
}

function isCodePointInRange(codePoint: number, [min, max]: Range): boolean {
  return codePoint >= min && codePoint <= max;
}

function isCodePointInRanges(codePoint: number, ranges: Range[]): boolean {
  return ranges.some((range) => isCodePointInRange(codePoint, range));
}

export function isCodePointKana(codePoint: number): boolean {
  return isCodePointInRanges(codePoint, KANA_RANGES);
}

function getProlongedHiragana(previousCharacter: string): string | null {
  switch (KANA_TO_VOWEL_MAPPING.get(previousCharacter)) {
    case "a":
      return "あ";
    case "i":
      return "い";
    case "u":
      return "う";
    case "e":
      return "え";
    case "o":
      return "う";
    default:
      return null;
  }
}

export function convertKatakanaToHiragana(text: string, keepProlongedSoundMarks = false): string {
  let result = "";
  const offset = HIRAGANA_CONVERSION_RANGE[0] - KATAKANA_CONVERSION_RANGE[0];
  for (let char of text) {
    const codePoint = char.codePointAt(0) as number;
    switch (codePoint) {
      case KATAKANA_SMALL_KA_CODE_POINT:
      case KATAKANA_SMALL_KE_CODE_POINT:
        break;
      case KANA_PROLONGED_SOUND_MARK_CODE_POINT:
        if (!keepProlongedSoundMarks && result.length > 0) {
          const char2 = getProlongedHiragana(result.charAt(result.length - 1));
          if (char2 !== null) char = char2;
        }
        break;
      default:
        if (isCodePointInRange(codePoint, KATAKANA_CONVERSION_RANGE)) {
          char = String.fromCodePoint(codePoint + offset);
        }
        break;
    }
    result += char;
  }
  return result;
}

export function getKanaDiacriticInfo(character: string): { character: string; type: DiacriticType } | null {
  const info = DIACRITIC_MAPPING.get(character);
  return info !== undefined ? { character: info.character, type: info.type } : null;
}

// ── 注音分配 ──

interface FuriganaGroup {
  isKana: boolean;
  text: string;
  textNormalized: string | null;
}

function createFuriganaSegment(text: string, reading: string): FuriganaSegment {
  return { text, reading };
}

function segmentizeFurigana(
  reading: string,
  readingNormalized: string,
  groups: FuriganaGroup[],
  groupsStart: number,
): FuriganaSegment[] | null {
  const groupCount = groups.length - groupsStart;
  if (groupCount <= 0) {
    return reading.length === 0 ? [] : null;
  }

  const group = groups[groupsStart] as FuriganaGroup;
  const { isKana, text } = group;
  const textLength = text.length;
  if (isKana) {
    const { textNormalized } = group;
    if (textNormalized !== null && readingNormalized.startsWith(textNormalized)) {
      const segments = segmentizeFurigana(
        reading.substring(textLength),
        readingNormalized.substring(textLength),
        groups,
        groupsStart + 1,
      );
      if (segments !== null) {
        if (reading.startsWith(text)) {
          segments.unshift(createFuriganaSegment(text, ""));
        } else {
          segments.unshift(...getFuriganaKanaSegments(text, reading));
        }
        return segments;
      }
    }
    return null;
  }

  let result: FuriganaSegment[] | null = null;
  for (let i = reading.length; i >= textLength; --i) {
    const segments = segmentizeFurigana(
      reading.substring(i),
      readingNormalized.substring(i),
      groups,
      groupsStart + 1,
    );
    if (segments !== null) {
      if (result !== null) {
        // 尾部不止一种分法，算有歧义
        return null;
      }
      segments.unshift(createFuriganaSegment(text, reading.substring(0, i)));
      result = segments;
    }
    // 最后一个非假名组只有一种分法
    if (groupCount === 1) break;
  }
  return result;
}

function getFuriganaKanaSegments(text: string, reading: string): FuriganaSegment[] {
  const textLength = text.length;
  const newSegments: FuriganaSegment[] = [];
  let start = 0;
  let state = reading.charAt(0) === text.charAt(0);
  for (let i = 1; i < textLength; ++i) {
    const newState = reading.charAt(i) === text.charAt(i);
    if (state === newState) continue;
    newSegments.push(createFuriganaSegment(text.substring(start, i), state ? "" : reading.substring(start, i)));
    state = newState;
    start = i;
  }
  newSegments.push(
    createFuriganaSegment(text.substring(start, textLength), state ? "" : reading.substring(start, textLength)),
  );
  return newSegments;
}

function getStemLength(text1: string, text2: string): number {
  const minLength = Math.min(text1.length, text2.length);
  if (minLength === 0) return 0;

  let i = 0;
  while (true) {
    const char1 = text1.codePointAt(i) as number;
    const char2 = text2.codePointAt(i) as number;
    if (char1 !== char2) break;
    const charLength = String.fromCodePoint(char1).length;
    i += charLength;
    if (i >= minLength) {
      if (i > minLength) {
        i -= charLength; // 不吃半个代理对
      }
      break;
    }
  }
  return i;
}

/** 把读音分配到词形的各段上：汉字段带读音，假名段读音为空。分不出来时整词一段。 */
export function distributeFurigana(term: string, reading: string): FuriganaSegment[] {
  if (reading === term) {
    return [createFuriganaSegment(term, "")];
  }

  const groups: FuriganaGroup[] = [];
  let groupPre: FuriganaGroup | null = null;
  let isKanaPre: boolean | null = null;
  for (const c of term) {
    const isKana = isCodePointKana(c.codePointAt(0) as number);
    if (isKana === isKanaPre && groupPre !== null) {
      groupPre.text += c;
    } else {
      groupPre = { isKana, text: c, textNormalized: null };
      groups.push(groupPre);
      isKanaPre = isKana;
    }
  }
  for (const group of groups) {
    if (group.isKana) {
      group.textNormalized = convertKatakanaToHiragana(group.text);
    }
  }

  const readingNormalized = convertKatakanaToHiragana(reading);
  const segments = segmentizeFurigana(reading, readingNormalized, groups, 0);
  if (segments !== null) {
    return segments;
  }
  return [createFuriganaSegment(term, reading)];
}

/** 活用形的注音：词干按词典形分配，活用词尾照原文显示。 */
export function distributeFuriganaInflected(term: string, reading: string, source: string): FuriganaSegment[] {
  const termNormalized = convertKatakanaToHiragana(term);
  const readingNormalized = convertKatakanaToHiragana(reading);
  const sourceNormalized = convertKatakanaToHiragana(source);

  let mainText = term;
  let stemLength = getStemLength(termNormalized, sourceNormalized);

  // 原文是从读音变过来的（而不是从词形）
  const readingStemLength = getStemLength(readingNormalized, sourceNormalized);
  if (readingStemLength > 0 && readingStemLength >= stemLength) {
    mainText = reading;
    stemLength = readingStemLength;
    reading = `${source.substring(0, stemLength)}${reading.substring(stemLength)}`;
  }

  const segments: FuriganaSegment[] = [];
  if (stemLength > 0) {
    mainText = `${source.substring(0, stemLength)}${mainText.substring(stemLength)}`;
    const segments2 = distributeFurigana(mainText, reading);
    let consumed = 0;
    for (const segment of segments2) {
      const { text } = segment;
      const start = consumed;
      consumed += text.length;
      if (consumed < stemLength) {
        segments.push(segment);
      } else if (consumed === stemLength) {
        segments.push(segment);
        break;
      } else {
        if (start < stemLength) {
          segments.push(createFuriganaSegment(mainText.substring(start, stemLength), ""));
        }
        break;
      }
    }
  }

  if (stemLength < source.length) {
    const remainder = source.substring(stemLength);
    const last = segments[segments.length - 1];
    if (last !== undefined && last.reading.length === 0) {
      // 最后一段没有读音就接在它后面
      last.text += remainder;
    } else {
      segments.push(createFuriganaSegment(remainder, ""));
    }
  }

  return segments;
}

// ── 音拍与音高 ──

export function isMoraPitchHigh(moraIndex: number, pitchAccentValue: number | string): boolean {
  if (typeof pitchAccentValue === "string") {
    return pitchAccentValue.charAt(moraIndex) === "H";
  }
  switch (pitchAccentValue) {
    case 0:
      return moraIndex > 0;
    case 1:
      return moraIndex < 1;
    default:
      return moraIndex > 0 && moraIndex < pitchAccentValue;
  }
}

export function getDownstepPositions(pitchString: string): number[] {
  const downsteps: number[] = [];
  const moraCount = pitchString.length;
  for (let i = 0; i < moraCount; i++) {
    if (i > 0 && pitchString.charAt(i - 1) === "H" && pitchString.charAt(i) === "L") {
      downsteps.push(i);
    }
  }
  if (downsteps.length === 0) {
    downsteps.push(pitchString.startsWith("L") ? 0 : -1);
  }
  return downsteps;
}

export function getKanaMorae(text: string): string[] {
  const morae: string[] = [];
  for (const c of text) {
    const last = morae.length - 1;
    if (SMALL_KANA_SET.has(c) && last >= 0) {
      morae[last] += c;
    } else {
      morae.push(c);
    }
  }
  return morae;
}

export function getKanaMoraCount(text: string): number {
  let moraCount = 0;
  for (const c of text) {
    if (!(SMALL_KANA_SET.has(c) && moraCount > 0)) {
      ++moraCount;
    }
  }
  return moraCount;
}

export function getPitchCategory(
  text: string,
  pitchAccentValue: number | string,
  isVerbOrAdjective: boolean,
): PitchCategory | null {
  const downstep =
    typeof pitchAccentValue === "string" ? (getDownstepPositions(pitchAccentValue)[0] as number) : pitchAccentValue;
  if (downstep === 0) {
    return "heiban";
  }
  if (isVerbOrAdjective) {
    return downstep > 0 ? "kifuku" : null;
  }
  if (downstep === 1) {
    return "atamadaka";
  }
  if (downstep > 1) {
    return downstep >= getKanaMoraCount(text) ? "odaka" : "nakadaka";
  }
  return null;
}
