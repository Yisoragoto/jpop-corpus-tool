# stable / debug 双轨更新

只有两条轨道。`main` push 自动构建固定 tag `debug`，删旧 Release 和 tag 后重建预发布，
用 `--latest=false`，资产恰好为 `JPOP.Corpus.Tool_<version>-debug.exe` 与 `latest.json`；
Release 建好后 macOS 作业再挂上 `JPOP.Corpus.Tool_<version>-debug_macOS_arm64.dmg` 与 `latest-macos.json`
（同样的五个字段）。两个平台的构建编号由 workflow 的 `identity` 作业算一次，两个包里嵌的是同一个值。
Windows 客户端只读 `latest.json`、只认 `-debug.exe`；macOS 客户端只读 `latest-macos.json`、只认 `-debug_macOS_arm64.dmg`
（`jp_update::Platform`）。macOS 包比 Windows 包晚几分钟挂上，这期间 macOS 客户端检查会说明「还没有 macOS 构建」。
正式版同样两个平台都出：`JPOP.Corpus.Tool_<version>_macOS_arm64.dmg` 挂在 `v<version>` 上。
正式版只能手动 `workflow_dispatch` 选择 stable，使用当前三处一致的版本，创建 `v<version>` 正式 Release 并标 Latest；
默认不覆盖已有正式版。维护者明确授权同版本重发时，手动 stable 可选择 `replace_existing`，在构建检查与安装包完成后删除旧 Release/tag，再以本次源码重建。
同版本重发不会让已安装用户收到 stable 版本比较的更新提示，发布说明须提醒手动覆盖安装。
其他分支 push、PR 不走发布 workflow；v* tag push 不触发发布。

守卫是 checkout 后、构建前第一步。它从事件、ref、channel 产生受限输出，后续构建和发布只消费这些输出。
可通过 `./scripts/tests/release-policy.Tests.ps1` 离线验证五条规则。

更新偏好和库目录共用本机 `settings.json`：

```json
{ "libraryRoot": "...", "update_channel": "stable", "skipped_stable": "", "skipped_debug": "" }
```

改库目录保留更新偏好和未知字段；写设置在进程内排队，用同目录临时文件替换。
设置损坏时拒绝覆写，防止一次切轨道把用户的目录设置清空。

- stable 读取 `/releases/latest`，排除预发布，用 semver 判新，只允许升级。
- debug 读取 `/releases/tags/debug`，清单是构建身份、版本、文件、SHA-256 和字节数的唯一来源。
  只有 buildId 不同且版本不倒退才提醒；同一版本的新构建可以下载。
- debug Release 404 才回落 stable；缺清单、格式错、缺哈希、文件名/地址/大小不一致都报错并拒绝安装。
- 清单和安装包地址必须精确位于本仓库 debug tag 下；后端保有资产，前端只回传构建身份或版本号。
  缓存和新下载都要验证 debug 字节数，安装前继续重新算 SHA-256；INSTALL_LAUNCHED 逻辑保持原样。
- 切 debug 要确认；跳过记录分轨道保存。旧版 localStorage 的跳过版本只在 stable 生效。

CI 注入 `JPOP_BUILD_ID=<sha12>-<UTC yyyyMMddHHmm>`；本地由 build.rs 读取 Git SHA 和 UTC 时间生成，
源码或 HEAD 变化会重新生成，设置 → 关于显示编进二进制的构建编号。

更新器的 HTTP 可注入 `update::Transport`，测试不访问 GitHub。发布操作只在推入 main 或手动运行 workflow 时发生；
本地验证不会为测试创建或删除远端 Release。

同一轨道的发布排队执行，不互相取消：发布步骤先删旧 Release 再建新的，中途取消会留下空档。

`debug` 是会移动的 tag。本地克隆里已有旧的 `debug` 时，`git fetch` 会报 would clobber existing tag；
用 `git fetch --tags --force` 或先 `git tag -d debug` 再拉。不要基于 `debug` tag 开分支。
