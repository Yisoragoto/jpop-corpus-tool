import { describe, expect, it } from "vitest";

import type { FrequencyRow, KwicHit } from "./api";
import { filterCloudRows, fontSizeFor, groupHitsBySong, isFunctionWord, isSentenceStart, layoutCloud, mergeByLemma, variantKey, type CloudWord } from "./wordCloud";

const row = (lemma: string, pos: string, freq: number, songCount = 1): FrequencyRow => ({
  lemma, pos, freq, songCount, surfaces: [lemma], jlpt: "",
});

describe("mergeByLemma", () => {
  it("把各种写法收在一起（字幕里标亮用）", () => {
    const words = mergeByLemma(
      [{ ...row("見る", "VERB", 9), surfaces: ["見", "見る"] }, { ...row("見る", "AUX", 3), surfaces: ["見", "みる"] }],
      5,
    );
    expect(words[0]!.surfaces).toEqual(["見", "見る", "みる"]);
  });

  it("同一词元的几种词性合成一个，次数相加，曲数取最大", () => {
    const words = mergeByLemma([row("好き", "ADJ", 30, 12), row("君", "PRON", 50, 40), row("好き", "NOUN", 25, 9)], 10);
    expect(words.map((w) => [w.lemma, w.freq, w.songCount])).toEqual([
      ["好き", 55, 12],
      ["君", 50, 40],
    ]);
  });

  it("只有一种词性时带上词性，几种时不带", () => {
    const words = mergeByLemma([row("好き", "ADJ", 30), row("君", "PRON", 50), row("好き", "NOUN", 25)], 10);
    expect(words.find((w) => w.lemma === "君")?.pos).toBe("PRON");
    expect(words.find((w) => w.lemma === "好き")?.pos).toBeNull();
  });

  it("按合并后的次数截断", () => {
    expect(mergeByLemma([row("a", "X", 3), row("b", "X", 2), row("c", "X", 1)], 2).map((w) => w.lemma)).toEqual(["a", "b"]);
  });
});

describe("fontSizeFor", () => {
  it("最多的拿最大号，最少的拿最小号", () => {
    expect(fontSizeFor(100, 1, 100, 12, 80)).toBe(80);
    expect(fontSizeFor(1, 1, 100, 12, 80)).toBe(12);
  });
  it("只有一个次数时不除以零", () => {
    expect(fontSizeFor(5, 5, 5, 12, 80)).toBe(80);
  });
});

describe("layoutCloud", () => {
  const words: CloudWord[] = Array.from({ length: 120 }, (_, i) => ({
    // 歌词里常见的长度：大多一到三个字
    lemma: ["君", "僕", "見る", "言う", "世界", "抱きしめる", "あなた", "love"][i % 8] + "空海花夢星".charAt(i % 5).repeat(i % 3),
    freq: 1000 - i * 7,
    songCount: 1,
    variants: [],
    mainPos: "NOUN",
    pos: null,
    surfaces: [],
  }));
  // 每个字按字号宽，近似全角字
  const measure = (text: string, size: number) => [...text].length * size;
  const opts = { width: 800, height: 420, minFont: 12, maxFont: 72, verticalRatio: 0.25, measure };

  it("词框不重叠，也不出画布", () => {
    const placed = layoutCloud(words, opts);
    expect(placed.length).toBeGreaterThan(30);
    for (const p of placed) {
      expect(p.x).toBeGreaterThanOrEqual(0);
      expect(p.y).toBeGreaterThanOrEqual(0);
      expect(p.x + p.w).toBeLessThanOrEqual(800);
      expect(p.y + p.h).toBeLessThanOrEqual(420);
    }
    for (let i = 0; i < placed.length; i++) {
      for (let j = i + 1; j < placed.length; j++) {
        const a = placed[i]!;
        const b = placed[j]!;
        const overlap = a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
        expect(overlap, `${a.word.lemma} × ${b.word.lemma}`).toBe(false);
      }
    }
  });

  it("同样的输入排出同样的结果", () => {
    expect(layoutCloud(words, opts)).toEqual(layoutCloud(words, opts));
  });

  it("前三名不竖排，带拉丁字母的不竖排", () => {
    const placed = layoutCloud(words, { ...opts, verticalRatio: 1 });
    expect(placed.filter((p) => p.rank < 3).every((p) => !p.vertical)).toBe(true);
    expect(placed.filter((p) => /[a-z]/.test(p.word.lemma)).every((p) => !p.vertical)).toBe(true);
  });

  it("第一名最大并且在中间附近", () => {
    const first = layoutCloud(words, opts)[0]!;
    expect(first.rank).toBe(0);
    expect(first.fontSize).toBe(72);
    expect(Math.abs(first.x + first.w / 2 - 400)).toBeLessThan(80);
  });

  it("没有词或画布为零时返回空", () => {
    expect(layoutCloud([], opts)).toEqual([]);
    expect(layoutCloud(words, { ...opts, width: 0 })).toEqual([]);
  });
});

