import { describe, expect, it } from "vitest";

import { crashNotice, messageOf } from "./errors";

describe("messageOf", () => {
  it("用 Error 的 message", () => {
    expect(messageOf(new Error("打不开 corpus.db"))).toBe("打不开 corpus.db");
  });

  it("认得 IPC 直接扔过来的 CommandError 形状", () => {
    expect(messageOf({ message: "下载失败：Peer disconnected" })).toBe(
      "下载失败：Peer disconnected",
    );
  });

  it("字符串原样用", () => {
    expect(messageOf("词典还没装")).toBe("词典还没装");
  });

  it("没带任何原因时不能给出 [object Object] 或 undefined", () => {
    // 这三种原来会变成界面上一句 "undefined" / "[object Object]"——
    // 既没告诉用户发生了什么，也没告诉我该去查哪儿
    for (const nothing of [undefined, null, {}, new Error(""), ""]) {
      const text = messageOf(nothing);
      expect(text).not.toBe("undefined");
      expect(text).not.toBe("[object Object]");
      expect(text.length).toBeGreaterThan(0);
      expect(text).toContain("诊断");
    }
  });

  it("嵌套的 message 不是字符串时也不当真", () => {
    expect(messageOf({ message: { nested: true } })).toContain("诊断");
  });
});

describe("crashNotice", () => {
  it("把原因带上，让白屏至少说得出一句话", () => {
    const notice = crashNotice(new Error("Cannot read properties of null"));
    expect(notice.title).toBe("界面出错了");
    expect(notice.detail).toBe("Cannot read properties of null");
  });
});
