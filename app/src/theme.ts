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
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api } from "./api";
import { UI_SCALE_LIMITS, UI_SCALE_STEP, getAppSettings, updateSettings, type AppSettings } from "./settings";

/** 下一档缩放：按一格走，夹在范围里；dir 为 0 时回到 100% */
export function stepUiScale(current: number, dir: -1 | 0 | 1): number {
  if (dir === 0) return 100;
  const [low, high] = UI_SCALE_LIMITS;
  const snapped = Math.round(current / UI_SCALE_STEP) * UI_SCALE_STEP;
  return Math.min(high, Math.max(low, snapped + dir * UI_SCALE_STEP));
}

/**
 * 整个界面的缩放。桌面端用 WebView 自己的缩放（文字、图标、控件、布局一起变，和 Ctrl+加号一样，不会糊）；
 * 浏览器里退回 CSS zoom。Ctrl + = / - / 0 也走这里，改的是设置，下次启动还是这个大小
 */
export function useUiScale(scale: number) {
  useEffect(() => {
    const factor = scale / 100;
    if (isTauri()) {
      void getCurrentWebview().setZoom(factor).catch(console.warn);
    } else {
      document.documentElement.style.zoom = scale === 100 ? "" : String(factor);
    }
  }, [scale]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!event.ctrlKey || event.altKey || event.metaKey) return;
      const dir = event.key === "=" || event.key === "+" ? 1 : event.key === "-" ? -1 : event.key === "0" ? 0 : null;
      if (dir === null) return;
      event.preventDefault();
      updateSettings({ uiScale: stepUiScale(getAppSettings().uiScale, dir) });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

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
