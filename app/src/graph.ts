/**
 * 关系网：从目录 + 合作边 + 成员关系搭出图，再用一个小的力导向模拟摆位置。
 *
 * 不引 d3：要的只是「互相推开、连线拉近、往中心收」三种力，一百来行就够，
 * 还能单测。节点几百个，斥力按格子分桶只算附近的，一帧不到一毫秒。
 */

import type { CollabEdge, DirectoryEntry, Membership } from "./api";

export interface GraphNode {
  id: number;
  name: string;
  imagePath: string;
  /** 作品最多的那个角色，决定颜色 */
  role: string;
  isBand: boolean;
  songs: number;
  r: number;
  x: number;
  y: number;
  vx: number;
  vy: number;
  /** 拖着的时候钉住 */
  fx: number | null;
  fy: number | null;
  /** 离焦点几步（局部图用；全局图都是 0） */
  depth: number;
}

export interface GraphLink {
  /** nodes 里的下标 */
  s: number;
  t: number;
  shared: number;
  /** 乐队 ↔ 成员：画虚线 */
  member: boolean;
}

export interface Graph {
  nodes: GraphNode[];
  links: GraphLink[];
  /** 每个节点的邻居（下标） */
  neighbors: number[][];
}

const ROLE_ORDER = ["performer", "composer", "lyricist", "arranger"] as const;

function mainRole(e: DirectoryEntry): string {
  let best: string = "performer";
  let n = -1;
  for (const r of ROLE_ORDER) {
    if (e[r] > n) {
      n = e[r];
      best = r;
    }
  }
  return best;
}

export function radiusFor(songs: number): number {
  return Math.min(22, 4 + Math.sqrt(Math.max(0, songs)) * 1.7);
}

export interface BuildOptions {
  /** 局部图：从这个人出发 */
  focus?: number;
  /** 局部图走几步，默认 1 */
  depth?: number;
  /** 只留这些角色为主的人（焦点和乐队成员总是留着） */
  roles?: Set<string>;
  /** 一起署名少于这么多首的边不画 */
  minShared?: number;
}

/**
 * 搭图。成员关系也是边（没一起署过名的乐队和成员照样连上），画成虚线。
 * 局部图按边做广度优先，走 `depth` 步。
 */
export function buildGraph(
  entries: DirectoryEntry[],
  edges: CollabEdge[],
  memberships: Membership[],
  opts: BuildOptions = {},
): Graph {
  const byId = new Map(entries.map((e) => [e.id, e]));
  const minShared = opts.minShared ?? 1;
  const bands = new Set(memberships.map((m) => m.bandId));
  for (const e of entries) if (e.artistType === "Group") bands.add(e.id);

  // 边：合作 + 成员。同一对两样都有时算成员边，粗细按合作数
  const pairs = new Map<string, { a: number; b: number; shared: number; member: boolean }>();
  const key = (a: number, b: number) => (a < b ? `${a}-${b}` : `${b}-${a}`);
  for (const e of edges) {
    if (e.shared < minShared) continue;
    pairs.set(key(e.source, e.target), { a: e.source, b: e.target, shared: e.shared, member: false });
  }
  for (const m of memberships) {
    const k = key(m.bandId, m.memberId);
    const old = pairs.get(k);
    pairs.set(k, { a: m.bandId, b: m.memberId, shared: old?.shared ?? 0, member: true });
  }

  const adjacency = new Map<number, number[]>();
  for (const p of pairs.values()) {
    (adjacency.get(p.a) ?? adjacency.set(p.a, []).get(p.a)!).push(p.b);
    (adjacency.get(p.b) ?? adjacency.set(p.b, []).get(p.b)!).push(p.a);
  }

  // 留哪些人
  const depthOf = new Map<number, number>();
  if (opts.focus !== undefined) {
    const maxDepth = opts.depth ?? 1;
    depthOf.set(opts.focus, 0);
    let frontier = [opts.focus];
    for (let d = 1; d <= maxDepth; d++) {
      const next: number[] = [];
      for (const id of frontier) {
        for (const n of adjacency.get(id) ?? []) {
          if (!depthOf.has(n)) {
            depthOf.set(n, d);
            next.push(n);
          }
        }
      }
      frontier = next;
    }
  } else {
    for (const e of entries) depthOf.set(e.id, 0);
    // 只在成员表里的人（没有署名）也要有节点
    for (const m of memberships) if (!depthOf.has(m.memberId)) depthOf.set(m.memberId, 0);
  }

  const memberIds = new Set(memberships.map((m) => m.memberId));
  const nodes: GraphNode[] = [];
  const index = new Map<number, number>();
  for (const [id, depth] of depthOf) {
    const e = byId.get(id);
    const m = memberships.find((x) => x.memberId === id || x.bandId === id);
    const name = e?.name ?? (m ? (m.memberId === id ? m.memberName : m.bandName) : "");
    if (name === "") continue;
    const role = e ? mainRole(e) : "performer";
    const keep =
      opts.roles === undefined || id === opts.focus || bands.has(id) || memberIds.has(id) || opts.roles.has(role);
    if (!keep) continue;
    index.set(id, nodes.length);
    nodes.push({
      id,
      name,
      imagePath: e?.imagePath || (m?.memberId === id ? m.memberImage : m?.bandImage) || "",
      role,
      isBand: bands.has(id),
      songs: e?.songs ?? 0,
      r: radiusFor(e?.songs ?? 0) + (id === opts.focus ? 3 : 0),
      x: 0,
      y: 0,
      vx: 0,
      vy: 0,
      fx: null,
      fy: null,
      depth,
    });
  }

  const links: GraphLink[] = [];
  for (const p of pairs.values()) {
    const s = index.get(p.a);
    const t = index.get(p.b);
    if (s === undefined || t === undefined) continue;
    links.push({ s, t, shared: p.shared, member: p.member });
  }

  // 全局图里一条边都没有的人太多会糊成一片：照样留着（Obsidian 也留孤立点），只是摆在外圈
  const neighbors: number[][] = nodes.map(() => []);
  for (const l of links) {
    neighbors[l.s]!.push(l.t);
    neighbors[l.t]!.push(l.s);
  }
  seedPositions(nodes, neighbors);
  return { nodes, links, neighbors };
}

