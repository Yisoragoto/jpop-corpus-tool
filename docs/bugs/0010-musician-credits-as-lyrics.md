# BUG-0010 乐手署名段被当成歌词

- 发现：2026-10-23 / 0.3.3 之后的 codex/validated-lyric-sources / 全平台
- 现象：「lulu.」的 QQ 候选开头有十几行「E. Guitar：若井滉斗」「1st Violin：…」，正文比对因此不过，留成「待复核」；若手动采用，这些行会进语料。
- 复现步骤：对「lulu. / Mrs. GREEN APPLE」取 QQ 候选。
- 根因：导入解析的署名表只认常见标签，乐手署名的写法是开放的。
- 影响面：带完整乐手署名的 QQ／酷狗歌词。
- 修复：☑ 根因修复（`rust/crates/jp-scraper/src/timed_sources.rs` 的 `drop_credit_block`：只在开头或结尾、连成一段的「标签：人名」行，标签全是拉丁字母或是常见中日文署名标签；中间的唱词不动，全是署名时不删） ☑ 自动化测试（`a_musician_credit_block_at_either_end_is_dropped_but_sung_lines_stay`）
- 清理条件：无
