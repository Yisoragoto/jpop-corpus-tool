# 第三方组件说明

本项目源码使用 MIT 协议。发布包还包含以下独立第三方组件，它们继续适用各自的许可证。

## 字体

- **Klee One SemiBold**：Copyright 2020 The Klee Project Authors，SIL Open Font License 1.1。完整许可证见 `assets/fonts/OFL-Klee.txt`。
- **LXGW WenKai Regular（霞鹜文楷）**：Copyright 2021-2026 LXGW，SIL Open Font License 1.1。完整许可证见 `assets/fonts/OFL-LXGWWenKai.txt`。

## FFmpeg

Windows Release 可能随软件附带独立运行的 `ffmpeg.exe`，用于音频片段导出、播放速度和音调处理。FFmpeg 并不适用本项目的 MIT 协议，其具体许可证由发布包中的构建配置决定。

- 项目与源码：[https://ffmpeg.org/](https://ffmpeg.org/)
- 下载与源码入口：[https://ffmpeg.org/download.html](https://ffmpeg.org/download.html)
- 每个发布包中的 `FFMPEG_BUILD_INFO.txt` 会记录所附二进制文件的版本、构建参数和许可证声明。

## Python 依赖

冻结后的桌面程序包含 `requirements.txt` 中列出的 Python 运行依赖。各依赖保留其原有许可证；可通过对应项目的发行元数据查看完整条款。
