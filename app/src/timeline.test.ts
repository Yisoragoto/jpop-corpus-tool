import { describe, expect, it } from "vitest";

import type { DirectoryEntry, Membership, TimelineRelease } from "./api";
import { buildDirectory } from "./libraryIndex";
import { buildTimeline, yearValue } from "./timeline";

const e = (id: number, name: string, extra: Partial<DirectoryEntry> = {}): DirectoryEntry => ({
  id, name, imagePath: "", artistType: "", ended: "", performer: 1, composer: 0, lyricist: 0, arranger: 0, songs: 1, firstYear: "", lastYear: "", ...extra,
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
