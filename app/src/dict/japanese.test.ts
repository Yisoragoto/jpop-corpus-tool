/**
 * 和 Yomitan 对账。基准 `__fixtures__/yomitan-japanese-util.json` 由
 * `rust/crates/jp-dict/tools/dump-yomitan-japanese-util.mjs` 直接跑 Yomitan 源码生成：
 * Yomitan 自带的用例，加上从真实词典抽的 1500 个词头（900 个带送り仮名、600 个纯汉字）。
 */

import { describe, expect, it } from "vitest";

import fixture from "./__fixtures__/yomitan-japanese-util.json";
import {
  convertKatakanaToHiragana,
  distributeFurigana,
  distributeFuriganaInflected,
  getDownstepPositions,
  getKanaDiacriticInfo,
  getKanaMoraCount,
  getKanaMorae,
  getPitchCategory,
  isMoraPitchHigh,
} from "./japanese";

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

describe("注音分配和 Yomitan 一致", () => {
  it("distributeFurigana", () => {
    expect(fixture.furigana.length).toBeGreaterThan(1500);
    const failures = fixture.furigana
      .filter((c) => !same(distributeFurigana(c.term, c.reading), c.expected))
      .map((c) => `${c.term}【${c.reading}】`);
    expect(failures).toEqual([]);
  });

  it("distributeFuriganaInflected", () => {
    expect(fixture.inflected.length).toBeGreaterThan(1600);
    const failures = fixture.inflected
      .filter((c) => !same(distributeFuriganaInflected(c.term, c.reading, c.source), c.expected))
      .map((c) => `${c.term}【${c.reading}】← ${c.source}`);
    expect(failures).toEqual([]);
  });

  it("送り仮名分开注，纯汉字整段注", () => {
    expect(distributeFurigana("打ち込む", "うちこむ")).toEqual([
      { text: "打", reading: "う" },
      { text: "ち", reading: "" },
      { text: "込", reading: "こ" },
      { text: "む", reading: "" },
    ]);
    expect(distributeFurigana("学校", "がっこう")).toEqual([{ text: "学校", reading: "がっこう" }]);
  });
});

describe("音拍、音高和 Yomitan 一致", () => {
  it("getKanaMorae / getKanaMoraCount", () => {
    for (const c of fixture.morae) {
      expect(getKanaMorae(c.text), c.text).toEqual(c.morae);
      expect(getKanaMoraCount(c.text), c.text).toBe(c.count);
    }
  });

  it("isMoraPitchHigh", () => {
    for (const c of fixture.pitch) {
      expect(isMoraPitchHigh(c.index, c.value), `${c.value}@${c.index}`).toBe(c.high);
    }
  });

  it("getDownstepPositions", () => {
    for (const c of fixture.downsteps) {
      expect(getDownstepPositions(c.value), c.value).toEqual(c.positions);
    }
  });

  it("getPitchCategory", () => {
    for (const c of fixture.categories) {
      expect(getPitchCategory(c.text, c.value, c.verb), `${c.text} ${c.value}`).toBe(c.category);
    }
  });

  it("getKanaDiacriticInfo / convertKatakanaToHiragana", () => {
    for (const c of fixture.diacritics) {
      expect(getKanaDiacriticInfo(c.character), c.character).toEqual(c.info);
    }
    for (const c of fixture.kanaConversion) {
      expect(convertKatakanaToHiragana(c.text), c.text).toBe(c.hiragana);
    }
  });
});
