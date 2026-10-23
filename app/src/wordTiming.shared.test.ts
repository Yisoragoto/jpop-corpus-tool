import { describe, expect, it } from "vitest";
import fixtures from "../../macos/Tests/JPCoreTests/Fixtures/LyricTimeline.json";
import { charTimeline, validLyricTime } from "./wordTiming";

describe("Rust / Swift / TypeScript shared lyric timeline fixtures", () => {
  for (const fixture of fixtures) {
    it(fixture.name, () => {
      const timeline = charTimeline(fixture.line.text, fixture.line.timeSec, fixture.endSec,
        fixture.timings.filter((unit) => unit.utteranceId === fixture.line.utteranceId));
      const mode = timeline ? "karaoke" : validLyricTime(fixture.line.timeSec) ? "lineSynced" : "plain";
      expect(mode).toBe(fixture.expected.mode);
      if (!timeline) { expect(fixture.expected.units).toEqual([]); return; }
      const starts = fixture.expected.units.flatMap((unit) => Array<number>(unit.charEnd - unit.charStart).fill(unit.startSec));
      const ends = fixture.expected.units.flatMap((unit) => Array<number>(unit.charEnd - unit.charStart).fill(unit.endSec));
      expect(timeline.starts).toEqual(starts);
      expect(timeline.ends).toEqual(ends);
    });
  }
});
