// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { parseSettings } from "./settings";
import { resolveTheme, useTheme } from "./theme";

const native = vi.hoisted(() => ({ setTheme: vi.fn().mockResolvedValue(undefined), accent: vi.fn().mockResolvedValue(null) }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ setTheme: native.setTheme }) }));
vi.mock("./api", () => ({ api: { systemAccent: native.accent } }));

beforeEach(() => {
  native.setTheme.mockResolvedValue(undefined);
  native.accent.mockResolvedValue({ light2: "#aabbcc" });
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
});
afterEach(() => {
  vi.restoreAllMocks();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("style");
});

it("migrates existing preferences to the system theme and validates stored choices", () => {
  expect(parseSettings("{}").theme).toBe("system");
  expect(parseSettings('{"theme":"invalid"}').theme).toBe("system");
  expect(parseSettings('{"theme":"light"}').theme).toBe("light");
  expect(parseSettings('{"theme":"dark"}').theme).toBe("dark");
  expect(resolveTheme("system", false)).toBe("light");
  expect(resolveTheme("system", true)).toBe("dark");
});

it("syncs CSS and the native window, responds to system changes, and removes listeners", async () => {
  const media = new EventTarget() as MediaQueryList;
  let dark = false;
  Object.defineProperty(media, "matches", { get: () => dark });
  vi.spyOn(window, "matchMedia").mockReturnValue(media);
  const remove = vi.spyOn(media, "removeEventListener");
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  function Harness({ theme }: { theme: "system" | "light" | "dark" }) { useTheme(theme); return null; }
  await act(async () => root.render(createElement(Harness, { theme: "system" })));
  expect(document.documentElement.dataset.theme).toBe("light");
  expect(native.setTheme).toHaveBeenLastCalledWith("light");
  expect(document.documentElement.style.getPropertyValue("--sys-accent-light2")).toBe("#aabbcc");
  dark = true;
  await act(async () => media.dispatchEvent(new Event("change")));
  expect(document.documentElement.dataset.theme).toBe("dark");
  expect(native.setTheme).toHaveBeenLastCalledWith("dark");
  await act(async () => root.render(createElement(Harness, { theme: "light" })));
  await act(async () => media.dispatchEvent(new Event("change")));
  expect(document.documentElement.dataset.theme).toBe("light");
  await act(async () => root.unmount());
  expect(remove).toHaveBeenCalledWith("change", expect.any(Function));
  expect(native.accent).toHaveBeenCalledTimes(1);
  host.remove();
});
