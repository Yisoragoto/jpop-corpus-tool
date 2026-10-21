import { expect, it } from "vitest";
import { registerHooks, type Hooks } from "./e2e";

it("普通运行完全没有测试入口，只有明确开启才注册", () => {
  const host: { __JPOP_E2E__?: Hooks } = {};
  const hooks: Hooks = { e2e_scan_folder: () => undefined, e2e_state: () => ({ route: "import", trackCount: 0, libraryPath: "fixture", scanning: false }) };
  registerHooks(false, host, hooks);
  expect("__JPOP_E2E__" in host).toBe(false);
  registerHooks(true, host, hooks);
  expect(host.__JPOP_E2E__).toBe(hooks);
});