describe("groupHitsBySong", () => {
  const hit = (songId: string, repeatCount = 1, utteranceId = 1): KwicHit => ({
    songId, artist: "A", title: `t${songId}`, audioPath: "", utteranceId, lineIdx: 0, timeSec: null,
    text: "", left: "", keyword: "", right: "", repeatCount, corrected: false, endSec: null, endKind: null, matchLen: 1, leftWords: [], rightWords: [], linePos: 0, songLines: 1,
  });
  const mid = (songId: string, left: string, utteranceId = 1): KwicHit => ({ ...hit(songId, 1, utteranceId), left });

  it("有一行以这个词开头的歌排前面，显示那一行；空格、句号后面的不算", () => {
    const songs = groupHitsBySong([
      mid("001", "雨が降る", 1),
      mid("001", "雨が降る", 2),
      mid("001", "雨が降る", 3),
      mid("002", "本当なんだ　", 4),
      mid("003", "そして", 5),
      mid("003", "", 6),
    ]);
    expect(songs.map((s) => [s.songId, s.atStart, s.first.utteranceId])).toEqual([
      ["003", true, 6],
      ["001", false, 1],
      ["002", false, 4],
    ]);
  });

  it("按歌合并，折叠的重复行也算次数，多的在前", () => {
    const songs = groupHitsBySong([hit("001", 1, 10), hit("002"), hit("001", 3, 11), hit("002")]);
    expect(songs.map((s) => [s.songId, s.count])).toEqual([
      ["001", 4],
      ["002", 2],
    ]);
  });

  it("跳转用每首歌的第一处", () => {
    expect(groupHitsBySong([hit("001", 1, 10), hit("001", 1, 11)])[0]?.first.utteranceId).toBe(10);
  });
});

describe("词云去虚词", () => {
  it("单个假名、形式名词、纯符号算虚词，实词不算", () => {
    for (const w of ["つ", "ら", "お", "る", "ー", "こと", "まま", "もの", "よう", "~", "＂", "  "]) expect(isFunctionWord(w)).toBe(true);
    for (const w of ["夜", "心", "言葉", "思い出", "ヨルシカ", "I", "月"]) expect(isFunctionWord(w)).toBe(false);
  });
  it("助词、助动词这类词性也算虚词", () => {
    const rows = [{ lemma: "から", pos: "ADP" }, { lemma: "て", pos: "SCONJ" }, { lemma: "夜", pos: "NOUN" }, { lemma: "僕", pos: "PRON" }];
    expect(filterCloudRows(rows, true, new Set()).map((r) => r.lemma)).toEqual(["夜", "僕"]);
  });

  it("隐藏词总会去掉，虚词只在开关开着时去掉", () => {
    const rows = [{ lemma: "夜" }, { lemma: "つ" }, { lemma: "花" }];
    expect(filterCloudRows(rows, true, new Set(["花"])).map((r) => r.lemma)).toEqual(["夜"]);
    expect(filterCloudRows(rows, false, new Set(["花"])).map((r) => r.lemma)).toEqual(["夜", "つ"]);
  });
});

describe("异体写法合并", () => {
  const forms = {
    思い出: { lemma: "思い出", normalized: "思い出", reading: "おもいで" },
    想い出: { lemma: "想い出", normalized: "思い出", reading: "おもいで" },
    遠く: { lemma: "遠く", normalized: "遠い", reading: "とおく" },
    遠い: { lemma: "遠い", normalized: "遠い", reading: "とおい" },
    Right: { lemma: "Right", normalized: "ライト", reading: "らいと" },
    ライト: { lemma: "ライト", normalized: "ライト", reading: "らいと" },
    かみ: { lemma: "かみ", normalized: "紙", reading: "かみ" },
    紙: { lemma: "紙", normalized: "紙", reading: "かみ" },
    ひとり: { lemma: "ひとり", normalized: "一人", reading: "ひとり" },
    一人: { lemma: "一人", normalized: "一人", reading: "ひとり" },
  } as Record<string, { lemma: string; normalized: string; reading: string }>;
  const key = (l: string) => variantKey(l, forms[l]);

  it("正规化写法和读音都相同才合并", () => {
    expect(key("想い出")).toBe(key("思い出"));
    expect(key("ひとり")).toBe(key("一人"));
    expect(key("遠く")).not.toBe(key("遠い"));
  });

  it("拉丁字母、两个字以内的假名不往汉字上并", () => {
    expect(key("Right")).not.toBe(key("ライト"));
    expect(key("かみ")).not.toBe(key("紙"));
    expect(variantKey("ヒト", undefined)).toBe(variantKey("ひと", undefined));
  });

  it("合并后显示次数多的写法，几种写法都留着查歌用", () => {
    const words = mergeByLemma([row("想い出", "NOUN", 21, 5), row("思い出", "NOUN", 52, 20), row("夜", "NOUN", 60, 30)], 10, key);
    expect(words.map((w) => [w.lemma, w.freq, w.variants])).toEqual([
      ["思い出", 73, ["思い出", "想い出"]],
      ["夜", 60, ["夜"]],
    ]);
    expect(words[0]!.mainPos).toBe("NOUN");
  });
});

describe("isSentenceStart", () => {
  it("只有这一行从这个词开始才算", () => {
    for (const left of ["", "  ", "　"]) expect(isSentenceStart(left)).toBe(true);
    for (const left of ["本当なんだ　", "僕は今 ", "終わり。", "雨が"]) expect(isSentenceStart(left)).toBe(false);
  });
});
