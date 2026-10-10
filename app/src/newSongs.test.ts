import { describe, expect, it } from "vitest";

import type { ImportReport, NewAudio, PlanItem, WatchedFolder } from "./api";
import {
  DIRECT_IMPORT_LIMIT,
  describeNewSongs,
  folderOf,
  foldersOf,
  newItems,
  NO_IGNORES,
  parseIgnores,
  pathKey,
  reviewReason,
  summarizeImport,
  withIgnored,
  withoutIgnoredFolder,
} from "./newSongs";

function item(patch: Partial<PlanItem> = {}): PlanItem {
  return {
    path: "D:\\music\\ヨルシカ\\晴る.flac",
    fileName: "晴る.flac",
    title: "晴る",
    artist: "ヨルシカ",
    artistSource: "tag",
    album: "",
    durationSec: 200,
    hasLyrics: false,
    warning: null,
    action: { kind: "new", songId: "211" },
    ...patch,
  };
}

function folder(path: string, reason: WatchedFolder["reason"] = "libraryAudio"): WatchedFolder {
  return { path, reason, depth: reason === "parent" ? 3 : 1, songs: 1, unknown: 0, new: 0 };
}

describe("哪些新歌能不经复核直接导", () => {
  it("标签齐全的几首可以", () => {
    expect(reviewReason([item(), item({ artistSource: "fileName" })])).toBeNull();
  });

  it("读不出标签的要先看", () => {
    expect(reviewReason([item(), item({ warning: "读不出元数据：failed to parse Flac file" })])).toContain("1 首读不出标签");
  });

  it("歌手是从文件夹名猜的要先看", () => {
    // MyMusic/新歌手/曲.flac 会被读成歌手 MyMusic、专辑 新歌手
    expect(reviewReason([item({ artistSource: "folder", artist: "MyMusic" })])).toContain("文件夹名猜的");
  });

  it("定不出歌手的要先看", () => {
    expect(reviewReason([item({ artistSource: "none", artist: "" })])).toContain("定不出歌手");
  });

  it("一大批要先看，哪怕每一首都没问题", () => {
    const many = Array.from({ length: DIRECT_IMPORT_LIMIT + 1 }, (_, n) => item({ path: `D:\\m\\${n}.flac` }));
    expect(reviewReason(many.slice(0, DIRECT_IMPORT_LIMIT))).toBeNull();
    expect(reviewReason(many)).toContain(`${DIRECT_IMPORT_LIMIT + 1} 首`);
  });

  it("只有会写进库的才算新歌", () => {
    const scan = {
      summary: { total: 3, new: 1, alreadyImported: 0, possibleDuplicates: 1, duplicatesInBatch: 0, skipped: 1 },
      items: [
        item(),
        item({ action: { kind: "possibleDuplicate", songId: "004", existingPath: "E:\\a.flac" } }),
        item({ action: { kind: "skipped", reason: "不支持 Opus" } }),
      ],
      tokenizerReady: true,
      ignored: [],
    };
    expect(newItems(scan).map((i) => i.action.kind)).toEqual(["new"]);
  });
});

describe("横幅上的那句话", () => {
  it("少的全列出来，多的只列前几首再说总数", () => {
    const three = [item(), item({ title: "左右盲", artist: "ヨルシカ" }), item({ title: "怪獣", artist: "サカナクション" })];
    expect(describeNewSongs(three)).toBe("晴る — ヨルシカ、左右盲 — ヨルシカ、怪獣 — サカナクション");
    expect(describeNewSongs([...three, item({ title: "モス" })])).toBe("晴る — ヨルシカ、左右盲 — ヨルシカ、怪獣 — サカナクション 等 4 首");
  });

  it("没有歌手的只写曲名，不留一个空的破折号", () => {
    expect(describeNewSongs([item({ artist: " " })])).toBe("晴る");
  });
});

describe("文件是在哪个文件夹里找到的", () => {
  const folders = [folder("D:\\music", "parent"), folder("D:\\music\\ヨルシカ"), folder("E:\\cloud music\\sakana")];

  it("两个都包含它时取贴得近的", () => {
    expect(folderOf(folders, "D:\\music\\ヨルシカ\\晴る.flac")?.path).toBe("D:\\music\\ヨルシカ");
    expect(folderOf(folders, "D:\\music\\Vaundy\\怪獣の花唄.flac")?.path).toBe("D:\\music");
  });

  it("大小写和分隔符不一样也对得上", () => {
    expect(folderOf(folders, "e:/Cloud Music/sakana/a.flac")?.path).toBe("E:\\cloud music\\sakana");
  });

  it("名字只是前缀相同的不算在里面", () => {
    expect(folderOf(folders, "D:\\music2\\a.flac")).toBeUndefined();
  });

  it("按首数从多到少列出文件夹", () => {
    const found: NewAudio = {
      folders,
      scan: { summary: { total: 0, new: 0, alreadyImported: 0, possibleDuplicates: 0, duplicatesInBatch: 0, skipped: 0 }, items: [], tokenizerReady: true, ignored: [] },
    };
    const items = [
      item({ path: "E:\\cloud music\\sakana\\a.flac" }),
      item({ path: "D:\\music\\ヨルシカ\\b.flac" }),
      item({ path: "D:\\music\\ヨルシカ\\c.flac" }),
    ];
    expect(foldersOf(found, items)).toEqual(["D:\\music\\ヨルシカ", "E:\\cloud music\\sakana"]);
  });
});

describe("导完之后说什么", () => {
  const planned = [
    item({ path: "D:\\m\\a.flac", title: "晴る", artist: "ヨルシカ" }),
    item({ path: "D:\\m\\b.flac", title: "左右盲", artist: "ヨルシカ" }),
    item({ path: "D:\\m\\c.flac", title: "怪獣", artist: "サカナクション" }),
    item({ path: "D:\\m\\d.flac", title: "壊れた曲", artist: "誰か" }),
  ];
  const report: ImportReport = {
    cancelled: false,
    tracks: [
      { songId: "211", path: "D:\\m\\a.flac", title: "晴る", outcome: { kind: "imported", lyricLines: 30, tokens: 200, credits: 1, correctionsRestored: 0 } },
      { songId: "212", path: "d:/m/b.flac", title: "左右盲", outcome: { kind: "imported", lyricLines: 0, tokens: 0, credits: 1, correctionsRestored: 0 } },
      { songId: "213", path: "D:\\m\\c.flac", title: "怪獣", outcome: { kind: "imported", lyricLines: 0, tokens: 0, credits: 1, correctionsRestored: 0 } },
      { songId: "214", path: "D:\\m\\d.flac", title: "", outcome: { kind: "failed", error: "磁盘满了" } },
    ],
  };

  it("数出导了几首、归到谁名下、谁还缺歌词、谁没成", () => {
    expect(summarizeImport(report, planned)).toEqual({
      imported: 3,
      artists: ["ヨルシカ", "サカナクション"],
      withoutLyrics: ["212", "213"],
      failed: [{ title: "D:\\m\\d.flac", error: "磁盘满了" }],
      cancelled: false,
    });
  });

  it("没成的那首的歌手不算「归到名下」", () => {
    expect(summarizeImport(report, planned).artists).not.toContain("誰か");
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
