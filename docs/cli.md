# CLI：同一份库的批处理入口

在 `rust/` 下运行 `cargo run -p jp-cli -- <命令>`，构建后的二进制名为 `jp`。
参数错误退出 1，运行失败退出 2，成功退出 0；进度到 stderr，JSON 数据到 stdout。

| 命令 | 行为 |
|---|---|
| `jp info` | 打开/新建库，显示路径、结构版本、歌/行/词数 |
| `jp fingerprint` | 显式重扫全库音频指纹并写库，输出 JSON 报告；有失败时退出 2 |
| `jp scan <path> --json` | 只读扫描文件或目录，输出计划，不建库、不升级库 |
| `jp import <path> --tokenize --json` | 执行导入；分词需已安装 Sudachi，不加标志则只导入歌词 |
| `jp tokenize --missing` | 在事务里补分词；不加标志则重分全库，校正按现有核心规则恢复 |
| `jp scrape <path...>` | 联网批量识别，输出 JSON Lines；保存原始 metadata 与刮削状态，已导入的歌只补空字段 |
| `jp export-anki --out cards.tsv` | 用 jp-anki 组卡，输出可由 Anki 导入的 HTML TSV；不连接 AnkiConnect、不打包音频 |

库路径解析只有一份：`jp_corpus::library_root::locate()`。jp-app 原模块只做 re-export。
顺序保持应用现状：有效的 `JPOP_CORPUS_HOME` → settings.json 的目录 → 可执行文件祖先里的库 →
当前工作目录的库 → LOCALAPPDATA 默认目录。环境变量指向没有建表的空目录时仍按原来的应用规则忽略。
首次在特定空目录建库，可以先在应用设置里选择该目录；CLI 会读取同一份设置。

CLI 没有 Tauri 依赖，参数解析使用标准库；数据库查询及补元数据 SQL 放在 jp-corpus。
`scrape` 沿用 provider 自身限流，不额外并发轰炸服务；需人工复核的候选只记状态，不自动采纳。
TSV 的笔记类型是现有 `JPOP Corpus`，需在 Anki 中已有该类型或自行对应字段。
