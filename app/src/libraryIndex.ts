/**
 * 资料库左栏的分法：音乐人（乐队和个人，成员收在乐队下面）、幕后。
 *
 * 规则和 `jp_corpus::members` 一致：**一首歌属于谁只看演唱署名**。
 * 佐藤千亜妃 给 きのこ帝国 写的歌只算乐队的；她自己演唱署名的歌才让她以个人身份出现在顶层。
 * 所以一个人在顶层出现，只因为她有自己的演唱署名——成员关系只决定她还挂在哪个乐队下面。
 */

import type { DirectoryEntry, Membership, PersonWork } from "./api";

export type BehindRole = "all" | "composer" | "lyricist" | "arranger";

export const BEHIND_ROLES: { key: BehindRole; label: string }[] = [
  { key: "all", label: "全部" },
  { key: "composer", label: "作曲" },
  { key: "lyricist", label: "作词" },
  { key: "arranger", label: "编曲" },
];

/** 日期只要年份：YYYY、YYYY-MM、YYYY-MM-DD 都行；`?`（知道离开了、不知道哪年）原样留着 */
export function yearOf(date: string): string {
  const m = /^\d{4}/.exec(date.trim());
  return m ? m[0] : date.trim();
}

/** 成员的加入、离开日期只收 YYYY、YYYY-MM、YYYY-MM-DD，或者「?」（知道离开了、不知道哪年）；空着也行 */
export function validDate(value: string): boolean {
  return value === "" || value === "?" || /^\d{4}(-\d{2}(-\d{2})?)?$/.test(value);
}

/** 「2007–2019」「2017–至今」「?–2019」 */
export function spanOf(joined: string, leftAt: string): string {
  const from = yearOf(joined) || "?";
  return `${from}–${leftAt ? yearOf(leftAt) : "至今"}`;
}

export interface MemberRow {
  membership: Membership;
  /** 成员本人在目录里的那一行；只在成员表里（没有任何署名）的人没有 */
  entry: DirectoryEntry | null;
  former: boolean;
}

export interface MusicianRow {
  entry: DirectoryEntry;
  /** 这一位是乐队：有成员，或者刮削认出来是 Group */
  isBand: boolean;
  members: MemberRow[];
  /** 这个人待过的乐队（以个人身份出现在顶层时，副标题写「曾属 X」） */
  bands: Membership[];
}

/** 现役在前、按加入先后；前成员在后、按离开先后 */
function memberOrder(a: Membership, b: Membership): number {
  const fa = a.leftAt !== "";
  const fb = b.leftAt !== "";
  if (fa !== fb) return fa ? 1 : -1;
  const key = (m: Membership) => (fa ? m.leftAt : m.joined) || "9999";
  return key(a).localeCompare(key(b)) || a.memberName.localeCompare(b.memberName, "ja");
}

export interface Directory {
  musicians: MusicianRow[];
  /** 只写不唱、也不在任何乐队里的人 */
  behind: DirectoryEntry[];
  byId: Map<number, DirectoryEntry>;
  /** 成员 id → 待过的乐队 */
  bandsOf: Map<number, Membership[]>;
  /** 乐队 id → 成员 */
  membersOf: Map<number, Membership[]>;
}

export function buildDirectory(entries: DirectoryEntry[], memberships: Membership[]): Directory {
  const byId = new Map(entries.map((e) => [e.id, e]));
  const membersOf = new Map<number, Membership[]>();
  const bandsOf = new Map<number, Membership[]>();
  for (const m of memberships) {
    (membersOf.get(m.bandId) ?? membersOf.set(m.bandId, []).get(m.bandId)!).push(m);
    (bandsOf.get(m.memberId) ?? bandsOf.set(m.memberId, []).get(m.memberId)!).push(m);
  }
  for (const list of membersOf.values()) list.sort(memberOrder);

  const musicians = entries
    .filter((e) => e.performer > 0)
    .map((entry): MusicianRow => {
      const members = (membersOf.get(entry.id) ?? []).map((membership) => ({
        membership,
        entry: byId.get(membership.memberId) ?? null,
        former: membership.leftAt !== "",
      }));
      return {
        entry,
        isBand: members.length > 0 || entry.artistType === "Group",
        members,
        bands: bandsOf.get(entry.id) ?? [],
      };
    })
    .sort((a, b) => b.entry.performer - a.entry.performer || a.entry.name.localeCompare(b.entry.name, "ja"));

  const behind = entries
    .filter((e) => e.performer === 0 && !bandsOf.has(e.id))
    .sort((a, b) => b.songs - a.songs || a.name.localeCompare(b.name, "ja"));

  return { musicians, behind, byId, bandsOf, membersOf };
}

function matches(name: string, q: string): boolean {
  return name.toLowerCase().includes(q);
}

export interface MusicianHit {
  row: MusicianRow;
  /** 筛选命中了成员名字：乐队自动展开 */
  memberHit: boolean;
}

/** 按名字筛音乐人；搜成员名字时把乐队也带出来 */
export function filterMusicians(rows: MusicianRow[], query: string): MusicianHit[] {
  const q = query.trim().toLowerCase();
  if (q === "") return rows.map((row) => ({ row, memberHit: false }));
  const out: MusicianHit[] = [];
  for (const row of rows) {
    const memberHit = row.members.some((m) => matches(m.membership.memberName, q));
    if (matches(row.entry.name, q) || memberHit) out.push({ row, memberHit });
  }
  return out;
}

