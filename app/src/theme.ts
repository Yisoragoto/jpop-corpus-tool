/**
 * 外观主题和系统强调色。
 *
 * - `<html data-theme>` 决定用哪一套颜色（styles.css 里的深色 / 浅色两套 WinUI 主题资源）；
 *   「跟随系统」时跟着 `prefers-color-scheme` 实时变。
 * - 窗口主题一起切（`setTheme`）：亚克力的底色和系统标题栏才会跟着变，不然浅色界面配一条黑标题栏。
 * - 系统强调色启动时问一次 Rust（`system_accent`），写进 `--sys-accent-*`；读不到用 CSS 回退色。
 */

import { useEffect } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "./api";
import type { AppSettings } from "./settings";

/** 只问一次：React 严格模式下 effect 会跑两遍，别发两个请求 */
let accentRequest: ReturnType<typeof api.systemAccent> | undefined;

export function resolveTheme(theme: AppSettings["theme"], systemDark: boolean): "light" | "dark" {
  return theme === "system" ? (systemDark ? "dark" : "light") : theme;
}

export function useTheme(theme: AppSettings["theme"]) {
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const resolved = resolveTheme(theme, media.matches);
      document.documentElement.dataset.theme = resolved;
      if (isTauri()) void getCurrentWindow().setTheme(resolved).catch(console.warn);
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);

  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    accentRequest ??= api.systemAccent();
    void accentRequest.then((colors) => {
      if (!alive || !colors) return;
      for (const [name, value] of Object.entries(colors)) {
        document.documentElement.style.setProperty(`--sys-accent-${name}`, value);
      }
    }).catch(console.warn);
    return () => { alive = false; };
  }, []);
}
