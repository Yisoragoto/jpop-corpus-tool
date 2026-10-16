import { describe, expect, it } from "vitest";

import type { DirectoryEntry, Membership } from "./api";
import {
  buildDirectory,
  filterBehind,
  filterMusicians,
  memberSubtitle,
  sectionWorks,
  musicianSubtitle,
  spanOf,
  validDate,
  yearOf,
} from "./libraryIndex";

function person(id: number, name: string, roles: Partial<DirectoryEntry> = {}): DirectoryEntry {
  return {
    id,
    name,
    imagePath: "",
    artistType: "",
    ended: "",
    imageManual: false,
    performer: 0,
    composer: 0,
    lyricist: 0,
    arranger: 0,
    songs: 0,
    firstYear: "",
    lastYear: "",
    ...roles,
  };
}

function member(band: DirectoryEntry, who: { id: number; name: string }, extra: Partial<Membership> = {}): Membership {
  return {
    bandId: band.id,
    bandName: band.name,
    memberId: who.id,
    memberName: who.name,
    part: "",
    joined: "",
    leftAt: "",
    source: "musicbrainz",
    bandImage: "",
    memberImage: "",
    ...extra,
  };
}

const kinoko = person(1, "きのこ帝国", { performer: 19, songs: 19, ended: "2019-05-27", artistType: "Group" });
const sato = person(2, "佐藤千亜妃", { performer: 6, composer: 25, lyricist: 25, songs: 25 });
const nishi = person(3, "西村ヒロ", { arranger: 4, songs: 4 });
const yorushika = person(4, "ヨルシカ", { performer: 30, songs: 30 });
const nbuna = person(5, "n-buna", { composer: 30, lyricist: 30, arranger: 30, songs: 30 });
const memberships = [
  member(kinoko, sato, { part: "主唱", joined: "2007", leftAt: "2019-05-27" }),
  member(kinoko, { id: 9, name: "あーちゃん" }, { part: "吉他", joined: "2007", leftAt: "2019" }),
  member(yorushika, nbuna, { part: "作词作曲", joined: "2017" }),
];

describe("buildDirectory", () => {
  const dir = buildDirectory([kinoko, sato, nishi, yorushika, nbuna], memberships);

  it("lists only people with their own performer credits at the top level", () => {
    // 佐藤千亜妃 有自己演唱署名的歌，顶层有她；n-buna 只写不唱，只挂在 ヨルシカ 下面
    expect(dir.musicians.map((m) => m.entry.name)).toEqual(["ヨルシカ", "きのこ帝国", "佐藤千亜妃"]);
  });

  it("nests members under their band, current members first", () => {
    const band = dir.musicians.find((m) => m.entry.id === kinoko.id)!;
    expect(band.isBand).toBe(true);
    expect(band.members.map((m) => m.membership.memberName)).toEqual(["あーちゃん", "佐藤千亜妃"]);
    // 只在成员表里、没有署名的人也挂得上，只是没有目录行
    expect(band.members[0]!.entry).toBeNull();
  });

  it("keeps band members out of the behind-the-scenes list", () => {
    expect(dir.behind.map((e) => e.name)).toEqual(["西村ヒロ"]);
  });

  it("describes a solo career after the band", () => {
    const solo = dir.musicians.find((m) => m.entry.id === sato.id)!;
    expect(solo.isBand).toBe(false);
    expect(musicianSubtitle(solo)).toBe("个人 · 6 首 · 曾属 きのこ帝国");
    const band = dir.musicians.find((m) => m.entry.id === kinoko.id)!;
    expect(musicianSubtitle(band)).toBe("乐队 · 19 首 · 2019 解散");
    expect(memberSubtitle(band.members[1]!)).toBe("前成员 · 2007–2019");
  });
});

describe("filters", () => {
  const dir = buildDirectory([kinoko, sato, nishi, yorushika, nbuna], memberships);

  it("brings the band along when a member's name matches", () => {
    const hits = filterMusicians(dir.musicians, "n-bu");
    expect(hits.map((h) => [h.row.entry.name, h.memberHit])).toEqual([["ヨルシカ", true]]);
  });

  it("filters behind-the-scenes people by role", () => {
    expect(filterBehind(dir.behind, "arranger", "")).toHaveLength(1);
    expect(filterBehind(dir.behind, "composer", "")).toHaveLength(0);
  });
});

describe("dates", () => {
  it("keeps only the year and says so when still active", () => {
    expect(yearOf("2019-05-27")).toBe("2019");
    expect(spanOf("2017", "")).toBe("2017–至今");
    expect(spanOf("", "?")).toBe("?–?");
  });

  it("accepts only year-like dates for members", () => {
    for (const ok of ["", "?", "2019", "2019-05", "2019-05-27"]) expect(validDate(ok)).toBe(true);
    for (const bad of ["19", "2019/05", "去年", "2019-5"]) expect(validDate(bad)).toBe(false);
  });
});

describe("sectionWorks", () => {
  const work = (id: string, performerIds: number[], roles: string[] = []) =>
    ({ id, title: id, artist: "", album: "", albumId: null, year: "", genre: "", audioPath: "", coverPath: "", durationSec: null, lineCount: 0, roles, performerIds });
  const band = memberships[0]!; // 佐藤千亜妃 在 きのこ帝国，2007–2019

  it("puts solo songs first and folds the band era away", () => {
    const got = sectionWorks(sato.id, [work("k1", [kinoko.id], ["lyricist"]), work("s1", [sato.id], ["performer"]), work("x1", [99], ["composer"])], [band]);
    expect(got.map((s) => [s.title, s.collapsed, s.works.map((w) => w.id).join()])).toEqual([
      ["个人作品", false, "s1"],
      ["在 きのこ帝国 时", true, "k1"],
      ["合作 / 幕后", false, "x1"],
    ]);
    expect(got[1]!.note).toBe("2007–2019 · 这些歌属于乐队，其中 1 首有署名");
  });

  it("opens the band era when that is all there is", () => {
    const got = sectionWorks(sato.id, [work("k1", [kinoko.id])], [band]);
    expect(got).toHaveLength(1);
    expect(got[0]!.collapsed).toBe(false);
    expect(got[0]!.title).toBe("");
  });
});
