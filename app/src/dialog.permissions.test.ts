import { afterEach, expect, it, vi } from "vitest";
import { confirm } from "@tauri-apps/plugin-dialog";
import capability from "../../rust/crates/jp-app/capabilities/default.json";

afterEach(() => vi.unstubAllGlobals());

it.each(["Ok", "Cancel"])("切换 debug 的原生确认入口获准，且保留 %s 结果", async (answer) => {
  // BUG-0002：SDK 的 confirm 实际调用 message；只测后端设置写入抓不到发布后的 ACL 拒绝。
  const invoke = vi.fn(async (command: string) => {
    expect(command).toBe("plugin:dialog|message");
    expect(capability.windows).toContain("main");
    expect(capability.permissions, `${command} 缺少主窗口权限`).toContain("dialog:allow-message");
    return answer;
  });
  vi.stubGlobal("window", { __TAURI_INTERNALS__: { invoke } });
  expect(await confirm("Switch to debug?", { kind: "warning" })).toBe(answer === "Ok");
  expect(invoke).toHaveBeenCalledOnce();
});
