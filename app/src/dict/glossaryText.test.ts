import { describe, expect, it } from "vitest";

import type { GlossaryEntry } from "./api";
import { glossaryPlainText, previewText, splitLineBreaks } from "./glossaryText";

describe("splitLineBreaks", () => {
  it("把明鏡日汉那种写在纯文本里的 <br> 当换行，大小写和自闭合都认", () => {
    expect(splitLineBreaks("(1)暗い<br>(2)休息<BR/>(3)遊蕩<br />")).toEqual(["(1)暗い", "(2)休息", "(3)遊蕩", ""]);
  });

  it("别的尖括号原样保留，不当 HTML", () => {
    expect(splitLineBreaks("A<b>B</b>")).toEqual(["A<b>B</b>"]);
  });
});

describe("glossaryPlainText / previewText", () => {
  const ruby: GlossaryEntry = {
    type: "structured-content",
    content: [
      { tag: "ruby", content: ["今更", { tag: "rt", content: "いまさら" }] },
      { tag: "br" },
      { tag: "span", content: "時機を失した今" },
      { tag: "img", path: "gaiji/x.svg" },
    ],
  };

  it("结构化内容去掉振假名和图片，换行变空格", () => {
    expect(glossaryPlainText(ruby)).toBe("今更 時機を失した今");
  });

  it("预览压掉空白并截断", () => {
    // 「一 二 / 三 四」共 9 个字，截前 7 个
    expect(previewText(["一\n二", "三<br>四"], 7)).toBe("一 二 / 三…");
    expect(previewText([{ type: "image", path: "a.png", description: "插图" }])).toBe("插图");
  });
});
