# 第三方组件说明

本项目源码使用 GPL-3.0-or-later 协议。发布包还包含以下独立第三方组件，它们继续适用各自的许可证。

## 移植的代码与数据

- **Yomitan**（[https://github.com/yomidevs/yomitan](https://github.com/yomidevs/yomitan)）：Copyright (C) 2023-2026 Yomitan Authors；Copyright (C) 2016-2022 Yomichan Authors，GPL-3.0-or-later。
  词典查词部分（`rust/crates/jp-dict`、`app/src/dict`）移植自 Yomitan：日语活用规则表与活用还原、文本预处理、查词与排序、词典导入、结构化内容与音高显示及相应样式、制卡字段模板与样式内联表（`structured-content-style.json`、`pronunciation-style.json`）；
  测试基准取自或由 Yomitan 源码生成（活用测试用例、测试词典 `valid-dictionary1` 与期望查词结果、注音分配用例）。各文件头部注明了对应的 Yomitan 源文件。
- **Lapis**（[https://github.com/donkuri/lapis](https://github.com/donkuri/lapis)）：GPL-3.0。「Lyrics」笔记类型的模板（`rust/crates/jp-anki/data/lyrics/`）改自 Lapis 的模板与样式表，文件头注明了出处和所基于的 commit，Lapis 的许可证原文放在同一目录的 `LICENSE-Lapis`。
- **hoshidicts**（[https://github.com/Manhhao/hoshidicts](https://github.com/Manhhao/hoshidicts)）：GPL-3.0-or-later。词典存储与查词流程参考了其设计，未复制代码。

## 字体

- **Klee One SemiBold**：Copyright 2020 The Klee Project Authors，SIL Open Font License 1.1。完整许可证见 `assets/fonts/OFL-Klee.txt`。
- **LXGW WenKai Regular（霞鹜文楷）**：Copyright 2021-2026 LXGW，SIL Open Font License 1.1。完整许可证见 `assets/fonts/OFL-LXGWWenKai.txt`。

## FFmpeg

Windows Release 可能随软件附带独立运行的 `ffmpeg.exe`，用于音频片段导出、播放速度和音调处理。FFmpeg 是独立程序，不适用本项目的协议，其具体许可证由发布包中的构建配置决定。

- 项目与源码：[https://ffmpeg.org/](https://ffmpeg.org/)
- 下载与源码入口：[https://ffmpeg.org/download.html](https://ffmpeg.org/download.html)
- 每个发布包中的 `FFMPEG_BUILD_INFO.txt` 会记录所附二进制文件的版本、构建参数和许可证声明。

## Python 依赖

冻结后的桌面程序包含 `requirements.txt` 中列出的 Python 运行依赖。各依赖保留其原有许可证；可通过对应项目的发行元数据查看完整条款。
