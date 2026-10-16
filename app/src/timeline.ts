/**
 * 时间轴：每位演唱者一行（泳道），发行按日期落在横轴上。
 *
 * 行的顺序按第一次发行排，但**单飞的前成员紧跟在他的乐队下面**，
 * 乐队结束那一点画一道虚线连到他个人的第一张——きのこ帝国 2019 解散，
 * 下一行就是 佐藤千亜妃，线从 2019 接到她 2020 的第一张。
 */

import type { PersonWork, TimelineRelease } from "./api";
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

/* ── 总览里的封面：挨得太近的错开上下，再挤的合成「+N」 ── */

export interface CoverSpot {
  /** 在原数组里的下标 */
  index: number;
  cx: number;
  /** 0 = 在线上，-1 / 1 = 往上 / 往下错开半格 */
  tier: -1 | 0 | 1;
  /** 合进这一张的还有几张（显示「+N」） */
  extra: number[];
}

/**
 * 按时间先后放封面。每张先试线上，撞了试上一层、再试下一层；三层都撞了就并进线上最近那张，记成「+N」。
 * `size` 是封面边长，两张之间至少留 `gap`。
 */
export function placeCovers(items: { x: number; size: number }[], gap = 4): CoverSpot[] {
  const spots: CoverSpot[] = [];
  const lastByTier = new Map<number, { x: number; size: number; spot: CoverSpot }>();
  items.forEach((it, index) => {
    for (const tier of [0, -1, 1] as const) {
      const last = lastByTier.get(tier);
      if (last === undefined || it.x - last.x >= (last.size + it.size) / 2 + gap) {
        const spot: CoverSpot = { index, cx: it.x, tier, extra: [] };
        spots.push(spot);
        lastByTier.set(tier, { x: it.x, size: it.size, spot });
        return;
      }
    }
    lastByTier.get(0)!.spot.extra.push(index);
  });
  return spots;
}

/* ── 放大后的时间树：一位音乐人的发行，乐队解散后接上单飞的成员 ── */

export interface TreeRelease {
  release: TimelineRelease & { at: number };
  songs: PersonWork[];
  /** 这一张是谁的（乐队解散后接上的成员作品，和树的主人不是同一个人） */
  personId: number;
}

export interface TreePart {
  personId: number;
  name: string;
  /** 乐队解散后接上的那一段：前面画「解散」那张卡片 */
  after: { bandName: string; ended: string } | null;
  items: TreeRelease[];
}

/** 这个人在这张发行里唱的歌：有专辑按专辑，单曲就是那一首 */
function songsOf(release: TimelineRelease, works: PersonWork[]): PersonWork[] {
  const own = works.filter((w) => w.performerIds.includes(release.personId));
  return release.albumId !== null ? own.filter((w) => w.albumId === release.albumId) : own.filter((w) => w.id === release.songId);
}

/**
 * 一棵树的几段：先是这个人自己的发行；他是乐队、而且解散了，再接上离开时间在那之后、有个人发行的前成员，
 * 只取解散之后的那些（之前的个人作品属于另一棵树）。`worksOf` 给每个人的作品（拿歌名）。
 */
export function buildTree(
  personId: number,
  dir: Directory,
  releases: TimelineRelease[],
  worksOf: Map<number, PersonWork[]>,
): TreePart[] {
  const entry = dir.byId.get(personId);
  const withAt = (r: TimelineRelease) => ({ ...r, at: yearValue(r.date) ?? 0 });
  const mine = releases
    .filter((r) => r.personId === personId && yearValue(r.date) !== null)
    .map(withAt)
    .sort((a, b) => a.at - b.at);
  const parts: TreePart[] = [
    {
      personId,
      name: entry?.name ?? "",
      after: null,
      items: mine.map((release) => ({ release, songs: songsOf(release, worksOf.get(personId) ?? []), personId })),
    },
  ];
  const ended = entry?.ended ? yearValue(entry.ended) : null;
  if (ended !== null && entry) {
    for (const m of dir.membersOf.get(personId) ?? []) {
      if (!m.leftAt) continue;
      const later = releases
        .filter((r) => r.personId === m.memberId && (yearValue(r.date) ?? 0) >= ended - 0.01)
        .map(withAt)
        .sort((a, b) => a.at - b.at);
      if (later.length === 0) continue;
      parts.push({
        personId: m.memberId,
        name: m.memberName,
        after: { bandName: entry.name, ended: entry.ended },
        items: later.map((release) => ({ release, songs: songsOf(release, worksOf.get(m.memberId) ?? []), personId: m.memberId })),
      });
    }
  }
  return parts;
}
