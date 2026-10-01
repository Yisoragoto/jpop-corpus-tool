# JPOP Corpus Tool

一个面向日语学习者的本地 J-pop 语料库工具。把自己拥有的歌曲和 LRC 歌词导入后，可以检索真实歌词语境、跟着音频逐行读歌词、点词查释义，并把真实例句做成 Anki 卡片。

> 本仓库只提供程序源码和构建工具，不包含商业歌曲、歌词、第三方词典、Anki 牌组或预生成语料数据库。

**0.2.0 起，桌面端换成了 Tauri 2 + React + Rust**（0.1.x 是 PyQt6）。界面、播放、查词、刮削、制卡全部重写为原生实现，启动和检索都快了一个量级；旧版 Python 代码仍留在仓库里（见[目录说明](#目录说明)），数据库 `corpus.db` 两版通用。

## 核心功能

### 曲库与播放

- 曲库按歌手分组，支持列表和封面网格两种视图；网格先显示歌手墙，点进去是他的歌。
- 歌词跟随播放逐行高亮，点一行跳到那一句，可以对单句循环。
- 倍速走 WSOLA 时间伸缩（**变速不变调**），变调另走 FFmpeg 渲染并缓存。
- 全屏歌词：背景是整张封面虚化铺满，播放条是盖在上面的毛玻璃。
- 振假名支持「只注汉字」和「整词注音」，字体、字号、行距、字距都可调。

### 查词

- 导入 Yomitan 格式的词典 zip（词条、词频、音调），查词结果包含结构化释义、音高、按读音区分的词频。
- 活用形、片假名、罗马字都能查；从点的词一直到行尾做最长匹配，所以「打ち込んでいませんでした」点第一个词就能还原到「打ち込む」。
- 曲库里点歌词上的词，右栏并列「词典」和「例句」两页，例句来自全语料。

### 检索与分析

- KWIC 语境检索：关键词居中对齐，可按歌手、词性、是否跨行、是否仅日文筛选，结果可导出 CSV。
- 词频统计（词元 × 词性，带 JLPT 等级）、时间线、语料报告 TXT 导出。
- 人物维度：演唱 / 作曲 / 作词 / 编曲，以及合作者图谱。

### Anki 制卡

- 通过 AnkiConnect 把词和真实歌词例句发到 Anki，自带「Lyrics」笔记类型（照 Lapis 做的，背面有封面和歌名歌手专辑）。
- 例句是点的那一行，这首歌里其他含这个词的句子接在后面，每句都从歌里切一段音频。
- 可选按歌手放进「牌组::歌手」子牌组；能读 Anki 的学习状态，排除已学过的词。
- 挖词报告（HTML）按 JLPT 分级列出语料里还没做成卡片的词。

### 刮削与导入

- 导入：扫描目录、复核计划、写入语料；**不覆盖本地文件里的原始 metadata**。
- 刮削：iTunes → MusicBrainz 识别曲目，封面取 Cover Art Archive，歌手照片取 Deezer；置信度不够的停在「需确认」等人工看一眼。
- 「补齐缺失封面」只补空的，用刮削时存下的候选，不重新搜索。

## 下载与安装

打开仓库右侧的 **Releases**，下载最新版本的 `JPOP.Corpus.Tool_x.y.z_x64-setup.exe`（或 `.msi`）。Windows 10/11 需要 **WebView2 运行时**，系统一般已自带，安装程序也会按需引导安装。

数据目录（`corpus.db`、`raw/audio`、`raw/lyrics_lrc`、`raw/covers`、`dictionaries.db`）的查找顺序：

1. 环境变量 `JPOP_CORPUS_HOME` 指向的目录；
2. 从可执行文件往上找，第一个含 `corpus.db` 的目录；
3. 当前工作目录。

> **实时分词需要 Sudachi 词典。** 分词器从项目目录下的 `venv/Lib/site-packages/sudachipy`、`sudachidict_core` 读取，这是和 0.1.x 共用的那一份。没有它时查词、播放、检索、制卡照常，只有振假名和导入时的分词不可用——按 `requirements.txt` 建一个 venv 即可。

## 第一次使用

1. 启动软件，进入「导入」，选一个装着自己合法拥有的音频的目录；同目录下同名的 `.lrc` 会自动匹配。
2. 看过扫描计划再决定导不导；写库完成后曲库就有歌了。
3. 想要封面和 metadata，去「刮削」页跑一遍。
4. 想查词，去「词典」页导入 Yomitan 格式的 zip（也可以从 0.1.x 登记过的词典包一键迁移）。
5. 要制卡就启动 Anki 并装上 AnkiConnect。

## 配置 Anki

1. 安装并启动 [Anki](https://apps.ankiweb.net/)。
2. 安装 [AnkiConnect（插件代码 2055492159）](https://ankiweb.net/shared/info/2055492159)。
3. 在软件的 Anki 页面刷新连接状态，选好牌组和笔记类型。

AnkiConnect 默认只监听本机地址，本工具不会上传你的语料或 Anki 数据。

## 从源码运行

需要 [Node.js](https://nodejs.org/) 20+、[Rust](https://rustup.rs/) stable 和 Windows 10/11 的 WebView2 运行时。

```powershell
git clone https://github.com/Yisoragoto/jpop-corpus-tool.git
cd jpop-corpus-tool\app
npm install
npm run app:dev      # 开发模式，带热更新
npm run app:build    # 出安装包，产物在 rust/target/release/bundle/
```

只跑检查：

```powershell
cd app
npm run typecheck
npm test
cd ..\rust
cargo test --workspace
cargo clippy --workspace --all-targets
```

首次启动会在数据目录里建空库。仓库中的 `examples/` 提供数据格式示例，不含真实歌曲或歌词。

## 目录说明

```text
app/           前端（React 19 + TypeScript strict + Vite）
rust/          Rust workspace
  jp-app/        Tauri 壳：command、状态、路径授权
  jp-corpus/     数据层，SQL 全在这里
  jp-tokenizer/  分词与 UPOS 映射（Sudachi）
  jp-audio/      播放引擎：WSOLA 变速、频谱、单句循环
  jp-dict/       Yomitan 式查词与词典导入
  jp-anki/       Anki 制卡与挖词报告
  jp-scraper/    曲目识别、封面与歌手照片
  jp-import/     扫描、计划、写库
  jp-normalize/  曲名 / 歌手名归一化
docs/          每个子系统的设计与对账记录
dialogs/       0.1.x 的 PyQt 界面模块（旧版，保留作参考）
scripts/       0.1.x 的元数据与建库脚本
assets/fonts/  随软件分发的 OFL 字体及许可证
examples/      不含版权内容的数据格式示例
raw/           本地音频和歌词目录，Git 默认忽略内容
```

`docs/` 里记的是每个子系统**怎么做的、为什么这么做、和旧版逐项对过哪些数字**——移植时的对账记录都在那里。

## 版权边界

请勿提交或再分发以下内容：

- 商业歌曲音频或未经授权的歌词、LRC 文件；
- 包含真实歌词的 `corpus.db`、处理后语料或 Anki 牌组；
- 无再分发许可的第三方 Yomitan 词典；
- 本机设置、缓存、备份和用户路径。

本工具用于处理用户自行提供且有权使用的材料。使用者应自行确认所在地区适用的版权和合理使用规则。

## 许可证

程序源码使用 [GNU General Public License v3.0 或更高版本](LICENSE)（GPL-3.0-or-later）。Copyright (C) 2026 Yisoragoto and JPOP Corpus contributors。

查词部分移植自 [Yomitan](https://github.com/yomidevs/yomitan)（GPL-3.0-or-later），「Lyrics」笔记类型改自 [Lapis](https://github.com/donkuri/lapis)（GPL-3.0）。内置字体、可选 FFmpeg 及其它第三方组件适用各自许可证，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
