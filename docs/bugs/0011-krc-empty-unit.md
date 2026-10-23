# BUG-0011 KRC／YRC 里一个空单位让整首解析失败

- 发现：2026-10-23 / 0.3.3 之后的 codex/validated-lyric-sources / 全平台
- 现象：「パラノイドパレード」的酷狗候选报「invalid lyric line 19: word timestamp without text」，整首拿不到。
- 复现步骤：KRC 一行里两个 `<start,duration,0>` 挨着，或者行尾多一个。
- 根因：`prefix_line` 要求每个时间标记后面必须有字，否则整首报错；来源实际会给没有字的单位。
- 影响面：带空单位的酷狗 KRC、网易 YRC。
- 修复：☑ 根因修复（`rust/crates/jp-scraper/src/timed_lyrics.rs`：没字的单位直接丢掉，不产生词；整行没字仍然报错） ☑ 自动化测试（`an_empty_krc_or_yrc_unit_is_dropped_instead_of_failing_the_song`）
- 清理条件：无
