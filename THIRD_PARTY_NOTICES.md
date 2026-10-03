# 第三方组件说明

本项目源码使用 GPL-3.0-or-later 协议。0.2.x 的发布物有两样：Windows 安装包（`jp-app.exe`，界面代码和字体打包在 exe 里面），
以及第一次用到分词时按需下载的 Sudachi 词典。下面列出其中的第三方组件，它们继续适用各自的许可证。

## 移植的代码与数据

- **Yomitan**（[https://github.com/yomidevs/yomitan](https://github.com/yomidevs/yomitan)）：Copyright (C) 2023-2026 Yomitan Authors；Copyright (C) 2016-2022 Yomichan Authors，GPL-3.0-or-later。
  词典查词部分（`rust/crates/jp-dict`、`app/src/dict`）移植自 Yomitan：日语活用规则表与活用还原、文本预处理、查词与排序、词典导入、结构化内容与音高显示及相应样式、制卡字段模板与样式内联表（`structured-content-style.json`、`pronunciation-style.json`）；
  测试基准取自或由 Yomitan 源码生成（活用测试用例、测试词典 `valid-dictionary1` 与期望查词结果、注音分配用例）。各文件头部注明了对应的 Yomitan 源文件。
- **Lapis**（[https://github.com/donkuri/lapis](https://github.com/donkuri/lapis)）：GPL-3.0。「Lyrics」笔记类型的模板（`rust/crates/jp-anki/data/lyrics/`）改自 Lapis 的模板与样式表，文件头注明了出处和所基于的 commit，Lapis 的许可证原文放在同一目录的 `LICENSE-Lapis`。
- **hoshidicts**（[https://github.com/Manhhao/hoshidicts](https://github.com/Manhhao/hoshidicts)）：GPL-3.0-or-later。词典存储与查词流程参考了其设计，未复制代码。

## 字体

- **Klee One SemiBold**：Copyright 2020 The Klee Project Authors，SIL Open Font License 1.1。完整许可证见 `assets/fonts/OFL-Klee.txt`。
- **LXGW WenKai Regular（霞鹜文楷）**：Copyright 2021-2026 LXGW，SIL Open Font License 1.1。完整许可证见 `assets/fonts/OFL-LXGWWenKai.txt`。

## 安装包里的程序组件

`jp-app.exe` 链接了 248 个第三方 Rust crate（另有 9 个是本项目自己的 `jp-*`）和两个从源码编进来的 C / C++ 库（见下一节），界面打包了 5 个 npm 包。
主要的几个：

| 组件 | 用途 | 许可证 |
|---|---|---|
| [Tauri 2](https://github.com/tauri-apps/tauri)（`tauri`、`wry`、`tao`）、`@tauri-apps/api`、`@tauri-apps/plugin-dialog` | 窗口、WebView、前后端通信 | Apache-2.0 OR MIT（`tao` 为 Apache-2.0） |
| [webview2-com](https://github.com/wravery/webview2-rs) | 调用系统的 WebView2。**WebView2 运行时本身不随安装包分发**，用的是系统里的那一份 | MIT |
| [React 19](https://react.dev/)（`react`、`react-dom`、`scheduler`） | 界面 | MIT |
| [sudachi.rs](https://github.com/WorksApplications/sudachi.rs) v0.6.11 | 日语分词 | Apache-2.0 |
| [mp3lame-encoder](https://github.com/DoumanAsh/mp3lame-encoder)、[mp3lame-sys](https://github.com/DoumanAsh/mp3lame-sys) | LAME 的 Rust 绑定（LAME 本身见下一节） | LGPL-3.0 |
| [rodio](https://github.com/RustAudio/rodio)、[cpal](https://github.com/RustAudio/cpal)、`rodio-wsola` | 播放、音频输出、变速不变调 | rodio：MIT OR Apache-2.0；cpal、rodio-wsola：Apache-2.0 |
| [Symphonia](https://github.com/pdeljanov/Symphonia)（FLAC、MP3、AAC、PCM、Vorbis、MP4、Ogg、RIFF 各模块） | 音频解码 | **MPL-2.0**（文件级弱 copyleft；本项目未修改其源码，源码见 crates.io） |
| [rusqlite](https://github.com/rusqlite/rusqlite) + 内置的 [SQLite](https://sqlite.org/) | 语料库和词典库 | rusqlite：MIT；SQLite：公有领域 |
| [lofty](https://github.com/Serial-ATA/lofty-rs) | 读音频文件的标签和时长 | MIT OR Apache-2.0 |
| [rustfft](https://github.com/ejmahler/RustFFT)、[realfft](https://github.com/HEnquist/realfft) | 频谱 | MIT OR Apache-2.0；realfft：MIT |
| [ureq](https://github.com/algesten/ureq)、[rustls](https://github.com/rustls/rustls)、[ring](https://github.com/briansmith/ring)、[webpki-roots](https://github.com/rustls/webpki-roots) | 联网（刮削、歌词、更新、下载词典） | ureq：MIT OR Apache-2.0；rustls：Apache-2.0 OR ISC OR MIT；ring：Apache-2.0 AND ISC；webpki-roots：CDLA-Permissive-2.0 |
| [flate2](https://github.com/rust-lang/flate2-rs)、[lzma-rs](https://github.com/gendx/lzma-rs)、[encoding_rs](https://github.com/hsivonen/encoding_rs) | 解压词典包和词典、歌词编码回退 | flate2：MIT OR Apache-2.0；lzma-rs：MIT；encoding_rs：(Apache-2.0 OR MIT) AND BSD-3-Clause |
| ICU4X（`icu_*`、`zerovec` 等 15 个） | 经 `url` → `idna` 间接引入，解析网址里的国际化域名 | Unicode-3.0 |

全部 257 个 crate 的许可证字段（`cargo tree` 原样输出，未合并同义写法）：

| 许可证 | 个数 | | 许可证 | 个数 |
|---|---|---|---|---|
| MIT OR Apache-2.0 | 123 | | Zlib OR Apache-2.0 OR MIT | 2 |
| MIT | 29 | | MIT OR Apache-2.0 OR Zlib | 2 |
| Apache-2.0 OR MIT | 17 | | Zlib | 1 |
| Unicode-3.0 | 15 | | MIT OR Zlib OR Apache-2.0 | 1 |
| MIT/Apache-2.0 | 14 | | CDLA-Permissive-2.0 | 1 |
| MPL-2.0 | 13 | | CC0-1.0 OR MIT-0 OR Apache-2.0 | 1 |
| GPL-3.0-or-later（本项目自己的 crate） | 9 | | BSD-3-Clause/MIT | 1 |
| Unlicense/MIT | 4 | | BSD-3-Clause AND MIT | 1 |
| Unlicense OR MIT | 4 | | Apache-2.0 OR ISC OR MIT | 1 |
| Apache-2.0 | 4 | | Apache-2.0 OR BSL-1.0 | 1 |
| ISC | 3 | | Apache-2.0 AND MIT | 1 |
| BSD-3-Clause | 3 | | Apache-2.0 AND ISC | 1 |
| LGPL-3.0 | 2 | | Apache-2.0 / MIT | 1 |
| | | | 0BSD OR MIT OR Apache-2.0 | 1 |
| | | | (Apache-2.0 OR MIT) AND BSD-3-Clause | 1 |

清单这样得到（只算真正链进 exe 的：普通依赖、Windows 目标、去掉编译期用完就丢的 proc-macro）：

```bash
cd rust && cargo tree -p jp-app -e normal,no-proc-macro --target x86_64-pc-windows-msvc --prefix none --format "{p}|{l}"
cd app && npm ls --omit=dev --all
```

## 按需下载：Sudachi 词典

- **SudachiDict（core）** 20260116：Copyright Works Applications Co., Ltd.，Apache-2.0（[https://github.com/WorksApplications/SudachiDict](https://github.com/WorksApplications/SudachiDict)）。
  安装包不带词典。用到分词时，程序从本仓库 `sudachi-dict-core` 发布下载 `system.dic.xz`，校验 SHA-256 后解压到语料库目录的 `sudachi/`，
  内容就是 `sudachidict_core` 20260116 里的 `system.dic`，未做修改。

## 编进程序的 C / C++ 库

这两个库不是 Rust crate，是从源码编译、静态链接进 `jp-app.exe` 的。它们是 copyleft 许可证，
随二进制分发时要能拿到对应的源码：本项目整体是 GPL-3.0-or-later、源码公开，两个库的源码也都能从下面的位置原样取得。

- **Rubber Band Library** 4.0.0：Copyright 2007-2024 Particular Programs Ltd，**GPL-2.0-or-later**
  （[https://breakfastquay.com/rubberband/](https://breakfastquay.com/rubberband/)，[https://github.com/breakfastquay/rubberband](https://github.com/breakfastquay/rubberband)）。用于变调。
  源码放在本仓库的 `third_party/rubberband/`，取自上游 `v4.0.0` 标签（提交号记在同一目录的 `RUBBERBAND_COMMIT`），
  许可证原文是同一目录的 `COPYING`。只取了单文件构建用得到的部分：`single/`、`rubberband/`（头文件）、
  `src/` 下的 `common/`、`faster/`、`finer/` 和顶层的三个文件；没有取 `src/ext/`（KissFFT、Speex、pommier 等可选后端）、
  命令行程序、LADSPA / LV2 / Vamp 插件和各语言绑定。**未修改任何文件**。
  编译方式是上游的 `single/RubberBandSingle.cpp`（内置 FFT、内置重采样器，不依赖其它库），见 `rust/crates/jp-audio/build.rs`。
  上游另有商业许可证；本项目用的是 GPL。
- **LAME** 3.100：Copyright (c) 1999-2011 The LAME Project，**LGPL-2.0-or-later**（[https://lame.sourceforge.io/](https://lame.sourceforge.io/)）。
  用于把 Anki 卡片上的例句音频片段编成 MP3。源码随 `mp3lame-sys` 0.1.11 这个 crate 一起发布（crates.io 上的包里有完整的
  `lame-3.100/` 目录和它的 `COPYING`），构建时从那里编译，未修改。只编了编码器，没有编解码器（mpglib）。

0.2.8 及更早的版本不含这两个库，变调和音频片段靠用户自己安装的 FFmpeg；现在程序不再调用 FFmpeg。

## 许可证原文怎么提供

**现状**：安装包里没有附带上面这些组件的许可证原文，本文件也不在安装包里；它们在本仓库（本文件、`assets/fonts/` 下的 OFL 原文、
`rust/crates/jp-anki/data/lyrics/LICENSE-Lapis`、`third_party/rubberband/COPYING`）和各组件的发布页（crates.io、npm）上。`sudachi-dict-core` 那个发布里也只有
`system.dic.xz`，没有附 Apache-2.0 原文。MIT、BSD、Apache-2.0、MPL-2.0、GPL、LGPL 都要求随二进制分发时附上许可证文本或声明，
所以这是一个已知的、待补的缺口。

## 旧版（0.1.x）

0.1.x 是用 PyInstaller 冻结的 Python 程序，和 0.2.x 不是同一套代码：

- 冻结后的程序包含 `requirements.txt` 中列出的 Python 运行依赖，各依赖保留其原有许可证。
- Windows 发布包可能附带 `ffmpeg.exe`，每个发布包里的 `FFMPEG_BUILD_INFO.txt` 记录了所附二进制的版本、构建参数和许可证声明。
  FFmpeg 是独立程序（[https://ffmpeg.org/](https://ffmpeg.org/)），许可证是 LGPL-2.1+ 或 GPL-2.0+，取决于构建。
