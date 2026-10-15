/**
 * 时间轴：每位演唱者一行（泳道），发行按日期落在横轴上。
 *
 * 行的顺序按第一次发行排，但**单飞的前成员紧跟在他的乐队下面**，
 * 乐队结束那一点画一道虚线连到他个人的第一张——きのこ帝国 2019 解散，
 * 下一行就是 佐藤千亜妃，线从 2019 接到她 2020 的第一张。
 */

import type { TimelineRelease } from "./api";
import type { Directory } from "./libraryIndex";

/** 日期换成带小数的年份：2014-07-01 ≈ 2014.5；只有年份的放在那一年正中 */
export function yearValue(date: string): number | null {
  const m = /^(\d{4})(?:-(\d{2})(?:-(\d{2}))?)?/.exec(date.trim());
  if (!m) return null;
  const y = Number(m[1]);
  if (m[2] === undefined) return y + 0.5;
  const month = Math.min(12, Math.max(1, Number(m[2])));
  const day = m[3] === undefined ? 15 : Math.min(31, Math.max(1, Number(m[3])));
  return y + (month - 1) / 12 + (day - 1) / 365;
}

export interface TimelineRow {
  id: number;
  name: string;
  imagePath: string;
  isBand: boolean;
  releases: (TimelineRelease & { at: number })[];
  start: number;
  /** 泳道画到哪：乐队结束的就到结束那天，否则到最后一次发行 */
  end: number;
  /** 乐队结束的那一刻（画个记号） */
  endedAt: number | null;
  /** 挂在某个乐队下面的成员行（缩进） */
  under: number | null;
}

export interface Connector {
  fromRow: number;
  toRow: number;
  fromAt: number;
  toAt: number;
}

export interface TimelineLayout {
  rows: TimelineRow[];
  connectors: Connector[];
  min: number;
  max: number;
}

export function buildTimeline(dir: Directory, releases: TimelineRelease[], query = ""): TimelineLayout {
  const byPerson = new Map<number, (TimelineRelease & { at: number })[]>();
  for (const r of releases) {
    const at = yearValue(r.date);
    if (at === null) continue;
    (byPerson.get(r.personId) ?? byPerson.set(r.personId, []).get(r.personId)!).push({ ...r, at });
  }
  for (const list of byPerson.values()) list.sort((a, b) => a.at - b.at);

  const makeRow = (id: number, under: number | null): TimelineRow | null => {
    const list = byPerson.get(id);
    const entry = dir.byId.get(id);
    if (!list || list.length === 0 || !entry) return null;
    const endedAt = entry.ended ? yearValue(entry.ended) : null;
    const isBand = (dir.membersOf.get(id)?.length ?? 0) > 0 || entry.artistType === "Group";
    const last = list[list.length - 1]!.at;
    return {
      id,
      name: entry.name,
      imagePath: entry.imagePath,
      isBand,
      releases: list,
      start: list[0]!.at,
      end: Math.max(last, endedAt ?? last),
      endedAt,
      under,
    };
  };

  const q = query.trim().toLowerCase();
  const wanted = (id: number) => {
    if (q === "") return true;
    const name = dir.byId.get(id)?.name.toLowerCase() ?? "";
    const members = (dir.membersOf.get(id) ?? []).map((m) => m.memberName.toLowerCase());
    return name.includes(q) || members.some((n) => n.includes(q));
  };

  const firsts = [...byPerson.keys()]
    .map((id) => ({ id, at: byPerson.get(id)![0]!.at }))
    .sort((a, b) => a.at - b.at || (dir.byId.get(a.id)?.name ?? "").localeCompare(dir.byId.get(b.id)?.name ?? "", "ja"));

  const rows: TimelineRow[] = [];
  const connectors: Connector[] = [];
  const placed = new Set<number>();
  for (const { id } of firsts) {
    if (placed.has(id)) continue;
    // 自己是某个乐队的成员、而那个乐队也在时间轴上：等乐队那一行来带他
    const ownBands = dir.bandsOf.get(id) ?? [];
    if (ownBands.some((b) => byPerson.has(b.bandId) && !placed.has(b.bandId) && wanted(b.bandId))) continue;
    if (!wanted(id)) continue;
    const row = makeRow(id, null);
    if (row === null) continue;
    placed.add(id);
    const bandIndex = rows.length;
    rows.push(row);
    for (const m of dir.membersOf.get(id) ?? []) {
      if (placed.has(m.memberId)) continue;
      const solo = makeRow(m.memberId, id);
      if (solo === null) continue;
      placed.add(m.memberId);
      const soloIndex = rows.length;
      rows.push(solo);
      // 前成员：从离开（或乐队结束）那一刻连到他之后的第一张
      if (m.leftAt) {
        const left = yearValue(m.leftAt) ?? row.endedAt;
        const after = left === null ? undefined : solo.releases.find((r) => r.at >= left - 0.01);
        if (left !== null && after !== undefined) {
          connectors.push({ fromRow: bandIndex, toRow: soloIndex, fromAt: Math.min(left, row.end), toAt: after.at });
        }
      }
    }
  }
  // 被乐队「预约」了、乐队却被筛掉的成员：补上
  for (const { id } of firsts) {
    if (placed.has(id) || !wanted(id)) continue;
    const row = makeRow(id, null);
    if (row !== null) {
      placed.add(id);
      rows.push(row);
    }
  }

  const all = rows.flatMap((r) => [r.start, r.end]);
  return {
    rows,
    connectors,
    min: all.length ? Math.floor(Math.min(...all)) : 2000,
    max: all.length ? Math.ceil(Math.max(...all)) : 2001,
  };
}
