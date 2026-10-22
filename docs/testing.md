# 本地重活与窗口测试

从仓库根目录运行。`scripts/heavy.ps1` 用仓库绝对路径生成命名互斥体，重活共用同一个队列：

```powershell
./scripts/heavy.ps1 -Command 'cargo test --workspace --no-fail-fast'
./scripts/heavy.ps1 -Command 'npm test'
./scripts/heavy.ps1 -Command 'node scripts/smoke.mjs' -TimeoutSeconds 1800
```

开两个终端同时运行时，第二个会打印「排队等待重活租约」。等待超时会失败；持有进程退出或被杀后内核释放锁。
不要只杀租约壳后继续保留它启动的重活子进程，手动取消时终止该进程树。不同 checkout 不共享租约；
若人为让不同 checkout 共用 target，需自行串行运行。CI 每个 job 有独立 checkout，不需要此包装。

窗口冒烟需要 Windows、WebView2、Node 22+（原生 WebSocket）、Rust 与已安装的前端依赖：

```powershell
./scripts/heavy.ps1 -Command 'node scripts/smoke.mjs'
```

默认先构建前端，再以 custom-protocol 构建应用，按顺序运行鼠标导入、键盘导入/播放、普通环境隔离三个场景。
这是因为 cargo test 会把同名 debug 二进制覆盖成连 Vite 的开发模式。可用 `--exe <exe>` 指定已经嵌入前端的二进制并跳过构建，
`--out <目录>` 指定报告目录，`--scenario mouse|keyboard|normal` 单跑；默认报告在 `output/smoke-<时间>`。
键盘播放需要输出设备，缺少时会失败并说明原因，不能把未验证的播放算作通过。

每个场景复制应用到全新临时目录，设置假的 LOCALAPPDATA、独立 WebView2 profile，自造 WAV 和歌词。
不会读真实曲库；从 0 首开始断言扫描、按钮导入至 1 首、全文检索节点。全文路径无需下载分词词典。
只用 DOM 和真实 Tab/Space 事件，不用坐标；阶段完成由轮询断言确认。
脚本阻断启动时的线上更新查询，避免远端版本变化引出的弹窗抢焦点；更新器本身由离线 transport 单元测试验收。
截图、健康状态、构建编号、错误及 JSON 报告保留，临时目录路径写入报告，便于失败后复查。

`JPOP_E2E=1` 才注册后端命令和前端 `window.__JPOP_E2E__`。普通环境验证两个命令不可调用且全局入口不存在。
前端测试入口复用现有跨页扫描请求；鼠标导入与键盘导入调用真实按钮事件，不能直接调用后端导入代替。

`node scripts/smoke.mjs <port> [output.json]` 保留给 release-check.ps1 的附着自检模式。
`scripts/check-skips.ps1` 列出显式 ignored 的外部前置条件；需要真实设备、词典或作者曲库的测试应显式 `--ignored` 另跑。