/** 初始位置：向日葵螺线（和 d3 一样），确定的、不重叠；孤立点放外圈 */
function seedPositions(nodes: GraphNode[], neighbors: number[][]) {
  const golden = Math.PI * (3 - Math.sqrt(5));
  const order = nodes.map((_, i) => i).sort((a, b) => neighbors[b]!.length - neighbors[a]!.length);
  order.forEach((i, k) => {
    const r = 14 * Math.sqrt(k + 0.5) * (neighbors[i]!.length === 0 ? 1.6 : 1);
    nodes[i]!.x = r * Math.cos(k * golden);
    nodes[i]!.y = r * Math.sin(k * golden);
  });
}

export interface Forces {
  /** 互相推开的力度 */
  repel: number;
  /** 连线想要的长度 */
  linkDistance: number;
  /** 往中心收的力度 */
  center: number;
}

export const DEFAULT_FORCES: Forces = { repel: 900, linkDistance: 70, center: 0.035 };

/** 力导向模拟。alpha 从 1 慢慢降到 alphaMin 就停；拖动、改参数时 reheat */
export class Simulation {
  alpha = 1;
  readonly alphaMin = 0.003;
  private readonly decay = 1 - Math.pow(0.003, 1 / 300);
  private readonly friction = 0.58;

  constructor(
    private graph: Graph,
    public forces: Forces = DEFAULT_FORCES,
  ) {}

  get running(): boolean {
    return this.alpha > this.alphaMin;
  }

  reheat(to = 0.6) {
    this.alpha = Math.max(this.alpha, to);
  }

  tick() {
    const { nodes, links } = this.graph;
    const a = this.alpha;
    const { repel, linkDistance, center } = this.forces;

    // 连线：弹簧，合作越多拉得越近
    for (const l of links) {
      const s = nodes[l.s]!;
      const t = nodes[l.t]!;
      let dx = t.x + t.vx - s.x - s.vx;
      let dy = t.y + t.vy - s.y - s.vy;
      const d = Math.hypot(dx, dy) || 1;
      const want = linkDistance * (l.member ? 0.7 : 1) + s.r + t.r;
      const strength = Math.min(1, 0.25 + Math.log2(1 + l.shared) * 0.15);
      const k = ((d - want) / d) * a * strength * 0.5;
      dx *= k;
      dy *= k;
      t.vx -= dx;
      t.vy -= dy;
      s.vx += dx;
      s.vy += dy;
    }

    // 斥力：按格子分桶，只和附近九格里的算
    const cell = Math.sqrt(repel) * 4;
    const grid = new Map<string, number[]>();
    nodes.forEach((n, i) => {
      const k = `${Math.floor(n.x / cell)},${Math.floor(n.y / cell)}`;
      (grid.get(k) ?? grid.set(k, []).get(k)!).push(i);
    });
    nodes.forEach((n, i) => {
      const cx = Math.floor(n.x / cell);
      const cy = Math.floor(n.y / cell);
      for (let gx = cx - 1; gx <= cx + 1; gx++) {
        for (let gy = cy - 1; gy <= cy + 1; gy++) {
          for (const j of grid.get(`${gx},${gy}`) ?? []) {
            if (j <= i) continue;
            const m = nodes[j]!;
            let dx = m.x - n.x;
            let dy = m.y - n.y;
            let d2 = dx * dx + dy * dy;
            if (d2 === 0) {
              // 叠在一起：按下标推开一点，确定的方向
              dx = ((i * 7919) % 13) - 6 || 1;
              dy = ((j * 104729) % 13) - 6 || 1;
              d2 = dx * dx + dy * dy;
            }
            const minD = n.r + m.r + 4;
            const f = (repel * a) / Math.max(d2, minD * minD * 0.25);
            const d = Math.sqrt(d2);
            const fx = (dx / d) * f;
            const fy = (dy / d) * f;
            n.vx -= fx;
            n.vy -= fy;
            m.vx += fx;
            m.vy += fy;
          }
        }
      }
    });

    for (const n of nodes) {
      n.vx -= n.x * center * a;
      n.vy -= n.y * center * a;
      if (n.fx !== null && n.fy !== null) {
        n.x = n.fx;
        n.y = n.fy;
        n.vx = 0;
        n.vy = 0;
        continue;
      }
      n.vx *= this.friction;
      n.vy *= this.friction;
      n.x += n.vx;
      n.y += n.vy;
    }
    this.alpha += (0 - this.alpha) * this.decay;
  }

  /** 一口气跑完（减少动态效果时直接摆好） */
  settle(maxTicks = 400) {
    for (let i = 0; i < maxTicks && this.running; i++) this.tick();
  }
}

/** 图坐标里离 (x, y) 最近、并且点在圆里（放宽 slop）的节点 */
export function nodeAt(nodes: GraphNode[], x: number, y: number, slop = 4): number {
  let best = -1;
  let bestD = Infinity;
  nodes.forEach((n, i) => {
    const d = Math.hypot(n.x - x, n.y - y);
    if (d <= n.r + slop && d < bestD) {
      best = i;
      bestD = d;
    }
  });
  return best;
}
