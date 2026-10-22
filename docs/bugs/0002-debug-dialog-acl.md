# BUG-0002 切换 debug 的原生确认框被 ACL 拒绝

- 发现：2026-10-22 / 版本 0.3.3 / Windows
- 现象：设置中从 stable 选 debug，提示 `Command plugin:dialog|message not allowed by ACL`，轨道仍为 stable。
- 复现步骤：设置 → 更新与系统 → 更新轨道 → 调试版（debug）。
- 根因：`rust/crates/jp-app/capabilities/default.json` 仅允许 dialog 的 open/save；`UpdateCard.tsx` 新增的 SDK confirm 实际走 `plugin:dialog|message`，漏配 `dialog:allow-message`。原有测试只验后端轨道写入和关于页，没有覆盖 SDK→原生权限入口。
- 影响面：确认框在设置写入之前失败；曲库与更新包校验不受影响。已安装二进制必须重新构建/安装才能更新内嵌 ACL。
- 修复：☑ 根因修复（fec2f5e） ☑ 自动化测试（`app/src/dialog.permissions.test.ts`）
- 清理条件：无临时兼容层

回归直接调用实际安装的 SDK confirm，验证真实 IPC 命令获主窗口能力授权，并保留 Ok/Cancel 结果。
补权限前两条失败，补后通过；确认框异常也纳入 switchChannel 的错误捕获。日志：本地 `output/engineering/dialog-acl-{red,green}.log`。
