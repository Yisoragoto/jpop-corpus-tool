/**
 * 逐行编辑歌词用的小工具：时间戳的读写、改动的摘要。纯函数，有测试。
 */

/** 秒 → `m:ss.xx`（和 LRC 一样到厘秒）。没有时间是空串 */
export function formatStamp(sec: number | null): string {
  if (sec === null || !Number.isFinite(sec)) return "";
  const centis = Math.round(Math.max(0, sec) * 100);
  const m = Math.floor(centis / 6000);
  const s = Math.floor(centis / 100) % 60;
  const c = centis % 100;
  return `${m}:${String(s).padStart(2, "0")}.${String(c).padStart(2, "0")}`;
}

/**
 * 读时间戳。认 `1:02.5`、`01:02.50`、`62.5`（纯秒）、`[01:02.50]`；空串是「没有时间」（null）。
 * 认不出来返回 undefined——界面据此标红，不让存。
 */
export function parseStamp(input: string): number | null | undefined {
  const t = input.trim().replace(/^\[|\]$/g, "");
  if (t === "") return null;
  const m = /^(\d+):(\d{1,2})(?:[.:](\d{1,3}))?$/.exec(t);
  if (m) {
    const sec = Number(m[2]);
    if (sec >= 60) return undefined;
    const frac = m[3] ? Number(`0.${m[3]}`) : 0;
    return round2(Number(m[1]) * 60 + sec + frac);
  }
  if (/^\d+(\.\d+)?$/.test(t)) return round2(Number(t));
  return undefined;
}

const round2 = (x: number) => Math.round(x * 100) / 100;

export interface EditRow {
  timeSec: number | null;
  text: string;
}

/** 改了什么：按行文本比，给一句话（存之前给人看一眼） */
export function summarizeEdit(before: EditRow[], after: EditRow[]): string {
  const clean = after.filter((r) => r.text.trim() !== "");
  const key = (r: EditRow) => `${r.timeSec ?? ""}\u0000${r.text.trim()}`;
  if (before.length === clean.length && before.every((r, i) => key(r) === key(clean[i]!))) return "没有改动";
  const count = (rows: EditRow[]) => {
    const m = new Map<string, number>();
    for (const r of rows) m.set(r.text.trim(), (m.get(r.text.trim()) ?? 0) + 1);
    return m;
  };
  const a = count(before);
  const b = count(clean);
  let removed = 0;
  let added = 0;
  for (const [t, n] of a) removed += Math.max(0, n - (b.get(t) ?? 0));
  for (const [t, n] of b) added += Math.max(0, n - (a.get(t) ?? 0));
  const retimed =
    added === 0 && removed === 0 ? clean.filter((r, i) => before[i] && r.timeSec !== before[i]!.timeSec).length : 0;
  const parts = [];
  if (added) parts.push(`新增或改写 ${added} 行`);
  if (removed) parts.push(`去掉或改写前 ${removed} 行`);
  if (retimed) parts.push(`调了 ${retimed} 行的时间`);
  if (!parts.length) parts.push("调了行的顺序或时间");
  return `${parts.join("，")}（${before.length} → ${clean.length} 行）`;
}
