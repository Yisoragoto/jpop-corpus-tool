import { describe, expect, it } from "vitest";

import type { DirectoryEntry, Membership, TimelineRelease } from "./api";
import { buildDirectory } from "./libraryIndex";
import { buildTimeline, buildTree, placeCovers, yearValue } from "./timeline";

const e = (id: number, name: string, extra: Partial<DirectoryEntry> = {}): DirectoryEntry => ({
  id, name, imagePath: "", artistType: "", ended: "", imageManual: false, performer: 1, composer: 0, lyricist: 0, arranger: 0, songs: 1, firstYear: "", lastYear: "", ...extra,
});
const rel = (personId: number, date: string, title = date): TimelineRelease => ({ personId, albumId: null, title, date, tracks: 1, coverPath: "", songId: title });

const kinoko = e(1, "きのこ帝国", { ended: "2019-05-27", artistType: "Group" });
const sato = e(2, "佐藤千亜妃");
const early = e(3, "AKG");
const ms: Membership[] = [
  { bandId: 1, bandName: "きのこ帝国", memberId: 2, memberName: "佐藤千亜妃", part: "主唱", joined: "2007", leftAt: "2019-05-27", source: "musicbrainz", bandImage: "", memberImage: "" },
];

describe("yearValue", () => {
  it("turns dates into fractional years", () => {
    expect(yearValue("2014")).toBe(2014.5);
    expect(yearValue("2014-01-01")).toBe(2014);
    expect(yearValue("2014-07")).toBeCloseTo(2014.54, 1);
    expect(yearValue("?")).toBeNull();
  });
});

describe("buildTimeline", () => {
  const dir = buildDirectory([kinoko, sato, early], ms);
  // 佐藤千亜妃 的个人作品早在 2012 就有一首（比 AKG 的还早也不该跑到乐队前面去）
  const releases = [rel(3, "2003"), rel(1, "2012-01-01"), rel(1, "2016"), rel(2, "2010"), rel(2, "2020-03-01")];

  it("puts a member's solo row right under the band", () => {
    const t = buildTimeline(dir, releases);
    expect(t.rows.map((r) => [r.name, r.under])).toEqual([
      ["AKG", null],
      ["きのこ帝国", null],
      ["佐藤千亜妃", 1],
    ]);
  });

  it("runs the band lane to its end and links the end to the first solo release after", () => {
    const t = buildTimeline(dir, releases);
    const band = t.rows[1]!;
    expect(band.end).toBeCloseTo(2019.4, 1);
    expect(t.connectors).toHaveLength(1);
    expect(t.connectors[0]!.toAt).toBeCloseTo(2020.17, 1);
    expect([t.min, t.max]).toEqual([2003, 2021]);
  });

  it("keeps the band when filtering by a member's name", () => {
    const t = buildTimeline(dir, releases, "佐藤");
    expect(t.rows.map((r) => r.name)).toEqual(["きのこ帝国", "佐藤千亜妃"]);
  });
});

describe("placeCovers", () => {
  it("staggers close covers and folds the rest into +N", () => {
    const spots = placeCovers([
      { x: 0, size: 30 },
      { x: 10, size: 30 },
      { x: 20, size: 30 },
      { x: 25, size: 30 },
      { x: 200, size: 30 },
    ]);
    expect(spots.map((s) => [s.index, s.tier])).toEqual([
      [0, 0],
      [1, -1],
      [2, 1],
      [4, 0],
    ]);
    expect(spots[0]!.extra).toEqual([3]);
  });
});

describe("buildTree", () => {
  const dir = buildDirectory([kinoko, sato, early], ms);
  const work = (id: string, performer: number, albumId: number | null) =>
    ({ id, title: id, artist: "", album: "", albumId, year: "", genre: "", audioPath: "", coverPath: "", durationSec: null, lineCount: 0, roles: ["performer"], performerIds: [performer] });
  const releases = [
    { ...rel(1, "2014-10-08", "フェイク"), albumId: 7, tracks: 2 },
    rel(2, "2018-06-27", "空から"),
    { ...rel(2, "2020-11-25", "PLANET"), albumId: 9, tracks: 1 },
  ];
  const works = new Map([
    [1, [work("東京", 1, 7), work("クロノスタシス", 1, 7)]],
    [2, [work("空から", 2, null), work("カタワレ", 2, 9)]],
  ]);

  it("continues a split band with the member's later releases only", () => {
    const parts = buildTree(1, dir, releases, works);
    expect(parts.map((p) => [p.name, p.after?.bandName ?? null, p.items.map((i) => i.release.title)])).toEqual([
      ["きのこ帝国", null, ["フェイク"]],
      ["佐藤千亜妃", "きのこ帝国", ["PLANET"]],
    ]);
    expect(parts[0]!.items[0]!.songs.map((w) => w.id)).toEqual(["東京", "クロノスタシス"]);
    expect(parts[1]!.items[0]!.songs.map((w) => w.id)).toEqual(["カタワレ"]);
  });

  it("gives a solo artist one part with all releases", () => {
    const parts = buildTree(2, dir, releases, works);
    expect(parts).toHaveLength(1);
    expect(parts[0]!.items.map((i) => i.release.title)).toEqual(["空から", "PLANET"]);
    expect(parts[0]!.items[0]!.songs.map((w) => w.id)).toEqual(["空から"]);
  });
});
