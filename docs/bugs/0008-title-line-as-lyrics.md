# BUG-0008 QQ／酷狗逐字歌词的「曲名 - 歌手」行进了语料

- 发现：2026-10-23 / 0.3.3 之后的 codex/validated-lyric-sources / 全平台
- 现象：库副本跑全库升级，补上逐字的 37 首里有 12 首第一句歌词是「ダンスホール - Mrs. GREEN APPLE」这种标题行；另有一批歌因此正文比对不到 97%，被留成「待复核」。
- 复现步骤：对「怪獣の花唄 / Vaundy」取 QQ 候选，第一行是「怪獣の花唄 - Vaundy」。
- 根因：QQ、酷狗的逐字正文开头带一行标题（歌手后面有时还跟中文别名，如「きのこ帝国 (蘑菇帝国)」，或者换成罗马字，如「エンドレス - sakanaction」），解析后原样当歌词。
- 影响面：所有自动采用和手动采用的 QQ／酷狗逐字歌词。
- 修复：☑ 根因修复（`rust/crates/jp-scraper/src/timed_sources.rs` 的 `drop_title_line`：前两行里「曲名 - 任意歌手写法」或与曲名+歌手完全对上的那一行去掉） ☑ 自动化测试（`the_title_artist_header_line_is_not_kept_as_lyrics`）
- 清理条件：已经采用进库的歌，重新采用一次即可
