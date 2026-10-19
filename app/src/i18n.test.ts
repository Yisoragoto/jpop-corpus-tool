import ts from "typescript";
import { describe, expect, it } from "vitest";

import { interpolate } from "./i18n";
import { en } from "./locales/en";
import { en1 } from "./locales/en1";
import { en2 } from "./locales/en2";
import { en3 } from "./locales/en3";
import { en4 } from "./locales/en4";
import { en5 } from "./locales/en5";

/** 全部源码（vite 的 glob，测试里也能用；不用 node:fs 就不需要 node 的类型） */
const FILES = import.meta.glob<string>(["./**/*.ts", "./**/*.tsx", "!./**/*.test.ts", "!./**/*.test.tsx", "!./locales/**", "!./**/*.d.ts"], {
  query: "?raw",
  import: "default",
  eager: true,
});
// 中日统一表意文字 / 平假名和片假名
const HAN = /[\u4e00-\u9fff]/;
const KANA = /[\u3040-\u30ff]/;

/** 不是界面文字、是日文数据或特意两种语言并排写的：不要求翻译 */
const NOT_UI = new Set([
  // 词性的日文名（统计报告的「和名」一列，和 Python 导出的 TXT 一样）
  "名詞", "動詞", "形容詞", "副詞", "助動詞", "代名詞", "固有名詞", "感動詞", "助詞", "接続詞", "数詞", "接辞", "従属接", "限定詞",
  // 词云当虚词去掉的形式名词（汉字写法）
  "事", "物", "儘", "様", "所", "為", "筈", "訳", "達", "等",
  // 图标里画的字、字体探测用的字
  "夢", "君", "空", "世界", "君僕夢",
  // pyFormat 的 CSV 表头（和 Python 版逐字节相同）
  "時刻(秒)", "左文脈", "右文脈", "全文", "歌手", "曲名",
]);
/** 这些文件里的中文是特意写死的（选语言的弹窗两种语言并排） */
const BILINGUAL_FILES = new Set(["i18n.ts", "components/LanguagePicker.tsx", "pages/SettingsPage.tsx:界面语言 · Language"]);

interface Found {
  file: string;
  line: number;
  text: string;
}

/** 扫一遍源码：`tr("…")` 的键，以及没套 `tr()` 的中文字面量 */
function scan() {
  const keys: Found[] = [];
  const bare: Found[] = [];
  for (const [path, text] of Object.entries(FILES)) {
    const file = path.replace(/^\.\//, "");
    const sf = ts.createSourceFile(path, text, ts.ScriptTarget.Latest, true, path.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS);
    const at = (n: ts.Node) => sf.getLineAndCharacterOfPosition(n.getStart(sf)).line + 1;
    const isTrArg = (n: ts.Node) =>
      n.parent !== undefined &&
      ts.isCallExpression(n.parent) &&
      ts.isIdentifier(n.parent.expression) &&
      n.parent.expression.text === "tr" &&
      n.parent.arguments[0] === n;
    const inType = (n: ts.Node) => {
      for (let p = n.parent; p; p = p.parent) {
        if (ts.isLiteralTypeNode(p)) return true;
        if (ts.isStatement(p)) return false;
      }
      return false;
    };
    const check = (n: ts.Node, s: string) => {
      if (!HAN.test(s) || inType(n)) return;
      if (BILINGUAL_FILES.has(file) || BILINGUAL_FILES.has(`${file}:${s}`)) return;
      if (NOT_UI.has(s.trim())) return;
      // 只有日文（有假名、没有中文才用的字）：歌词示例之类
      if (KANA.test(s) && !/[们这说里个么没为从词选择发应该时会对设导认点开关显单纯样号与查读写库]/.test(s)) return;
      bare.push({ file, line: at(n), text: s.trim() });
    };
    const visit = (n: ts.Node) => {
      if (ts.isStringLiteral(n) || ts.isNoSubstitutionTemplateLiteral(n)) {
        if (isTrArg(n)) keys.push({ file, line: at(n), text: n.text });
        else check(n, n.text);
      } else if (ts.isTemplateExpression(n)) {
        check(n, [n.head.text, ...n.templateSpans.map((s) => s.literal.text)].join("…"));
      } else if (ts.isJsxText(n)) {
        check(n, n.text);
      }
      ts.forEachChild(n, visit);
    };
    visit(sf);
  }
  return { keys, bare };
}

const { keys, bare } = scan();

describe("界面文字都有英文", () => {
  it("每一处 tr() 的键都有英文", () => {
    const missing = [...new Set(keys.filter((k) => !(k.text in en)).map((k) => `${k.file}:${k.line} ${JSON.stringify(k.text)}`))];
    expect(missing).toEqual([]);
  });

  it("没有漏套 tr() 的中文字面量", () => {
    expect(bare.map((b) => `${b.file}:${b.line} ${b.text.slice(0, 60)}`)).toEqual([]);
  });

  it("英文里的占位符在中文键里都有（拼错了会原样露出 {xxx}）", () => {
    const names = (s: string) => new Set([...s.matchAll(/\{(\w+)(?:\|[^{}]*)?\}/g)].map((m) => m[1]!));
    const bad = Object.entries(en).flatMap(([zh, e]) => {
      if (typeof e !== "string") return [];
      const have = names(zh);
      return [...names(e)].filter((n) => !have.has(n)).map((n) => `${JSON.stringify(zh)} → {${n}}`);
    });
    expect(bad).toEqual([]);
  });

  it("几个分表之间同一个键不会给出不同的英文", () => {
    const seen = new Map<string, string>();
    const conflicts: string[] = [];
    for (const part of [en1, en2, en3, en4, en5]) {
      for (const [k, v] of Object.entries(part)) {
        const prev = seen.get(k);
        if (prev !== undefined && prev !== v) conflicts.push(`${k}: ${prev} / ${v}`);
        seen.set(k, v);
      }
    }
    expect(conflicts).toEqual([]);
  });

  it("没有用不上的英文（源码里已经删掉的键）", () => {
    const used = new Set(keys.map((k) => k.text));
    expect(Object.keys(en).filter((k) => !used.has(k))).toEqual([]);
  });
});

describe("interpolate", () => {
  it("填占位符，认不出的原样留着", () => {
    expect(interpolate("{a} and {b}", { a: 1 })).toBe("1 and {b}");
  });
  it("单复数按值是不是 1 挑", () => {
    const s = "{n} {n|song|songs}";
    expect(interpolate(s, { n: 1 })).toBe("1 song");
    expect(interpolate(s, { n: 3 })).toBe("3 songs");
    expect(interpolate(s, { n: "1,204" })).toBe("1,204 songs");
    expect(interpolate("{n|is|are}", { n: "1" })).toBe("is");
  });
});
