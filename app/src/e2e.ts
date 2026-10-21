/** 只在后端确认 JPOP_E2E=1 后装上测试入口；复用现有跨页导入请求，不另写导入流程。 */
import { invoke } from "@tauri-apps/api/core";

interface State { route: string; trackCount: number; libraryPath: string; scanning: boolean }
export interface Hooks { e2e_scan_folder(path: string): void; e2e_state(): State }
type Host = { __JPOP_E2E__?: Hooks };

export function registerHooks(enabled: boolean, host: Host, hooks: Hooks): void {
  if (enabled) host.__JPOP_E2E__ = hooks;
}

export function installHooks(scan: (path: string) => void): () => void {
  const host = window as Window & Host;
  let alive = true;
  let queued = false;
  const hooks: Hooks = {
    e2e_scan_folder(path) { queued = true; scan(path); },
    e2e_state() {
      const app = document.querySelector<HTMLElement>(".app[data-route]");
      const importer = document.querySelector<HTMLElement>("[data-import-plan]");
      if (importer?.dataset.importPlan === "true") queued = false;
      return { route: app?.dataset.route ?? "", trackCount: Number(app?.dataset.trackCount ?? 0),
        libraryPath: app?.dataset.libraryPath ?? "", scanning: queued || importer?.dataset.scanning === "true" };
    },
  };
  void invoke<{ enabled: boolean }>("e2e_state").then((gate) => {
    if (alive) registerHooks(gate.enabled, host, hooks);
  }).catch(() => undefined);
  return () => { alive = false; if (host.__JPOP_E2E__ === hooks) delete host.__JPOP_E2E__; };
}
