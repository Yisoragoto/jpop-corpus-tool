import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { api, LYRICS_SOURCE_LABELS, type TimedSourceConfig, type TimedSourceSettings } from "./api";
import { candidatePrecision } from "./components/TimedLyricsPanel";

const settings: TimedSourceSettings = {
  netease: true, qq: true, kugou: true, amll: true, lrclib: true, apple: false,
  appleStorefront: "jp", appleDeveloperTokenSet: true,
};
const config: TimedSourceConfig = {
  netease: true, qq: true, kugou: true, amll: true, lrclib: true, apple: false, appleStorefront: "jp",
};

beforeEach(() => { invoke.mockReset(); });

describe("逐字歌词 IPC 合同", () => {
  it("刷新名单只读取目标，不触发搜索或替换", async () => {
    const targets = [{ songId: "a", title: "Song", artist: "Artist", lyricLines: 12, timedLines: 3 }];
    invoke.mockResolvedValue(targets);
    expect(await api.lyricsTimedTargets()).toEqual(targets);
    expect(invoke.mock.calls).toEqual([["lyrics_timed_targets", undefined]]);
  });

  it("保留多个候选、零逐字计数和逐来源失败说明；搜索不自动采用", async () => {
    const response = {
      songId: "a", title: "Song", currentLines: 12, currentTimedLines: 0,
      candidates: [
        { id: "first", source: "qq", format: "qrc", title: "Song", artist: "Artist", lineCount: 12, timedLineCount: 12, preview: ["preview"], matchNote: "title matched" },
        { id: "second", source: "lrclib", format: "lrc", title: "Song (live)", artist: "Artist", lineCount: 14, timedLineCount: 0, preview: [], matchNote: "check version", mode: "lineSynced" },
      ],
      warnings: [{ source: "kugou", message: "HTTP 503" }],
    };
    invoke.mockResolvedValue(response);
    expect(await api.lyricsTimedSearch("a")).toEqual(response);
    expect(invoke.mock.calls).toEqual([["lyrics_timed_search", { songId: "a" }]]);
  });

  it("采用传递歌曲和候选 ID，过期候选拒绝后不自动重试", async () => {
    invoke.mockRejectedValue({ message: "stale candidate" });
    await expect(api.lyricsTimedApply("b", "candidate-a")).rejects.toThrow("stale candidate");
    expect(invoke.mock.calls).toEqual([["lyrics_timed_apply", { songId: "b", candidateId: "candidate-a" }]]);
  });

  it("设置读取只返回配置状态，常规保存省略令牌字段以保留旧值", async () => {
    invoke.mockResolvedValue(settings);
    expect(await api.lyricsSourcesGet()).toEqual(settings);
    expect(await api.lyricsSourcesSet(config)).toEqual(settings);
    expect(invoke.mock.calls).toEqual([
      ["lyrics_sources_get", undefined],
      ["lyrics_sources_set", { config }],
    ]);
    expect(Object.keys(config)).not.toContain("appleDeveloperToken");
    expect(Object.keys(config)).not.toContain("appleMusicUserToken");
  });

  it("明确清除 Developer Token 用空字符串", async () => {
    invoke.mockResolvedValue(settings);
    await api.lyricsSourcesSet({ ...config, appleDeveloperToken: "" });
    expect(invoke).toHaveBeenCalledWith("lyrics_sources_set", { config: { ...config, appleDeveloperToken: "" } });
  });

  it("新增来源均有显示名称，保留原有网易云来源名称", () => {
    for (const source of ["qq", "kugou", "amll", "apple", "netease", "lrclib"] as const) expect(LYRICS_SOURCE_LABELS[source]).toBeTruthy();
    expect(LYRICS_SOURCE_LABELS.netease).toBe("在线搜到");
    expect(LYRICS_SOURCE_LABELS.apple).toContain("元数据");
    expect(config.lrclib).toBe(true);
    expect(config.apple).toBe(false);
  });
});

describe("candidate precision is never inferred from a lyrics format claiming word sync", () => {
  it.each(["qrc", "yrc", "krc", "ttml", "elrc"])("zero word counts remain non-word for %s", (format) => {
    expect(candidatePrecision({ format, timedLineCount: 0 })).toBe("lineOrPlain");
  });
  it("labels LRCLIB line and plain candidates without inflating their zero word counts", () => {
    expect(candidatePrecision({ format: "lrc", timedLineCount: 0 })).toBe("line");
    expect(candidatePrecision({ format: "plain", timedLineCount: 0 })).toBe("plain");
    expect(candidatePrecision({ format: "ttml", timedLineCount: 0, mode: "plain" })).toBe("plain");
    expect(candidatePrecision({ format: "unknown", timedLineCount: 0, mode: "lineSynced" })).toBe("line");
    expect(candidatePrecision({ format: "ttml", timedLineCount: 0, mode: "karaoke" })).toBe("lineOrPlain");
  });
  it("requires positive finite actual word counts for a word-timed label", () => {
    expect(candidatePrecision({ format: "elrc", timedLineCount: 2 })).toBe("word");
    expect(candidatePrecision({ format: "ttml", timedLineCount: NaN })).toBe("lineOrPlain");
  });
});
