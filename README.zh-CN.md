<div align="center">

# JPOP Corpus Tool

[![license](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)
[![platform](https://img.shields.io/badge/platform-Windows%2010%2F11-lightgrey)](https://github.com/Yisoragoto/jpop-corpus-tool/releases)
[![built with](https://img.shields.io/badge/built%20with-Tauri%202%20%C2%B7%20React%2019%20%C2%B7%20Rust-orange)](#从源码运行)

面向日语学习者的本地 J-pop 语料库。把自己拥有的歌曲和 LRC 歌词导入后，跟着音频逐行读歌词、
点词查释义（Yomitan 那一套），再把刚听到的真实例句做成 Anki 卡片。

[English](README.md) · **简体中文**

<table>
<tr>
<td><img src="docs/screenshots/lyrics-fullscreen.webp" alt="全屏歌词"></td>
<td><img src="docs/screenshots/library.webp" alt="曲库"></td>
</tr>
<tr>
<td><img src="docs/screenshots/dictionary.webp" alt="词典"></td>
<td><img src="docs/screenshots/kwic.webp" alt="KWIC 检索"></td>
</tr>
<tr>
<td><img src="docs/screenshots/anki-page.webp" alt="Anki 页"></td>
<td><img src="docs/screenshots/scrape.webp" alt="刮削页"></td>
</tr>
</table>

</div>

> 本仓库只提供程序源码和构建工具，不包含商业歌曲、歌词、第三方词典、Anki 牌组或预生成语料数据库。

## 下载

去 [Releases](https://github.com/Yisoragoto/jpop-corpus-tool/releases) 下载最新的
`JPOP.Corpus.Tool_x.y.z_x64-setup.exe`（或 `.msi`）。Windows 10/11 需要 **WebView2 运行时**，
系统一般已自带，安装程序也会按需引导安装。

「设置 → 系统」里可以检查更新、看更新日志，也可以直接下载安装新版本——安装包只从本仓库的
Releases 取，装之前校验 SHA-256。自动安装默认关着。

0.2.0 起桌面端换成了 Tauri 2 + React + Rust（0.1.x 是 PyQt6）。旧版 Python 代码仍留在仓库的 `legacy/` 下作参考，
数据库 `corpus.db` 两版通用。

## 核心功能

### 曲库与播放

- 曲库按歌手分组，支持列表和封面网格两种视图；网格先显示歌手墙，点进去是他的歌。
- 歌词跟随播放逐行高亮，点一行跳到那一句，可以对单句循环。
- 倍速走 WSOLA 时间伸缩（**变速不变调**）；变调由内置的 Rubber Band 把整首歌渲染一次并缓存。
- 全屏歌词：背景是整张封面虚化铺满，播放条是盖在上面的毛玻璃。
- 振假名支持「只注汉字」和「整词注音」，字体、字号、行距、字距都可调。

### 查词（Yomitan 词典）

[Yomitan](https://github.com/yomidevs/yomitan) 是浏览器上的日语弹出式查词扩展（Yomichan 的继任者），
它定义了一套词典打包格式：一个 zip 里装着词条、按读音区分的词频、音高（アクセント）和结构化释义。
**本工具查词这一块是按 Yomitan 的做法移植到 Rust 的**，所以可以直接吃它那一套词典：

- 在「词典」页导入 Yomitan 格式的 zip，想装几本装几本，顺序可以拖着排——查词结果按这个顺序出。
- 活用形、片假名、罗马字都能查；从点的词一直到行尾做最长匹配，所以
  「打ち込んでいませんでした」点第一个词就能还原到「打ち込む」。
- 释义是结构化的（保留词典原本的层级、例句、图片），音高和词频都按读音分别显示。
- 曲库里点歌词上的词，右栏并列「词典」和「例句」两页：一页是释义，一页是这个词在你整个语料里出现过的句子。

**词典本身要自己准备，仓库里不含任何词典。** 常用的几本（JMdict / 三省堂 / 明鏡 / JPDB 词频 / アクセント辞典 等）
可以在 [MarvNC/yomitan-dictionaries](https://github.com/MarvNC/yomitan-dictionaries) 找到下载地址。
各词典的版权和再分发条件由各自作者决定，自行确认。

![全屏歌词里点词](docs/screenshots/lyrics-lookup.webp)

### 检索与分析

- KWIC 语境检索：关键词居中对齐，可按歌手、词性、是否跨行、是否仅日文筛选，结果可导出 CSV。
- 词频统计（词元 × 词性，带 JLPT 等级）、按年份的时间线、语料报告 TXT 导出。
- 人物维度：演唱 / 作曲 / 作词 / 编曲，以及合作者图谱。

![分析页](docs/screenshots/analytics.webp)

### Anki 制卡

[Anki](https://apps.ankiweb.net/) 是开源的间隔重复记忆软件，
[AnkiConnect](https://ankiweb.net/shared/info/2055492159) 是它的插件，开一个本机接口让别的程序往里加卡片——
本工具就是通过它制卡的，**只连本机，不上传任何东西**。

- 查词结果上点「＋ 制卡」，词、释义、词频、音高和真实例句一起进 Anki。
- 自带「Lyrics」笔记类型：照 [Lapis](https://github.com/donkuri/lapis) 改的，背面右侧是歌曲封面，
  封面旁边竖排歌名、歌手、专辑。Anki 里没有这个类型时第一次制卡会自动建。
- 例句是你点的那一行，这首歌里其他含这个词的句子接在后面，**每句都从歌里切一段对应的音频**。
- 可选按歌手放进「牌组::歌手」子牌组；能读 Anki 的学习状态，排除已经学过的词。
- 挖词报告（HTML）按 JLPT 分级列出语料里还没做成卡片的词。

![Lyrics 笔记类型的卡片背面](docs/screenshots/anki-lyrics-card.webp)

### 导入与元数据

- 导入：**选一个目录，或者直接选几首歌**；扫描只读取，复核计划之后才写库；
  **不覆盖本地文件里的原始 metadata**。
- 歌词：导入时自动认音频旁边的同名 `.lrc`。没有的话——
  - 「设置 → 曲库维护 → 补齐缺失歌词」批量补：先看音频旁边有没有 `.lrc`，没有再上网搜（网易云）；
  - 曲库里单首可以「在线补齐」，或者「导入歌词文件…」用自己手上那一份
    （`.lrc` / `.txt`，UTF-8、Shift-JIS、GBK 都认）；
  - **挑不准就不要**：曲名、歌手、时长都要对得上才采纳——同名曲太多，塞错一首的歌词比没有歌词糟得多。
    挑不出来的那几首留给你自己导。
  - 补回来的歌词存进 `raw/lyrics_lrc/{song_id}.lrc`，和导入进来的一样；换歌词时你校正过的分词
    会按原文套回去，不会丢。
- 刮削：iTunes → MusicBrainz 识别曲目，封面取 Cover Art Archive，歌手照片取 Deezer；
  置信度不够的停在「需确认」等人工看一眼。
- 旧库在别的地方（比如 0.1.x 那个项目目录）：「设置 → 曲库维护 → 从别的语料库目录迁移数据」
  先预览要搬多少，再把歌、歌词、分词、署名、收听记录和词典**合并**进当前库；
  当前库已经有的那几首跳过，音频文件不复制，原路径照旧能放。

## 第一次使用

1. 进「导入」，选一个装着自己合法拥有的音频的目录，或者用「选单曲…」直接挑几首；
   同目录下同名的 `.lrc` 会自动匹配。
2. 看过扫描计划再决定导不导。
3. 导进来没有歌词的话，去「设置 → 曲库维护 → 补齐缺失歌词」，或者在曲库里单首处理。
4. 想要封面和 metadata，去「刮削」页跑一遍。
5. 想查词，去「词典」页导入 Yomitan 格式的 zip（0.1.x 登记过的词典包可以一键迁移）。
6. 要制卡就启动 Anki 并装上 AnkiConnect（见下）。

### 数据目录

`corpus.db`、`raw/audio`、`raw/lyrics_lrc`、`raw/covers`、`dictionaries.db` 的查找顺序：

1. 环境变量 `JPOP_CORPUS_HOME` 指向的目录；
2. 设置页里选过的目录（记在 `%LOCALAPPDATA%\JPOP Corpus Tool\settings.json`）；
3. 从可执行文件往上找，第一个**建好表**的库；
4. 当前工作目录；
5. 都没有就用 `%LOCALAPPDATA%\JPOP Corpus Tool`，**并在那里新建一个空库**。

所以新装的程序双击就能开，第一次进去是一个空库。**已经有库的人**（比如 0.1.x 用过来的）
去「设置 → 关于 → 语料库 → 切换目录…」选那个目录，重启就接上了——数据目录不放在安装目录下，
卸载不会把你的歌和语料一起删掉。

> **振假名、在歌词里点词、导入时分词，都需要 Sudachi 词典。** 安装包里不带它（解开 207 MB）。
> 从 0.2.5 起程序会帮你下：缺词典时会弹出一条横幅，「设置 → 歌词 → 分词词典（Sudachi）」里也有「下载」按钮——
> 从本仓库 `sudachi-dict-core` 那个发布下一个 43 MB 的 `.xz`，按固定的 SHA-256 校验，解压到语料库目录的
> `sudachi/` 里。每个语料库目录只下一次。同一张卡片也可以从手头已有的目录（0.1.x 的项目目录或它的 venv）复制。
>
> 词典的查找顺序：
>
> 1. 语料库目录里的 `venv/Lib/site-packages/sudachipy` + `sudachidict_core`——0.1.x 的布局，仍然认；
> 2. 语料库目录里的 `sudachi/`——下载的、在设置里复制进来的、或者迁移时带过来的；
> 3. 程序所在目录的 `resources/sudachi/`——安装包不带，只有你自己放了才会用到。
>
> 没有它时播放、「词典」页、检索、制卡照常。

### 配置 Anki

1. 安装并启动 [Anki](https://apps.ankiweb.net/)。
2. 安装 [AnkiConnect（插件代码 2055492159）](https://ankiweb.net/shared/info/2055492159)。
3. 在「设置 → 制卡」里选好牌组和笔记类型（默认用自带的「Lyrics」，没有就第一次制卡时自动建）。

AnkiConnect 默认只监听本机地址（`http://127.0.0.1:8765`），本工具不会把你的语料或 Anki 数据传到任何地方。
制卡**只追加不覆盖**：同一个词再制一次会跳过，不会动你已有的卡片。

## 从源码运行

### 需要

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://rustup.rs/) stable，以及 MSVC 的 C++ 生成工具（SQLite、LAME、Rubber Band 都是从源码编的）
- Windows 10/11 和 WebView2 运行时

### 开发模式

```powershell
git clone https://github.com/Yisoragoto/jpop-corpus-tool.git
cd jpop-corpus-tool\app
npm install
npm run app:dev
```

### 打包

```powershell
npm run app:build    # 产物在 rust/target/release/bundle/
```

### 只跑检查

```powershell
cd app
npm run typecheck
npm test
cd ..\rust
cargo test --workspace
cargo clippy --workspace --all-targets
```

首次启动会在数据目录里建空库。仓库中的 `examples/` 提供数据格式示例，不含真实歌曲或歌词。

### 目录说明

```text
app/           前端（React 19 + TypeScript strict + Vite）
rust/          Rust workspace
  jp-app/        Tauri 壳：command、状态、路径授权
  jp-corpus/     数据层，SQL 全在这里
  jp-tokenizer/  分词与 UPOS 映射（Sudachi）
  jp-audio/      播放引擎：WSOLA 变速、频谱、单句循环
  jp-dict/       Yomitan 式查词与词典导入
  jp-anki/       Anki 制卡与挖词报告
  jp-scraper/    曲目识别、封面、歌手照片、在线歌词
  jp-import/     扫描、计划、写库
  jp-normalize/  曲名 / 歌手名归一化
docs/          每个子系统的设计与对账记录
legacy/        0.1.x 的 PyQt 版，保留作参考：gui.py、dialogs/、scripts/、packaging/
scripts/       发版前的自检脚本（release-check.ps1）
assets/fonts/  随软件分发的 OFL 字体及许可证
examples/      不含版权内容的数据格式示例
raw/           本地音频和歌词目录，Git 默认忽略内容
```

`docs/` 里记的是每个子系统**怎么做的、为什么这么做、和旧版逐项对过哪些数字**。

## 依赖

| 名称 | 许可证 |
|---|---|
| [Tauri 2](https://tauri.app/) | Apache-2.0 / MIT |
| [React 19](https://react.dev/) | MIT |
| [rodio](https://github.com/RustAudio/rodio) · [rodio-wsola](https://crates.io/crates/rodio-wsola) | MIT / Apache-2.0 · Apache-2.0 |
| [Symphonia](https://github.com/pdeljanov/Symphonia)（解码） | MPL-2.0 |
| [sudachi.rs](https://github.com/WorksApplications/sudachi.rs) | Apache-2.0 |
| [rusqlite](https://github.com/rusqlite/rusqlite) · SQLite | MIT · Public Domain |
| [ureq](https://github.com/algesten/ureq) | MIT / Apache-2.0 |
| [serde](https://serde.rs/) | MIT / Apache-2.0 |

## 致谢与出处

| 名称 | 用在哪 | 许可证 |
|---|---|---|
| [Yomitan](https://github.com/yomidevs/yomitan) | 查词、活用还原、结构化释义、音高显示 | GPL-3.0-or-later |
| [Lapis](https://github.com/donkuri/lapis) | 自带「Lyrics」笔记类型的底本 | GPL-3.0 |
| [hoshidicts](https://github.com/Manhhao/hoshidicts) | 词典存储设计（未复制代码） | GPL-3.0-or-later |
| [Klee One](https://fonts.google.com/specimen/Klee+One) · [霞鹜文楷](https://github.com/lxgw/LxgwWenKai) | 随软件分发的字体 | SIL OFL 1.1 |
| [Rubber Band Library](https://breakfastquay.com/rubberband/) | 变调。源码在 [`third_party/rubberband/`](third_party/rubberband/)，编进程序里 | GPL-2.0-or-later |
| [LAME](https://lame.sourceforge.io/) | Anki 卡片上音频片段的 MP3 编码，编进程序里 | LGPL-2.0-or-later |

逐文件的出处写在各文件头部，完整说明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

## 版权边界

请勿提交或再分发以下内容：

- 商业歌曲音频或未经授权的歌词、LRC 文件；
- 包含真实歌词的 `corpus.db`、处理后语料或 Anki 牌组；
- 无再分发许可的第三方 Yomitan 词典；
- 本机设置、缓存、备份和用户路径。

本工具用于处理用户自行提供且有权使用的材料。使用者应自行确认所在地区适用的版权和合理使用规则。

## 许可证

程序源码使用 [GNU General Public License v3.0 或更高版本](LICENSE)（GPL-3.0-or-later）。
Copyright (C) 2026 Yisoragoto and JPOP Corpus contributors。
