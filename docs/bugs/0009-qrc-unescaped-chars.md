# BUG-0009 歌词里有 & 或引号的 QRC 整首解析失败

- 发现：2026-10-23 / 0.3.3 之后的 codex/validated-lyric-sources / 全平台
- 现象：「lulu.」的 QQ 候选报「invalid lyric XML: malformed entity reference」，「東京フラッシュ」报「expected a whitespace not 'S'」，两首都拿不到 QQ 逐字。
- 复现步骤：QRC 的 `LyricContent="…"` 里含裸 `&`，或含 `"Stay"` 这样的引号。
- 根因：QQ 把歌词正文塞进 XML 属性时不做转义，严格的 XML 解析在第一个特殊字符处失败。
- 影响面：正文含 `&`、`"`、`<` 的 QQ 歌词。
- 修复：☑ 根因修复（`rust/crates/jp-scraper/src/timed_lyrics.rs`：解析前把 `LyricContent` 属性值里的 `"`、`<` 和不构成实体的 `&` 转义，已有实体只解码一次） ☑ 自动化测试（`qrc_with_a_bare_ampersand_in_the_lyrics_still_parses`、`qrc_with_a_bare_quote_or_angle_bracket_in_the_lyrics_still_parses`）
- 清理条件：无
