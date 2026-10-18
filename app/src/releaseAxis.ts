/**
 * 人物档案上那条发行轴的分列：每一年一列，这一年的发行排在列里。
 *
 * 原来是按年份比例把圆点撒在一条线上，同一年的几张只错开 9px，叠成一坨；
 * 中间没发行的年份也照样占着宽度，十年里只发过三年的人，轴上大半是空的。
 * 现在一年一列，宽度跟着这一年发了几张走；**连着两年以上没发行就折成一个「…」**，
 * 只空一年的留一根细的列（看得出断了一年）。乐队解散那一年标出来，哪怕那年没发行。
 */

export type AxisColumn<T> =
  | { type: "year"; year: number; items: T[]; ended: boolean }
  | { type: "gap"; from: number; to: number };

export function buildAxisColumns<T extends { year: number }>(items: T[], endYear: number | null): AxisColumn<T>[] {
  if (items.length === 0) return [];
  const byYear = new Map<number, T[]>();
  for (const item of items) byYear.set(item.year, [...(byYear.get(item.year) ?? []), item]);
  const years = [...byYear.keys()];
  const first = Math.min(...years);
  const last = Math.max(...years, endYear ?? -Infinity);

  const columns: AxisColumn<T>[] = [];
  let emptyFrom: number | null = null;
  const flushEmpty = (until: number) => {
    if (emptyFrom === null) return;
    if (until - emptyFrom >= 2) columns.push({ type: "gap", from: emptyFrom, to: until - 1 });
    else for (let y = emptyFrom; y < until; y++) columns.push({ type: "year", year: y, items: [], ended: false });
    emptyFrom = null;
  };
  for (let year = first; year <= last; year++) {
    const list = byYear.get(year);
    const ended = year === endYear;
    if (list === undefined && !ended) {
      emptyFrom ??= year;
      continue;
    }
    flushEmpty(year);
    columns.push({ type: "year", year, items: list ?? [], ended });
  }
  return columns;
}
