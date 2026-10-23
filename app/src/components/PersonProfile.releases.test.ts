import { describe, expect, it } from "vitest";
import type { Track } from "../api";
import { releaseTitle, splitReleases, type AlbumGroup } from "./PersonProfile";

const track = (id: string, title: string, year = "2024"): Track => ({ id, title, year, coverPath: "" }) as unknown as Track;
const group = (key: string, title: string, tracks: Track[], year = "2024"): AlbumGroup => ({
  key,
  albumId: key.startsWith("id:") ? Number(key.slice(3)) : null,
  title,
  year,
  coverPath: "",
  tracks,
});

describe("概览：单曲和 EP / 专辑", () => {
  it("按标题后缀、同名小发行、没有专辑来认单曲，其余算专辑，都是最新在前", () => {
    const { singles, albums } = splitReleases([
      group("id:1", "二人称", [track("a", "1"), track("b", "2"), track("c", "3"), track("d", "4")], "2024"),
      group("id:2", "修羅 - Single", [track("e", "修羅")], "2025"),
      group("id:3", "あぶく", [track("f", "あぶく")], "2026"),
      group("id:4", "創作 - EP", [track("g", "春泥棒"), track("h", "強盗と花束")], "2021"),
      // 只收了一首的专辑：曲名和专辑名不同，仍算专辑
      group("id:5", "盗作", [track("i", "春ひさぎ")], "2020"),
      group("single", "单曲", [track("j", "老人と海", "2021"), track("k", "ただ君に晴れ", "2018")]),
    ]);
    expect(singles.map((g) => g.title)).toEqual(["あぶく", "修羅 - Single", "創作 - EP", "老人と海", "ただ君に晴れ"]);
    expect(albums.map((g) => g.title)).toEqual(["二人称", "盗作"]);
    // 没有专辑的歌各占一张，点进去还是那一组
    expect(singles.filter((g) => g.key === "single").map((g) => g.tracks.length)).toEqual([1, 1]);
  });

  it("单曲区的标题去掉「- Single / - EP」", () => {
    expect(releaseTitle("修羅 - Single")).toBe("修羅");
    expect(releaseTitle("創作 – EP")).toBe("創作");
    expect(releaseTitle("Single")).toBe("Single");
  });
});
