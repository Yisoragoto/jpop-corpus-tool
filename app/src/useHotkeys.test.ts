// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";

import { keepsKey, keyboardFocused, trackFocusOrigin } from "./useHotkeys";

function make(html: string): HTMLElement {
  document.body.innerHTML = html;
  return document.querySelector("[data-target]") as HTMLElement;
}

const mouse = () => false;
const keyboard = () => true;

describe("keepsKey", () => {
  it("输入框里所有键都留给打字", () => {
    const input = make('<input type="text" data-target>');
    for (const code of ["Space", "ArrowLeft", "ArrowUp"]) expect(keepsKey(input, code, mouse)).toBe(true);
    expect(keepsKey(make("<textarea data-target></textarea>"), "Space", mouse)).toBe(true);
  });

  it("用鼠标点过的按钮：空格和方向键仍是全局快捷键", () => {
    const button = make("<button data-target>播放</button>");
    expect(keepsKey(button, "Space", mouse)).toBe(false);
    expect(keepsKey(button, "ArrowRight", mouse)).toBe(false);
    // 点过歌词里的词（按钮里的字）之后也一样
    const inner = make('<button class="token"><span class="kc" data-target>夜</span></button>');
    expect(keepsKey(inner, "Space", mouse)).toBe(false);
  });

  it("用 Tab 移到按钮上（BUG-0001）：空格留给按钮，方向键仍是全局的", () => {
    const button = make("<button data-target>导入</button>");
    expect(keepsKey(button, "Space", keyboard)).toBe(true);
    expect(keepsKey(button, "Enter", keyboard)).toBe(true);
    expect(keepsKey(button, "ArrowLeft", keyboard)).toBe(false);
  });

  it("滑块自己用方向键，空格不抢", () => {
    const slider = make('<input type="range" data-target>');
    expect(keepsKey(slider, "ArrowLeft", mouse)).toBe(true);
    const custom = make('<div role="slider" tabindex="0" data-target></div>');
    expect(keepsKey(custom, "ArrowUp", mouse)).toBe(true);
    expect(keepsKey(custom, "Space", mouse)).toBe(false);
  });

  it("页面空白处：全部是全局快捷键", () => {
    expect(keepsKey(make("<div data-target></div>"), "Space", keyboard)).toBe(false);
    expect(keepsKey(null, "Space", keyboard)).toBe(false);
  });

  it("鼠标点出来的焦点和 Tab 移过去的焦点分得开", () => {
    document.body.innerHTML = "<button id=a>a</button><button id=b>b</button>";
    const a = document.getElementById("a")!;
    const b = document.getElementById("b")!;
    const untrack = trackFocusOrigin();
    try {
      window.dispatchEvent(new Event("pointerdown"));
      a.focus();
      expect(keyboardFocused(a)).toBe(false);
      expect(keepsKey(a, "Space")).toBe(false);
      // 之后用 Tab 移到下一个：这次聚焦前按的是键盘
      window.dispatchEvent(new KeyboardEvent("keydown", { code: "Tab", key: "Tab" }));
      b.focus();
      expect(keyboardFocused(b)).toBe(true);
      expect(keepsKey(b, "Space")).toBe(true);
    } finally {
      untrack();
    }
  });
});
