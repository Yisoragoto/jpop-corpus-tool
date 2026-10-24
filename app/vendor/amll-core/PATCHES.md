# AMLL core（打过补丁的副本）

来源：npm 包 `@applemusic-like-lyrics/core@0.6.0`（[amll-dev/applemusic-like-lyrics](https://github.com/amll-dev/applemusic-like-lyrics) 的 `packages/core`），
AGPL-3.0-only，许可证原文见同目录 `LICENSE`。本项目整体是 GPL-3.0-or-later，按 GPLv3 第 13 条与 AGPLv3 的部分组合发布；
这一部分（含下面的改动）仍按 AGPL-3.0 的条款提供。上游未修改的源码见上面的仓库。

只保留了 ESM 产物、类型声明和样式（`dist/amll-core.mjs`、`dist/amll-core.d.mts`、`dist/style.css`、`dist/rolldown-runtime-*.mjs`）。
`package.json` 是重写过的：只导出 ESM，去掉了运行时用不到的 `vitest`、`deep-freeze` 依赖，pixi 的 peer 版本限定到 7.x。

## 改动

目的：在 AMLL 的歌词里**点词查词**。AMLL 原来只有行级点击事件（`line-click`），不知道点中的是哪个词，
而且会把词合并、拆分，外面没法从 DOM 反推原文位置。

1. `createWord`：传入的 `LyricWord` 带 `charStart`（这个词在原文里的起始字符下标，Unicode 码点）时，
   写到词元素的 `data-char-start` 上。
2. `LyricLineMouseEvent` 多一个 `wordElement` 字段；`DomLyricPlayer` 的鼠标处理把被点中的
   `[data-char-start]` 元素放进去（没点在词上为 `null`）。
3. 拆词（`chunkAndSplitLyricWords`）时普通词会按空白、CJK 单字重建成新对象，额外字段会丢：
   加了一个按 Unicode 码点走的游标，把 `charStart` 带到拆出来的每一段上。
4. 类型声明对应加了 `LyricWord.charStart` 和 `LyricLineMouseEvent.wordElement`。

改动处都带 `JPOP Corpus Tool patch` 注释。调用方（`app/src/components/AmllLyrics.tsx`）用 `wordElement` 和点击位置
算出点中的字符，再换算成分词去查词。

重新生成：`scripts/vendor-amll.py`（从 npm 包解出来的目录 → 这里），升级 AMLL 时改版本号重跑，补丁对不上会直接报错。
