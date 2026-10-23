# BUG-0007 全库逐字补歌词不限速

- 发现：2026-10-23 / 0.3.3 之后的 codex/validated-lyric-sources / 全平台
- 现象：「一键补全逐字歌词」对每首歌向网易云、QQ、酷狗、AMLL、LRCLIB 连发请求，歌与歌之间也不停，整库两百多首一口气打过去。
- 复现步骤：刮削页点「一键补全逐字歌词」，看请求间隔。
- 根因：旧的补歌词（只走网易云）客户端带 600ms 限速；新的批量队列改用 `search_with_transport`，传进去的 `CancellableTransport` 只管取消，没有限速。
- 影响面：全库升级、导入后自动补歌词；这些是非公开接口，被限流或封 IP 后整批失败。
- 修复：☑ 根因修复（`rust/crates/jp-app/src/lyric_upgrade.rs`：同一主机两次请求至少隔 600ms，跨歌共享；等待中按停止立即返回） ☑ 自动化测试（`requests_to_one_host_are_spaced_but_other_hosts_are_not_held_up`）
- 清理条件：无
