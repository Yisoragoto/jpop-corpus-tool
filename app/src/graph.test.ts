import { describe, expect, it } from "vitest";

import type { DirectoryEntry, Membership } from "./api";
import { buildGraph, nodeAt, Simulation } from "./graph";

function entry(id: number, name: string, extra: Partial<DirectoryEntry> = {}): DirectoryEntry {
  return { id, name, imagePath: "", artistType: "", ended: "", imageManual: false, performer: 0, composer: 0, lyricist: 0, arranger: 0, songs: 1, firstYear: "", lastYear: "", ...extra };
}

const people = [
  entry(1, "ヨルシカ", { performer: 10, songs: 10, artistType: "Group" }),
  entry(2, "n-buna", { composer: 10, songs: 10 }),
  entry(3, "suis", { performer: 1, songs: 1 }),
  entry(4, "西村ヒロ", { arranger: 3, songs: 3 }),
  entry(5, "Vaundy", { performer: 5, songs: 5 }),
];
const edges = [
  { source: 1, target: 2, shared: 10 },
  { source: 2, target: 4, shared: 2 },
  { source: 4, target: 5, shared: 3 },
];
const ms: Membership[] = [
  { bandId: 1, bandName: "ヨルシカ", memberId: 3, memberName: "suis", part: "主唱", joined: "2017", leftAt: "", source: "musicbrainz", bandImage: "", memberImage: "" },
];

describe("buildGraph", () => {
  it("adds membership edges even without shared credits, marked as members", () => {
    const g = buildGraph(people, edges, ms);
    const member = g.links.find((l) => l.member)!;
    expect([g.nodes[member.s]!.name, g.nodes[member.t]!.name].sort()).toEqual(["suis", "ヨルシカ"]);
    expect(g.nodes.find((n) => n.id === 1)!.isBand).toBe(true);
  });

  it("walks the given number of steps from the focus", () => {
    const one = buildGraph(people, edges, ms, { focus: 2, depth: 1 });
    expect(one.nodes.map((n) => n.name).sort()).toEqual(["n-buna", "ヨルシカ", "西村ヒロ"]);
    const two = buildGraph(people, edges, ms, { focus: 2, depth: 2 });
    expect(two.nodes.map((n) => n.name).sort()).toEqual(["Vaundy", "n-buna", "suis", "ヨルシカ", "西村ヒロ"]);
  });

  it("drops thin edges and filtered roles", () => {
    const g = buildGraph(people, edges, ms, { minShared: 3, roles: new Set(["performer"]) });
    // 西村ヒロ 是编曲：筛掉；n-buna 是作曲：筛掉
    expect(g.nodes.map((n) => n.name).sort()).toEqual(["Vaundy", "suis", "ヨルシカ"]);
  });
});

describe("Simulation", () => {
  it("settles linked nodes closer than unlinked ones and stops", () => {
    const g = buildGraph(people, edges, ms);
    const sim = new Simulation(g);
    sim.settle();
    expect(sim.running).toBe(false);
    const pos = (id: number) => g.nodes.find((n) => n.id === id)!;
    const d = (a: number, b: number) => Math.hypot(pos(a).x - pos(b).x, pos(a).y - pos(b).y);
    expect(d(1, 2)).toBeLessThan(d(1, 5));
    for (const n of g.nodes) expect(Number.isFinite(n.x) && Number.isFinite(n.y)).toBe(true);
  });

  it("finds the node under the pointer", () => {
    const g = buildGraph(people, edges, ms);
    const n = g.nodes[0]!;
    expect(nodeAt(g.nodes, n.x + 1, n.y)).toBe(0);
    expect(nodeAt(g.nodes, n.x + 500, n.y + 500)).toBe(-1);
  });
});

it("settled collaboration portraits have physical separation and capped hit regions", () => {
  const entries=Array.from({length:18},(_,i)=>entry(i,`Person ${i}`,{performer:1,songs:i+1}));
  const g=buildGraph(entries,entries.slice(1).map(e=>({source:0,target:e.id,shared:1})),[]);
  new Simulation(g).settle();
  for(let i=0;i<g.nodes.length;i++) for(let j=i+1;j<g.nodes.length;j++) {
    const a=g.nodes[i]!,b=g.nodes[j]!;
    expect(Math.hypot(a.x-b.x,a.y-b.y)).toBeGreaterThan(a.r+b.r+4);
  }
  const a=g.nodes[0]!;
  expect(nodeAt([a],a.x+10,a.y,0,5)).toBe(-1);
});