export function roleCount(entry: DirectoryEntry, role: BehindRole): number {
  return role === "all" ? entry.songs : entry[role];
}

export function filterBehind(entries: DirectoryEntry[], role: BehindRole, query: string): DirectoryEntry[] {
  const q = query.trim().toLowerCase();
  return entries
    .filter((e) => roleCount(e, role) > 0 && (q === "" || matches(e.name, q)))
    .sort((a, b) => roleCount(b, role) - roleCount(a, role) || a.name.localeCompare(b.name, "ja"));
}

/** 幕后的人做过哪些角色，按作品多的在前 */
export function rolesOf(entry: DirectoryEntry): ("composer" | "lyricist" | "arranger" | "performer")[] {
  const all = (["performer", "composer", "lyricist", "arranger"] as const).filter((r) => entry[r] > 0);
  return [...all].sort((a, b) => entry[b] - entry[a]);
}

/** 音乐人一行的副标题 */
export function musicianSubtitle(row: MusicianRow): string {
  const { entry } = row;
  if (row.isBand) {
    const ended = entry.ended ? ` · ${yearOf(entry.ended)} 解散` : "";
    return `乐队 · ${entry.performer} 首${ended}`;
  }
  const band = row.bands[0];
  if (band !== undefined) {
    const was = band.leftAt !== "" ? "曾属" : "也在";
    return `个人 · ${entry.performer} 首 · ${was} ${band.bandName}`;
  }
  return `${entry.performer} 首`;
}

/** 成员一行的副标题：前成员写年份段，现役写分工 */
export function memberSubtitle(m: MemberRow): string {
  const { membership } = m;
  if (m.former) return `前成员 · ${spanOf(membership.joined, membership.leftAt)}`;
  return membership.part || "成员";
}

/** 专辑的署名里取第一位（`A/B` 这种合作专辑归到 A） */
export function albumOwner(albumArtist: string): string {
  return albumArtist.split("/")[0]?.trim() ?? "";
}

/**
 * 资料库中间那一栏在看什么：一首歌、一个人（可以带着要定位的专辑）、或者一张对不上人的专辑。
 * 歌本身存在 `useLibrary` 的 `selected` 里，这里只记「现在看的是歌」。
 */
export type LibraryView =
  | { kind: "song" }
  | { kind: "person"; id: number; albumId?: number | null }
  | { kind: "album"; albumId: number; title: string }
  /** 全局关系网；focus = 打开时选中的人 */
  | { kind: "graph"; focus?: number | null }
  | { kind: "timeline" }
  /** 词云（筛选记在 CloudView 的模块里） */
  | { kind: "cloud" };

export function sameView(a: LibraryView, b: LibraryView): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "person" && b.kind === "person") return a.id === b.id;
  if (a.kind === "album" && b.kind === "album") return a.albumId === b.albumId;
  return true;
}

export interface WorkSection {
  key: string;
  /** 只有一区时不写标题 */
  title: string;
  note: string;
  /** 默认收起（「在 X 时」：那些歌属于乐队，放着备查） */
  collapsed: boolean;
  works: PersonWork[];
}

/**
 * 档案页的作品分区：自己演唱的 → 在各个乐队时（乐队演唱的）→ 合作 / 幕后（给别人写、编的）。
 *
 * 只有一区时不要标题；有自己的作品时「在 X 时」默认收起——佐藤千亜妃 的页面先看到的是她自己的歌，
 * きのこ帝国 的歌在下面点开才有。现役成员、自己没发过歌的（suis），乐队那一区就直接展开。
 */
export function sectionWorks(personId: number, works: PersonWork[], bands: Membership[]): WorkSection[] {
  const own = works.filter((w) => w.performerIds.includes(personId));
  const taken = new Set(own.map((w) => w.id));
  const bandSections = bands.map((b) => {
    const list = works.filter((w) => !taken.has(w.id) && w.performerIds.includes(b.bandId));
    for (const w of list) taken.add(w.id);
    return { band: b, list };
  });
  const rest = works.filter((w) => !taken.has(w.id));
  const hasOther = own.length > 0 || rest.length > 0;

  const sections: WorkSection[] = [];
  if (own.length > 0) {
    sections.push({ key: "own", title: bands.length > 0 ? "个人作品" : "作品", note: "", collapsed: false, works: own });
  }
  for (const { band, list } of bandSections) {
    if (list.length === 0) continue;
    const wrote = list.filter((w) => w.roles.length > 0).length;
    sections.push({
      key: `band-${band.bandId}`,
      title: `在 ${band.bandName} 时`,
      note: `${spanOf(band.joined, band.leftAt)} · 这些歌属于乐队${wrote > 0 ? `，其中 ${wrote} 首有署名` : ""}`,
      collapsed: hasOther,
      works: list,
    });
  }
  if (rest.length > 0) {
    sections.push({
      key: "credits",
      title: own.length > 0 || bandSections.some((b) => b.list.length > 0) ? "合作 / 幕后" : "作品",
      note: "",
      collapsed: false,
      works: rest,
    });
  }
  if (sections.length === 1) sections[0]!.title = "";
  return sections;
}
