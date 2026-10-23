/**
 * 真实歌词单位的时间轴与词首定位。位置使用 Unicode 标量下标，与后端一致。
 * 不按字数、拍数、行长生成时间；词段内的所有字符共享该段的原始起止时间。
 */
import type { KwicHit } from "./api";

export interface WordTiming {
  utteranceId: number;
  charStart: number;
  charEnd: number;
  startSec: number;
  endSec: number | null;
  /** 已知的真实来源为 lrc / manual；未知或算法来源不参与同步。 */
  source: string;
}

export type TimingKind = "manual" | "lrc" | "line";
export interface WordStart {
  /** 来源单位的原始开始时间；缺少可信单位时为行首，不附加估算或提前量。 */
  sec: number;
  kind: TimingKind;
}

/** 关键词在这一行里的字符区间 `[start, end)`，优先用左文脈定位重复词。 */
export function keywordSpan(hit: Pick<KwicHit, "text" | "left" | "keyword">): { start: number; end: number } {
  const chars = [...hit.text];
  const kw = [...hit.keyword];
  let start = -1;
  if (hit.text.startsWith(hit.left)) start = [...hit.left].length;
  if (start < 0 || chars.slice(start, start + kw.length).join("") !== hit.keyword) {
    const at = hit.text.indexOf(hit.keyword);
    start = at < 0 ? 0 : [...hit.text.slice(0, at)].length;
  }
  return { start, end: Math.min(chars.length, start + Math.max(1, kw.length)) };
}

export function validLyricTime(sec: number | null): sec is number {
  return sec !== null && Number.isFinite(sec) && sec >= 0;
}

/** 下一行与音频结束都是硬上界；坏的下一行时间不能放宽曲目边界。 */
export function lyricEndBound(nextLine: number | null, trackDuration: number | null): number | null {
  const bounds = [nextLine, trackDuration].filter(validLyricTime);
  return bounds.length > 0 ? Math.min(...bounds) : null;
}

function validRange(timing: WordTiming, length: number): boolean {
  return Number.isInteger(timing.charStart) && Number.isInteger(timing.charEnd)
    && timing.charStart >= 0 && timing.charStart < timing.charEnd && timing.charEnd <= length;
}

/** 校准 > 来源单位 > 行首。起点已知但结尾未知的单位可以定位，不能用来补齐动画。 */
export function wordStart(hit: Pick<KwicHit, "text" | "left" | "keyword" | "utteranceId" | "timeSec" | "endSec" | "endKind">, timings: readonly WordTiming[]): WordStart | null {
  const fallback: WordStart | null = validLyricTime(hit.timeSec) ? { sec: hit.timeSec, kind: "line" } : null;
  const chars = [...hit.text];
  const { start } = keywordSpan(hit);
  if (!hit.keyword || chars.slice(start, start + [...hit.keyword].length).join("") !== hit.keyword) return fallback;
  // 旧的试听窗口可能有估算结尾，它不能约束真实歌词单位的时间。
  const limit = hit.endKind !== "estimated" && validLyricTime(hit.endSec) ? hit.endSec : null;
  const mine = timings.filter((timing) => timing.utteranceId === hit.utteranceId
    && validRange(timing, chars.length) && timing.charStart <= start && start < timing.charEnd
    && validLyricTime(timing.startSec)
    && (!validLyricTime(hit.timeSec) || timing.startSec >= hit.timeSec)
    && (limit === null || timing.startSec < limit)
    && (timing.endSec === null || (validLyricTime(timing.endSec) && timing.endSec > timing.startSec && (limit === null || timing.endSec <= limit))));
  for (const source of ["manual", "lrc"] as const) {
    const units = mine.filter((timing) => timing.source === source);
    const unit = units[0];
    // 多个互相矛盾的单位不能靠返回顺序猜；退回可靠的行首。
    if (unit && units.every((other) => other.startSec === unit.startSec && other.charStart === unit.charStart && other.charEnd === unit.charEnd)) {
      return { sec: unit.startSec, kind: source };
    }
  }
  return fallback;
}

/** 按键校准时扣掉的反应时间，只影响用户录入的校准点。 */
export const REACTION_SEC = 0.15;

export interface CharTimeline {
  /** 一个来源单位内的所有 Unicode 字符具有相同的 start/end。 */
  starts: number[];
  ends: number[];
  kind: "manual" | "lrc";
}

/**
 * 只有已知来源的有效片段完整覆盖原文时才返回时间轴，否则该行回退。
 * 缺失结尾、未覆盖字符、重叠、倒退及越过已知行边界的时间都不补造。
 * 单独的手动起点只能用于 seek；有明确起止的人工单位可覆盖相同范围的来源单位。
 * 调用方负责按 utteranceId 筛选，避免重复副歌串用别行时间。
 */
export function charTimeline(
  text: string,
  lineStart: number | null,
  lineEnd: number | null,
  timings: readonly WordTiming[],
): CharTimeline | null {
  const length = [...text].length;
  if (length === 0 || (lineStart !== null && !validLyricTime(lineStart))
    || (lineEnd !== null && (!validLyricTime(lineEnd) || (lineStart !== null && lineEnd <= lineStart)))) return null;

  const units = new Map<string, WordTiming>();
  for (const unit of timings) {
    if (unit.source !== "lrc" && unit.source !== "manual") continue;
    // 手动锚点没有结束时间，不能据此推算剩余字符或单位的时长。
    if (unit.source === "manual" && unit.endSec === null) continue;
    const key = `${unit.charStart}:${unit.charEnd}`;
    const previous = units.get(key);
    if (previous?.source === unit.source && (previous.startSec !== unit.startSec || previous.endSec !== unit.endSec)) return null;
    if (!previous || unit.source === "manual") units.set(key, unit);
  }

  const ordered = [...units.values()].sort((a, b) => a.charStart - b.charStart || a.charEnd - b.charEnd);
  const starts: number[] = [];
  const ends: number[] = [];
  let cursor = 0;
  let previousEnd = lineStart ?? 0;
  for (const unit of ordered) {
    if (!validRange(unit, length) || unit.charStart !== cursor
      || !validLyricTime(unit.startSec) || !validLyricTime(unit.endSec)
      || unit.endSec <= unit.startSec || unit.startSec < previousEnd
      || (lineEnd !== null && unit.endSec > lineEnd)) return null;
    for (let index = unit.charStart; index < unit.charEnd; ++index) {
      starts.push(unit.startSec);
      ends.push(unit.endSec);
    }
    cursor = unit.charEnd;
    previousEnd = unit.endSec;
  }
  if (cursor !== length) return null;
  return { starts, ends, kind: ordered.some((unit) => unit.source === "manual") ? "manual" : "lrc" };
}

/** done 为已完成的字符数；[done, activeEnd) 整个真实单位共享 frac。 */
export function charProgress(timeline: CharTimeline, sec: number): { done: number; activeEnd: number; frac: number } {
  const { starts, ends } = timeline;
  if (!Number.isFinite(sec)) return { done: 0, activeEnd: 0, frac: 0 };
  let done = 0;
  while (done < ends.length && ends[done]! <= sec) ++done;
  const start = starts[done];
  const end = ends[done];
  if (start === undefined || end === undefined || sec <= start || end <= start) return { done, activeEnd: done, frac: 0 };
  let activeEnd = done + 1;
  while (activeEnd < ends.length && starts[activeEnd] === start && ends[activeEnd] === end) ++activeEnd;
  return { done, activeEnd, frac: Math.min(1, (sec - start) / (end - start)) };
}
