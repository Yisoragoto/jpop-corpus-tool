import { describe, expect, it } from "vitest";

import { buildAxisColumns } from "./releaseAxis";

const at = (...years: number[]) => years.map((year, i) => ({ year, id: i }));
const shape = (cols: ReturnType<typeof buildAxisColumns>) =>
  cols.map((c) => (c.type === "gap" ? `${c.from}…${c.to}` : `${c.year}:${c.items.length}${c.ended ? "!" : ""}`));

describe("buildAxisColumns", () => {
  it("一年一列，同一年的都进同一列", () => {
    expect(shape(buildAxisColumns(at(2019, 2019, 2020), null))).toEqual(["2019:2", "2020:1"]);
  });

  it("只空一年留一根细列，连着空两年以上折成一段", () => {
    expect(shape(buildAxisColumns(at(2017, 2019, 2024), null))).toEqual(["2017:1", "2018:0", "2019:1", "2020…2023", "2024:1"]);
  });

  it("解散那一年标出来，没发行也有一列", () => {
    expect(shape(buildAxisColumns(at(2016, 2017), 2020))).toEqual(["2016:1", "2017:1", "2018…2019", "2020:0!"]);
    expect(shape(buildAxisColumns(at(2016, 2018), 2018))).toEqual(["2016:1", "2017:0", "2018:1!"]);
  });

  it("没有发行就什么都不画", () => {
    expect(buildAxisColumns([], 2020)).toEqual([]);
  });
});
