import { describe, expect, it } from "vitest";

import type { PlanItem, ScanResult } from "./api";
import { newItems, NO_IGNORES, parseIgnores, pathKey, withIgnored, withoutIgnoredFolder } from "./newSongs";

function item(path: string, kind: "new" | "skipped"): PlanItem {
  return {
    path,
    fileName: path,
    title: path,
    artist: "ヨルシカ",
    artistSource: "tag",
    album: "",
    durationSec: 200,
    hasLyrics: false,
    warning: null,
    action: kind === "new" ? { kind: "new", songId: "211" } : { kind: "skipped", reason: "不是音频" },
  };
}

describe("新歌", () => {
  it("只有会写进库的才算", () => {
    const scan = { items: [item("a.flac", "new"), item("b.txt", "skipped")] } as unknown as ScanResult;
    expect(newItems(scan).map((i) => i.path)).toEqual(["a.flac"]);
  });
});

describe("「别再提」的清单", () => {
  it("存坏了的当空清单", () => {
    for (const raw of [null, "", "不是 JSON", "null", "[]", '{"folders":"D:\\\\m","files":[1,null,""]}']) {
      expect(parseIgnores(raw)).toEqual(NO_IGNORES);
    }
    expect(parseIgnores('{"folders":["D:\\\\m"],"files":["D:\\\\m\\\\a.flac",7]}')).toEqual({
      folders: ["D:\\m"],
      files: ["D:\\m\\a.flac"],
    });
  });

  it("同一个路径换个写法再加一次不会多出一条", () => {
    const once = withIgnored(NO_IGNORES, { files: ["D:\\m\\a.flac"], folders: ["D:\\m"] });
    const twice = withIgnored(once, { files: ["d:/M/A.flac", "D:\\m\\b.flac"], folders: ["D:/m/"] });
    expect(twice).toEqual({ folders: ["D:\\m"], files: ["D:\\m\\a.flac", "D:\\m\\b.flac"] });
  });

  it("恢复一个文件夹不动别的", () => {
    const current = { folders: ["D:\\m", "E:\\other"], files: ["D:\\m\\a.flac"] };
    expect(withoutIgnoredFolder(current, "d:/m/")).toEqual({ folders: ["E:\\other"], files: ["D:\\m\\a.flac"] });
  });

  it("比较键和后端的一样：分隔符统一、不分大小写、去掉末尾的分隔符", () => {
    expect(pathKey("D:\\Music\\ヨルシカ\\")).toBe("d:/music/ヨルシカ");
    expect(pathKey("d:/music/ヨルシカ")).toBe("d:/music/ヨルシカ");
  });
});
