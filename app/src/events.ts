/**
 * 后端事件（进度、完成）的 `listen`：和 `@tauri-apps/api/event` 的一样，只是收到的载荷先过一遍
 * `trPayload`——里面的报错、提示句在英文界面下换成英文。各页面都从这里 import。
 */

import { listen as tauriListen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";

import { trPayload } from "./i18n";

export function listen<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  return tauriListen<T>(event, (e) => handler({ ...e, payload: trPayload(e.payload) }));
}
