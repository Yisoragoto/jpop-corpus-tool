"""
02_parse_lrc_tokenize.py
解析 LRC 歌词文件，用 GiNZA 分词，输出 processed/utterances.jsonl。

每行输出一个 JSON 对象，字段：
  song_id, line_idx, time_sec, text, tokens
  tokens 每个元素：text, lemma, pos, dep, head
"""

import re
import json
import pathlib
import spacy

# ---------- 路径配置 ----------
# In the repository these scripts live in legacy/scripts/ and the corpus is at the
# repository root, one level above legacy/. In a 0.1.x release package they sit in
# <app>/scripts/ with no legacy/ level.
_SCRIPTS_PARENT = pathlib.Path(__file__).resolve().parents[1]
BASE_DIR    = _SCRIPTS_PARENT.parent if _SCRIPTS_PARENT.name == "legacy" else _SCRIPTS_PARENT
LRC_DIR     = BASE_DIR / "raw" / "lyrics_lrc"
OUTPUT_FILE = BASE_DIR / "processed" / "utterances.jsonl"

# LRC 时间戳正则：[分:秒.毫秒]
LRC_LINE_RE = re.compile(r"^\[(\d+):(\d+\.\d+)\](.*)")

# 跳过明显的元数据关键词（中文标注的作词/作曲等）
SKIP_KEYWORDS = ["作词", "作曲", "编曲", "作詞", "作曲", "編曲"]

def parse_lrc(lrc_path: pathlib.Path) -> list[dict]:
    """解析 LRC 文件，返回 [{time_sec, text}, ...] 列表，只保留有实际歌词的行。"""
    results = []
    text = lrc_path.read_text(encoding="utf-8")
    for raw_line in text.splitlines():
        m = LRC_LINE_RE.match(raw_line.strip())
        if not m:
            continue
        minutes, seconds_str, lyric = m.group(1), m.group(2), m.group(3).strip()

        # 跳过空行
        if not lyric:
            continue
        # 跳过元数据行
        if any(kw in lyric for kw in SKIP_KEYWORDS):
            continue

        time_sec = int(minutes) * 60 + float(seconds_str)
        results.append({"time_sec": round(time_sec, 3), "text": lyric})
    return results

def tokenize(nlp, text: str) -> list[dict]:
    """用 GiNZA 分词，返回 token 列表。"""
    doc = nlp(text)
    return [
        {
            "text":  token.text,
            "lemma": token.lemma_,
            "pos":   token.pos_,
            "dep":   token.dep_,
            "head":  token.head.text,
        }
        for token in doc
    ]

def main():
    OUTPUT_FILE.parent.mkdir(parents=True, exist_ok=True)

    print("加载 GiNZA 模型……")
    nlp = spacy.load("ja_ginza")

    lrc_files = sorted(LRC_DIR.glob("[0-9]*.lrc"))
    print(f"找到 {len(lrc_files)} 个 LRC 文件，开始处理……\n")

    total_utterances = 0

    with open(OUTPUT_FILE, "w", encoding="utf-8") as out_f:
        for lrc_path in lrc_files:
            song_id = lrc_path.stem  # 文件名去掉 .lrc，即 "001" 等
            lines = parse_lrc(lrc_path)

            for line_idx, line in enumerate(lines):
                tokens = tokenize(nlp, line["text"])
                record = {
                    "song_id":  song_id,
                    "line_idx": line_idx,
                    "time_sec": line["time_sec"],
                    "text":     line["text"],
                    "tokens":   tokens,
                }
                out_f.write(json.dumps(record, ensure_ascii=False) + "\n")
                total_utterances += 1

            print(f"[{song_id}] {len(lines)} 行")

    print(f"\n完成：共 {total_utterances} 条 utterance，输出到 {OUTPUT_FILE}")

if __name__ == "__main__":
    main()
