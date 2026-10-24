import { describe, expect, it } from "vitest";

import {
  clearLine,
  defaultUnits,
  initialUnits,
  ktvLayout,
  lineStatus,
  mergeNext,
  nudge,
  splitUnit,
  tapDown,
  tapUp,
  toSavePayload,
  unitText,
  type EditLine,
} from "./karaoke";
import type { WordTiming } from "./wordTiming";

const texts = (line: EditLine) => line.units.map((u) => unitText(line, u));
const line = (text: string): EditLine => ({ text, timeSec: null, units: defaultUnits(text) });

describe("默认切分", () => {
  it("一个字一拍，拗音小字并进前一个字，标点和空白跟着前一个字", () => {
    expect(texts(line("きゃりー、ぱみゅ"))).toEqual(["きゃ", "り", "ー、", "ぱ", "みゅ"]);
    expect(texts(line("夜に 駆ける"))).toEqual(["夜", "に ", "駆", "け", "る"]);
  });
  it("拉丁字母和数字连成一个词，行首的标点并进第一个字", () => {
    expect(texts(line("「Hello world 123」"))).toEqual(["「Hello ", "world ", "123」"]);
  });
  it("位置按 Unicode 字符算，不按 UTF-16", () => {
    const l = line("𠮷野家");
    expect(l.units.map((u) => [u.charStart, u.charEnd])).toEqual([[0, 1], [1, 2], [2, 3]]);
  });
});

describe("初始单位", () => {
  const t = (charStart: number, charEnd: number, startSec: number, endSec: number | null, source = "lrc"): WordTiming =>
    ({ utteranceId: 1, charStart, charEnd, startSec, endSec, source });

  it("已有的真实单位原样保留，没覆盖的字按拍切开不带时间", () => {
    const units = initialUnits("夜に駆ける", [t(0, 2, 10, 10.5), t(4, 5, 11, 11.4)]);
    expect(units).toEqual([
      { charStart: 0, charEnd: 2, startSec: 10, endSec: 10.5 },
      { charStart: 2, charEnd: 3, startSec: null, endSec: null },
      { charStart: 3, charEnd: 4, startSec: null, endSec: null },
      { charStart: 4, charEnd: 5, startSec: 11, endSec: 11.4 },
    ]);
  });
  it("带起止的手动校准优先于同范围的来源；只有起点的手动校准不参与；重叠的丢掉", () => {
    const units = initialUnits("夜に", [t(0, 1, 10, 10.2), t(0, 1, 10.1, 10.3, "manual"), t(1, 2, 12, null, "manual"), t(0, 2, 9, 9.5)]);
    expect(units[0]).toEqual({ charStart: 0, charEnd: 1, startSec: 10.1, endSec: 10.3 });
    expect(units[1]).toEqual({ charStart: 1, charEnd: 2, startSec: null, endSec: null });
  });
});

describe("打点", () => {
  const lines = (): EditLine[] => [line("夜に"), line("あ")];

  it("按住模式：按下记开始，松开记结尾并前进，行尾跳到下一行", () => {
    let state = tapDown(lines(), { line: 0, unit: 0 }, 10, "hold");
    expect(state.cursor).toEqual({ line: 0, unit: 0 });
    let up = tapUp(state.lines, state.cursor, 10.4, "hold");
    expect(up.cursor).toEqual({ line: 0, unit: 1 });
    state = tapDown(up.lines, up.cursor, 10.6, "hold");
    up = tapUp(state.lines, state.cursor, 11, "hold");
    expect(up.cursor).toEqual({ line: 1, unit: 0 });
    expect(up.lines[0]!.units.map((u) => [u.startSec, u.endSec])).toEqual([[10, 10.4], [10.6, 11]]);
  });

  it("点按模式：按下就前进，前一个字的结尾等于这一下；一行最后一个字等松开", () => {
    let state = tapDown(lines(), { line: 0, unit: 0 }, 10, "tap");
    expect(state.cursor).toEqual({ line: 0, unit: 1 });
    // 中间的松开不起作用
    const ignored = tapUp(state.lines, { line: 0, unit: 0 }, 10.1, "tap");
    expect(ignored.lines[0]!.units[0]!.endSec).toBeNull();
    state = tapDown(state.lines, state.cursor, 10.5, "tap");
    expect(state.lines[0]!.units[0]!.endSec).toBe(10.5);
    expect(state.cursor).toEqual({ line: 0, unit: 1 });
    const up = tapUp(state.lines, state.cursor, 11, "tap");
    expect(up.lines[0]!.units[1]).toMatchObject({ startSec: 10.5, endSec: 11 });
    expect(up.cursor).toEqual({ line: 1, unit: 0 });
  });

  it("重打一行的开头时，后面比它早的旧时间清掉，不留下倒退的时间", () => {
    const done = tapUp(tapDown(lines(), { line: 0, unit: 1 }, 5, "hold").lines, { line: 0, unit: 1 }, 5.5, "hold").lines;
    const again = tapDown(done, { line: 0, unit: 0 }, 6, "hold").lines;
    expect(again[0]!.units[1]).toMatchObject({ startSec: null, endSec: null });
    expect(lineStatus(again, 0, null).status).not.toBe("error");
  });

  it("松开太快也不会出现零长度", () => {
    const down = tapDown(lines(), { line: 0, unit: 0 }, 10, "hold");
    const up = tapUp(down.lines, down.cursor, 10, "hold");
    expect(up.lines[0]!.units[0]!.endSec).toBeCloseTo(10.03);
  });
});

