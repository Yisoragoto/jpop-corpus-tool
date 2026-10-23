# BUG-0001 聚焦导入或播放按钮时空格被全局热键吞掉

- 发现：2026-10-21 / 版本 0.3.2 / Windows WebView2
- 现象：音频设备可用时，按钮有焦点但空格不激活；导入计划停在 0 首。
- 复现步骤：空库扫描一份音频；Tab 到「导入 1 首」，按 Space。导入没有发生。
- 根因：`app/src/useHotkeys.ts:37` 原来只排除输入框；冒泡到 window 的按钮 Space 被 preventDefault，再交给全局 audioToggle，浏览器无法在 keyup 激活按钮。
- 影响面：导入、播放以及其他可聚焦按钮；已有自处理键盘的结果行也可能重复触发播放。
- 修复：☑ 根因修复（与本档案同一提交；`git log --diff-filter=A -- docs/bugs/0001-space-button.md` 可定位） ☑ 自动化测试（`scripts/smoke.mjs` 的 keyboard 场景）
- 清理条件：无临时兼容层

真实窗口先红后绿：修复前没有语言弹窗、audioReady=true，Tab 后 Space 导入断言超时；修复后曲数 0→1，播放按钮由 paused→playing。
证据位于本地 `output/engineering/F-keyboard-red-clean/report.json` 与 `F-smoke-green/report.json`（不提交临时数据）。
