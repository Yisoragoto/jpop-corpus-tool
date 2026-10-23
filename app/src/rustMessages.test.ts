/**
 * 后端（Rust）发到界面上的中文：报错、命令返回的提示句、进度消息。
 *
 * 后端不分语言，照旧写中文；界面是英文时由 `trBackend`（i18n.ts）按 `locales/backend.ts` 换成英文。
 * 这里扫一遍 Rust 源码，列出所有中文字符串字面量（去掉测试、日志、panic 和纯日文数据），
 * 要求每一条在 backend.ts 里都有英文——新加一句报错忘了翻译，这个测试会指出来。
 */

import { describe, expect, it } from "vitest";

import { backendKey, interpolate, setLanguageForTest, trBackend, trPayload } from "./i18n";
import { backendEn } from "./locales/backend";
import { en } from "./locales/en";

const RUST = import.meta.glob<string>(["../../rust/crates/*/src/**/*.rs"], { query: "?raw", import: "default", eager: true });

/**
 * 不进界面的：macOS 桥（另有 Swift 界面）、分词和正规化的日文数据表、诊断报告（给开发者看的）、
 * LRC 里认署名行用的关键词、测试夹具、和 Python 版逐字节相同的日文统计报告、Anki 卡片内容和样式（日文学习材料）、
 * 挖词报告的 HTML（导出的文件，后续再做）
 */
const SKIP_FILE =
  /jp-cli\/|jp-ffi\/|jp-tokenizer\/|jp-normalize\/|jp-app\/src\/diagnostics\.rs|jp-app\/src\/log\.rs|\/bin\/|\/examples\/|jp-import\/src\/lrc\.rs|jp-corpus\/src\/fixture\.rs|jp-corpus\/src\/stats\.rs|jp-anki\/src\/(card|model|lyrics_model|mining_report|dict)\.rs/;
const HAN = /\p{Script=Han}/u;
const KANA = /[\p{Script=Hiragana}\p{Script=Katakana}]/u;
const ZH_ONLY = /[们这说里个么没为从词选择发应该时会对设导认点开关显单纯样号与查读写库]/;

/** 一个 Rust 源文件里的字符串字面量（跳过注释、字符字面量；认 r"…"、r#"…"#） */
export function rustStrings(src: string): { value: string; before: string }[] {
  const out: { value: string; before: string }[] = [];
  let i = 0;
  const n = src.length;
  while (i < n) {
    const c = src[i]!;
    const next = src[i + 1];
    if (c === "/" && next === "/") {
      while (i < n && src[i] !== "\n") i += 1;
    } else if (c === "/" && next === "*") {
      let depth = 1;
      i += 2;
      while (i < n && depth > 0) {
        if (src[i] === "/" && src[i + 1] === "*") (depth += 1), (i += 2);
        else if (src[i] === "*" && src[i + 1] === "/") (depth -= 1), (i += 2);
        else i += 1;
      }
    } else if (c === "'" ) {
      // 字符字面量 'x' / '\n' / '\''；生命周期 'a 不闭合，原样跳过一个字符
      const m = /^'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^\\'])'/u.exec(src.slice(i, i + 16));
      i += m ? m[0].length : 1;
    } else if ((c === "r" || (c === "b" && next === "r")) && /^b?r#*"/.test(src.slice(i, i + 10)) && !/[\w]/.test(src[i - 1] ?? " ")) {
      const m = /^b?r(#*)"/.exec(src.slice(i))!;
      const close = '"' + m[1];
      const start = i + m[0].length;
      const end = src.indexOf(close, start);
      out.push({ value: src.slice(start, end), before: src.slice(Math.max(0, i - 120), i) });
      i = end + close.length;
    } else if (c === '"') {
      let j = i + 1;
      let value = "";
      while (j < n && src[j] !== '"') {
        if (src[j] === "\\") {
          const e = src[j + 1];
          if (e === "n") value += "\n";
          else if (e === "t") value += "\t";
          else if (e === "\n" || e === "\r") {
            // 行尾续行：吃掉换行和下一行开头的空白
            j += 1;
            while (j < n && /\s/.test(src[j]!)) j += 1;
            continue;
          } else value += e ?? "";
          j += 2;
        } else {
          value += src[j];
          j += 1;
        }
      }
      out.push({ value, before: src.slice(Math.max(0, i - 120), i) });
      i = j + 1;
    } else {
      i += 1;
    }
  }
  return out;
}