describe("手动调整", () => {
  const timed = (): EditLine[] => [{
    text: "夜に駆",
    timeSec: 10,
    units: [
      { charStart: 0, charEnd: 1, startSec: 10, endSec: 10.5 },
      { charStart: 1, charEnd: 2, startSec: 10.5, endSec: 11 },
      { charStart: 2, charEnd: 3, startSec: 11.5, endSec: 12 },
    ],
  }];

  it("贴着的边界一起挪，不越过邻居；有空隙的边界单独挪", () => {
    const moved = nudge(timed(), { line: 0, unit: 1 }, "start", 0.1);
    expect(moved[0]!.units[0]!.endSec).toBeCloseTo(10.6);
    expect(moved[0]!.units[1]!.startSec).toBeCloseTo(10.6);
    const far = nudge(timed(), { line: 0, unit: 1 }, "start", -5);
    expect(far[0]!.units[1]!.startSec).toBeCloseTo(10.03);
    const end = nudge(timed(), { line: 0, unit: 1 }, "end", 2);
    expect(end[0]!.units[1]!.endSec).toBe(11.5);
    expect(end[0]!.units[2]!.startSec).toBe(11.5);
  });

  it("拆开、合并、清空一行", () => {
    const merged = mergeNext(timed(), { line: 0, unit: 0 });
    expect(merged[0]!.units[0]).toEqual({ charStart: 0, charEnd: 2, startSec: 10, endSec: 11 });
    const split = splitUnit(merged, { line: 0, unit: 0 });
    expect(split[0]!.units.slice(0, 2)).toEqual([
      { charStart: 0, charEnd: 1, startSec: 10, endSec: 10.5 },
      { charStart: 1, charEnd: 2, startSec: 10.5, endSec: 11 },
    ]);
    expect(splitUnit(timed(), { line: 0, unit: 0 })).toEqual(timed());
    expect(lineStatus(clearLine(timed(), 0), 0, null).status).toBe("empty");
  });

  it("原来的数据不被改动（撤销靠旧的那份）", () => {
    const before = timed();
    const snapshot = JSON.stringify(before);
    nudge(before, { line: 0, unit: 0 }, "end", 0.2);
    splitUnit(before, { line: 0, unit: 0 });
    tapDown(before, { line: 0, unit: 2 }, 3, "hold");
    expect(JSON.stringify(before)).toBe(snapshot);
  });
});

describe("状态和保存", () => {
  it("完整、部分、越过下一行、倒退", () => {
    const lines: EditLine[] = [
      { text: "あい", timeSec: 1, units: [{ charStart: 0, charEnd: 1, startSec: 1, endSec: 1.5 }, { charStart: 1, charEnd: 2, startSec: 1.5, endSec: 2 }] },
      { text: "う", timeSec: 1.8, units: [{ charStart: 0, charEnd: 1, startSec: null, endSec: null }] },
      { text: "え", timeSec: 3, units: [{ charStart: 0, charEnd: 1, startSec: 3, endSec: null }] },
    ];
    expect(lineStatus(lines, 0, null)).toEqual({ status: "partial", reason: "overlapNext" });
    expect(lineStatus(lines, 0, 1.9).status).toBe("partial");
    expect(lineStatus(lines, 1, null).status).toBe("empty");
    expect(lineStatus(lines, 2, null).status).toBe("partial");
    const fixed = lines.map((l, i) => (i === 1 ? { ...l, timeSec: 2.2 } : l));
    expect(lineStatus(fixed, 0, null).status).toBe("done");
    const backwards: EditLine[] = [{ text: "あい", timeSec: 1, units: [{ charStart: 0, charEnd: 1, startSec: 2, endSec: 2.5 }, { charStart: 1, charEnd: 2, startSec: 1, endSec: 1.2 }] }];
    expect(lineStatus(backwards, 0, null).status).toBe("error");
  });

  it("存的时候只带打过的单位", () => {
    const payload = toSavePayload([{ text: "あい", timeSec: 1, units: [{ charStart: 0, charEnd: 1, startSec: 1, endSec: 1.5 }, { charStart: 1, charEnd: 2, startSec: null, endSec: null }] }]);
    expect(payload).toEqual([{ timeSec: 1, text: "あい", units: [{ charStart: 0, charEnd: 1, startSec: 1, endSec: 1.5 }] }]);
  });
});

describe("KTV 两行交替", () => {
  const lines = [
    { start: 10, end: 13 },
    { start: 13.5, end: 16 },
    { start: null, end: null }, // 没有时间的行跳过
    { start: 16.2, end: 19 },
    { start: 30, end: 33 },
  ];

  it("前奏显示歌名，第一句前倒数", () => {
    expect(ktvLayout(lines, 2)).toMatchObject({ intro: true, slots: [0, 1], active: 0, countdown: 0 });
    expect(ktvLayout(lines, 7.5)).toMatchObject({ intro: false, countdown: 3 });
    expect(ktvLayout(lines, 9.2)).toMatchObject({ countdown: 1 });
  });

  it("唱完上一行，它的位置换成后面第二行", () => {
    expect(ktvLayout(lines, 11).slots).toEqual([0, 1]);
    expect(ktvLayout(lines, 14).slots).toEqual([3, 1]);
    expect(ktvLayout(lines, 14).active).toBe(1);
    expect(ktvLayout(lines, 17).slots).toEqual([3, 4]);
  });

  it("连着唱不倒数，长间奏后倒数；唱完全部就空了", () => {
    expect(ktvLayout(lines, 13.2).countdown).toBe(0);
    expect(ktvLayout(lines, 27.5).countdown).toBe(3);
    expect(ktvLayout(lines, 40)).toMatchObject({ slots: [null, null], active: null });
  });
});
