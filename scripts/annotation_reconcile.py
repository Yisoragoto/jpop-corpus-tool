"""形态信息（tokens 的 pos_ja / ctype / cform / reading / normalized）的独立对账。

拿 SudachiPy（Python 原版，不是本程序用的 sudachi.rs）在同一份词典、同一份 sudachi.json 上
重新分析每一行，用 SudachiPy 自己给的字符位置（`begin()` / `end()`）对齐库里的 token，
逐个比五列。和 Rust 那边（jp-import 的 annotate.rs，用字符串查找定位）是两套独立的对齐办法。

    <装了 sudachipy 的 python> -I scripts/annotation_reconcile.py <补过形态信息的 corpus.db> <sudachi 目录>

库里的 token 在原文里找不到对应的 SudachiPy 词（GiNZA 时代的空白 token、切分不同的）时，
期望值是「标不上」（pos_ja 为 ''）或者「单独分析这个词得到的结果」，脚本按同样的规则算期望值再比。
"""

import os
import sqlite3
import sys
from collections import Counter

from sudachipy import Dictionary, SplitMode

DB, DIR = sys.argv[1], sys.argv[2]
tokenizer = Dictionary(config_path=os.path.join(DIR, "sudachi.json"), resource_dir=DIR, dict=os.path.join(DIR, "system.dic")).create(SplitMode.C)


def morph(m):
    pos = m.part_of_speech()
    star = lambda p: "" if p == "*" else p  # noqa: E731
    return ("-".join(p for p in pos[:4] if p != "*"), star(pos[4]), star(pos[5]), m.reading_form(), m.normalized_form())


con = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
lines = {}
for uid, text in con.execute("SELECT id, text FROM utterances"):
    lines[uid] = text
rows = con.execute(
    "SELECT utterance_id, id, surface, pos_ja, ctype, cform, reading, normalized FROM tokens ORDER BY utterance_id, token_idx, id"
).fetchall()

by_line = {}
for r in rows:
    by_line.setdefault(r[0], []).append(r[1:])

kinds = Counter()
problems = []
for uid, toks in by_line.items():
    text = lines[uid]
    # SudachiPy 给的是原文里的字符位置
    spans = {}
    for m in tokenizer.tokenize(text):
        if m.surface().strip():
            spans[(m.begin(), m.end())] = m
    cursor = 0
    for tid, surface, *stored in toks:
        stored = tuple(stored)
        found = text.find(surface, cursor) if surface else -1
        span = None
        if found >= 0:
            span = (found, found + len(surface))
            cursor = span[1]
        m = spans.get(span) if span else None
        if m is not None and m.surface() == surface:
            want, kind = morph(m), "aligned"
        else:
            alone = [x for x in tokenizer.tokenize(surface) if x.surface().strip()] if surface else []
            if len(alone) == 1 and alone[0].surface() == surface:
                want, kind = morph(alone[0]), "fallback"
            else:
                want, kind = ("", None, None, None, None), "unknown"
        kinds[kind] += 1
        if stored != want:
            problems.append(f"{uid}/{tid} {surface!r}: rust={stored} python={want}")

total = sum(kinds.values())
print(f"{total:,} 个 token：按位置对齐 {kinds['aligned']:,} · 单独分析 {kinds['fallback']:,} · 标不上 {kinds['unknown']:,}")
print(f"不一致 {len(problems)} 个")
for p in problems[:40]:
    print("  " + p)
sys.exit(1 if problems else 0)