/** 界面上可能看到的那些：中文、不在测试里、不是日志或 panic、不是纯日文 */
function uiMessages(): Map<string, string[]> {
  const found = new Map<string, string[]>();
  for (const [path, text] of Object.entries(RUST)) {
    const file = path.replace(/^(\.\.\/)+rust\/crates\//, "");
    if (SKIP_FILE.test(file)) continue;
    const cut = text.indexOf("#[cfg(test)]");
    const code = cut >= 0 ? text.slice(0, cut) : text;
    for (const { value, before } of rustStrings(code)) {
      if (!HAN.test(value)) continue;
      if (KANA.test(value) && !ZH_ONLY.test(value)) continue;
      // SQL（里面的中文是注释或写进库的默认值）
      if (/^\s*(SELECT|INSERT|UPDATE|DELETE|WITH|CREATE)\b/.test(value)) continue;
      // 这句之前最近的一条语句里是日志、panic：不进界面
      const stmt = before.slice(Math.max(before.lastIndexOf(";"), before.lastIndexOf("{\n"), before.lastIndexOf("}\n")) + 1);
      if (/(log::|tracing::|eprintln!|println!|\.expect\(|unreachable!|panic!|debug_assert|assert!|log_line|crate::log)/.test(stmt)) continue;
      const key = backendKey(value);
      found.set(key, [...(found.get(key) ?? []), file]);
    }
  }
  return found;
}

describe("后端发到界面的中文都有英文", () => {
  const messages = uiMessages();

  it("扫到了东西（glob 路径没写错）", () => {
    expect(messages.size).toBeGreaterThan(200);
  });

  it("每一条都有英文", () => {
    const missing = [...messages.entries()].filter(([k]) => !(k in backendEn)).map(([k, files]) => `${files[0]} ${JSON.stringify(k)}`);
    expect(missing).toEqual([]);
  });

  it("英文的占位符和中文对得上", () => {
    const count = (s: string) => new Set([...s.matchAll(/\{(\d+)\}/g)].map((m) => m[1]));
    const bad = Object.entries(backendEn).filter(([zh, en]) => [...count(en)].some((x) => !count(zh).has(x)));
    expect(bad).toEqual([]);
  });

  it("没有用不上的英文（Rust 里已经删掉的句子）", () => {
    expect(Object.keys(backendEn).filter((k) => !messages.has(k))).toEqual([]);
  });
});

describe("trBackend", () => {
  it("中文界面原样返回", () => {
    setLanguageForTest("zh");
    expect(trBackend("找不到安装包：C:/x.exe")).toBe("找不到安装包：C:/x.exe");
  });

  it("英文界面：按模板换，占位符里的东西照搬", () => {
    setLanguageForTest("en");
    try {
      const sample = Object.entries(backendEn).find(([k]) => /\{0\}/.test(k) && !/\{1\}/.test(k))!;
      const zh = sample[0].replace("{0}", "C:/x.exe");
      expect(trBackend(zh)).toBe(interpolate(sample[1], { 0: "C:/x.exe" }));
      // anyhow 的链用 ": " 连起来：一段一段换，认不出的（英文原因）照搬
      expect(trBackend(`${zh}: Peer disconnected`)).toBe(`${interpolate(sample[1], { 0: "C:/x.exe" })}: Peer disconnected`);
      // 没有中文的不动
      expect(trBackend("Peer disconnected")).toBe("Peer disconnected");
    } finally {
      setLanguageForTest("zh");
    }
  });
});

describe("前后端同一句话", () => {
  it("没有分词词典：前端 NO_DICTIONARY 的英文和后端报错的英文一样（App 靠 includes 认这条提示）", () => {
    expect(backendEn["找不到 Sudachi 词典"]).toBe(en["找不到 Sudachi 词典"]);
  });
});

describe("trPayload", () => {
  it("只换后端写的那几个字段，歌名歌词原样", () => {
    setLanguageForTest("en");
    try {
      const payload = { title: "没有改动", error: "没有改动", items: [{ text: "没有改动", reason: "没有改动" }], warnings: ["没有改动"] };
      expect(trPayload(payload)).toEqual({
        title: "没有改动",
        error: "No changes",
        items: [{ text: "没有改动", reason: "No changes" }],
        warnings: ["No changes"],
      });
      // 没有可换的：原样返回同一个对象
      const plain = { title: "夜に駆ける", songs: [1, 2] };
      expect(trPayload(plain)).toBe(plain);
    } finally {
      setLanguageForTest("zh");
    }
  });
});

describe("backendKey", () => {
  it("Rust 的占位符按出现顺序编号，{{ }} 是字面的花括号", () => {
    expect(backendKey("{} 里没有 corpus.db")).toBe("{0} 里没有 corpus.db");
    expect(backendKey("补齐分词：{} 首、{n} 行、{:.1} 个词、{x:?}")).toBe("补齐分词：{0} 首、{1} 行、{2} 个词、{3}");
    expect(backendKey("{{字面}}{}")).toBe("{{字面}}{0}");
  });
});
