// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LyricUnit } from "./LyricUnit";

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div"); document.body.append(container);
  root = createRoot(container);
});
afterEach(async () => {
  await act(async () => { root.unmount(); });
  container.remove(); vi.unstubAllGlobals();
});
const unit = (onSeek: (sec: number) => void, onLookup?: () => void, startSec: number | null = 12.345, canSeek = true) => createElement(LyricUnit, {
  children: "𠮷", charIndex: 0, className: "lyric-unit", startSec, canSeek, onSeek, onLookup,
});

describe("real React lyric-unit clicks keep lookup and exact seeks", () => {
  it("a real DOM click seeks the unchanged source time and looks up once, without bubbling into the parent word/line action", async () => {
    const seek = vi.fn(); const lookup = vi.fn(); const parent = vi.fn();
    await act(async () => { root.render(createElement("button", { onClick: parent }, unit(seek, lookup))); });
    await act(async () => { container.querySelector("span")!.click(); });
    expect(seek.mock.calls).toEqual([[12.345]]);
    expect(lookup).toHaveBeenCalledTimes(1);
    expect(parent).not.toHaveBeenCalled();
  });
  it("un-tokenized real units seek without being overwritten by the stage line click", async () => {
    const seek = vi.fn(); const line = vi.fn();
    await act(async () => { root.render(createElement("p", { onClick: line }, unit(seek))); });
    await act(async () => { container.querySelector("span")!.click(); });
    expect(seek).toHaveBeenCalledWith(12.345);
    expect(line).not.toHaveBeenCalled();
  });
  it("lookup still works when audio belongs to another song or cannot seek", async () => {
    const seek = vi.fn(); const lookup = vi.fn();
    await act(async () => { root.render(unit(seek, lookup, 12.345, false)); });
    await act(async () => { container.querySelector("span")!.click(); });
    expect(seek).not.toHaveBeenCalled();
    expect(lookup).toHaveBeenCalledTimes(1);
  });
  it.each([null, NaN, Infinity, -1])("invalid/plain time %s never causes a fabricated seek, but preserves lookup", async (start) => {
    const seek = vi.fn(); const lookup = vi.fn();
    await act(async () => { root.render(unit(seek, lookup, start)); });
    await act(async () => { container.querySelector("span")!.click(); });
    expect(seek).not.toHaveBeenCalled();
    expect(lookup).toHaveBeenCalledTimes(1);
  });
  it("a repeated/non-BMP unit takes the current row's source start after rerendering", async () => {
    const seek = vi.fn(); const lookup = vi.fn();
    await act(async () => { root.render(unit(seek, lookup, 10)); });
    await act(async () => { container.querySelector("span")!.click(); });
    await act(async () => { root.render(unit(seek, lookup, 30)); });
    await act(async () => { container.querySelector("span")!.click(); });
    expect(seek.mock.calls).toEqual([[10], [30]]);
    expect(lookup).toHaveBeenCalledTimes(2);
  });
  it("keyboard activation works for units outside a lookup button", async () => {
    const seek = vi.fn();
    await act(async () => { root.render(unit(seek)); });
    await act(async () => { container.querySelector("span")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); });
    expect(seek).toHaveBeenCalledWith(12.345);
  });
  it("characters split by ruby share the original unit start and retain lookup", async () => {
    const seek = vi.fn(); const lookup = vi.fn(); const parent = vi.fn();
    await act(async () => { root.render(createElement("button", { onClick: parent },
      createElement("ruby", null, unit(seek, lookup), unit(seek, lookup), createElement("rt", null, "test")))); });
    const nodes = container.querySelectorAll("span");
    await act(async () => { nodes[0]!.click(); nodes[1]!.click(); });
    expect(seek.mock.calls).toEqual([[12.345], [12.345]]);
    expect(lookup).toHaveBeenCalledTimes(2);
    expect(parent).not.toHaveBeenCalled();
  });
});
