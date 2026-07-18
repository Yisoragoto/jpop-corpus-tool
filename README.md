# JPOP Corpus Tool

一个面向日语学习者的本地 J-pop 语料库工具。把自己拥有的歌曲和 LRC 歌词导入后，可以检索真实歌词语境、逐行播放音频、查词并制作 Anki 卡片。

> 本仓库只提供程序源码和构建工具，不包含商业歌曲、歌词、第三方词典、Anki 牌组或预生成语料数据库。

## 核心功能

### 语料搜索

- 使用 SQLite FTS5 搜索歌词中的表面形或词元。
- 按歌手、词性、上下文长度和日文内容筛选。
- 以 KWIC 形式查看关键词左右语境，并从命中时间直接播放音频。
- 生成词频、词性分布和词汇覆盖率报告。

### Anki 制卡

- 通过 AnkiConnect 把选中的单词和真实歌词例句发送到 Anki。
- 可选择目标牌组、释义、例句数量和音频片段。
- 支持读取 Anki 学习状态、排除已学习词和更新已有卡片。
- 可导入 Yomitan 格式词典，为卡片提供释义、词频和声调信息。

### 歌曲播放器

- 曲库按歌手折叠，支持搜索歌曲和查看同步歌词。
- 点击歌词跳转播放，支持倍速、升降调和单行循环。
- 支持仅汉字振假名、全振假名以及自定义歌词字体、字号、行距和字间距。
- 在歌词中右键查词，可查看词典释义和全语料出现位置，并直接加入指定 Anki 牌组。

## 下载与安装

普通用户不需要安装 Python、PyQt6、GiNZA 或 FFmpeg。请打开仓库右侧的 **Releases**，下载最新版本：

- `JpopCorpusTool-x.y.z-windows-setup.exe`：推荐，标准 Windows 安装程序。
- `JpopCorpusTool-x.y.z-windows-portable.zip`：便携版，解压后运行 `JpopCorpusTool.exe`。

程序数据默认保存在：

```text
%LOCALAPPDATA%\JpopCorpusTool
```

安装升级不会覆盖这里的歌曲、歌词、设置和数据库。也可以通过环境变量 `JPOP_CORPUS_HOME` 指定其他数据目录。

## 第一次使用

1. 启动软件并进入“导入”。
2. 选择自己合法拥有的本地歌曲文件；同目录下同名的 `.lrc` 歌词会自动匹配。
3. 等待分词和数据库写入完成。
4. 在“搜索”中检索单词，或进入“曲库”播放歌曲和查看歌词。
5. 需要制卡时启动 Anki，并安装 AnkiConnect 插件。

当前支持的具体音频格式取决于 Windows 媒体后端；发布包内置 FFmpeg，用于音频裁剪、变速和变调。

## 配置 Anki

1. 安装并启动 [Anki](https://apps.ankiweb.net/)。
2. 安装 [AnkiConnect（插件代码 2055492159）](https://ankiweb.net/shared/info/2055492159)。
3. 在软件的 Anki 页面刷新连接状态。
4. 选择目标牌组后即可发送或更新卡片。

AnkiConnect 默认只监听本机地址，本工具不会上传你的语料或 Anki 数据。

## 从源码运行

开发环境推荐使用 64 位 Python 3.12 和 Windows 10/11。

```powershell
git clone https://github.com/Yisoragoto/jpop-corpus-tool.git
cd jpop-corpus-tool
.\setup_windows.bat
.\run_gui.bat
```

手动安装方式：

```powershell
python -m venv .venv
.\.venv\Scripts\python.exe -m pip install --upgrade pip
.\.venv\Scripts\python.exe -m pip install -r requirements.txt
.\.venv\Scripts\python.exe gui.py
```

首次启动会自动创建空数据库。仓库中的 `examples/` 提供数据格式示例，但不包含真实歌曲或歌词。

## 本地构建 Windows 发行版

构建环境需要 Python 3.12、Inno Setup 6，以及可选的 FFmpeg：

```powershell
.\packaging\build_windows.ps1 -Version 0.1.0 -FfmpegPath "D:\tools\ffmpeg\bin\ffmpeg.exe"
```

输出：

```text
dist/JpopCorpusTool-0.1.0-windows-portable.zip
dist/JpopCorpusTool-0.1.0-windows-setup.exe
```

推送形如 `v0.1.0` 的 Git 标签后，GitHub Actions 会自动构建并把这两个文件发布到 Releases。

## 版权边界

请勿提交或再分发以下内容：

- 商业歌曲音频或未经授权的歌词、LRC 文件；
- 包含真实歌词的 `corpus.db`、处理后语料或 Anki 牌组；
- 无再分发许可的第三方 Yomitan 词典；
- 本机设置、缓存、备份和用户路径。

本工具用于处理用户自行提供且有权使用的材料。使用者应自行确认所在地区适用的版权和合理使用规则。

## 目录说明

```text
dialogs/       Anki、词典、曲库等界面模块
scripts/       元数据、分词和数据库构建脚本
assets/fonts/  随软件分发的 OFL 字体及许可证
packaging/     PyInstaller 与 Inno Setup 构建脚本
examples/      不含版权内容的数据格式示例
raw/           本地音频和歌词目录，Git 默认忽略内容
```

## 许可证

程序源码使用 [MIT License](LICENSE)。内置字体和可选 FFmpeg 适用各自许可证，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
