"""
01_fetch_lrc.py
批量用 syncedlyrics 下载 LRC 歌词文件。

读取 metadata/songs.csv，对每首歌搜索 LRC，
成功的保存到 raw/lyrics_lrc/{id}.lrc，
找不到的记录到 raw/lyrics_lrc/_missing.txt。
"""

import csv
import time
import pathlib
import syncedlyrics

# ---------- 路径配置 ----------
# In the repository these scripts live in legacy/scripts/ and the corpus is at the
# repository root, one level above legacy/. In a 0.1.x release package they sit in
# <app>/scripts/ with no legacy/ level.
_SCRIPTS_PARENT = pathlib.Path(__file__).resolve().parents[1]
BASE_DIR = _SCRIPTS_PARENT.parent if _SCRIPTS_PARENT.name == "legacy" else _SCRIPTS_PARENT
CSV_PATH = BASE_DIR / "metadata" / "songs.csv"
LRC_DIR  = BASE_DIR / "raw" / "lyrics_lrc"
MISSING_FILE = LRC_DIR / "_missing.txt"

# 每次请求之间等待秒数（避免频繁请求被封）
SLEEP_SECONDS = 1.5

def main():
    LRC_DIR.mkdir(parents=True, exist_ok=True)

    missing_ids = []

    with open(CSV_PATH, encoding="utf-8-sig", newline="") as f:
        reader = csv.DictReader(f)
        songs = list(reader)

    total = len(songs)
    print(f"共 {total} 首歌，开始下载 LRC……\n")

    for song in songs:
        song_id = song["id"]
        title   = song["title"]
        artist  = song["artist"]
        lrc_path = LRC_DIR / f"{song_id}.lrc"

        # 已下载过则跳过
        if lrc_path.exists():
            print(f"[{song_id}] 已存在，跳过：{title}")
            continue

        search_term = f"{artist} {title}"
        print(f"[{song_id}] 搜索：{search_term}")

        try:
            lrc_text = syncedlyrics.search(search_term)
        except Exception as e:
            print(f"  !! 异常：{e}")
            lrc_text = None

        if lrc_text:
            lrc_path.write_text(lrc_text, encoding="utf-8")
            # 粗略统计行数作为质量参考
            line_count = lrc_text.count("\n")
            print(f"  OK  保存到 {lrc_path.name}（约 {line_count} 行）")
        else:
            print(f"  --  未找到")
            missing_ids.append(f"{song_id}\t{artist}\t{title}")

        time.sleep(SLEEP_SECONDS)

    # 写入缺失清单
    if missing_ids:
        MISSING_FILE.write_text(
            "id\tartist\ttitle\n" + "\n".join(missing_ids) + "\n",
            encoding="utf-8"
        )
        print(f"\n未找到 {len(missing_ids)} 首，已记录到 {MISSING_FILE}")
    else:
        print("\n所有歌曲均找到 LRC！")

    found = total - len(missing_ids)
    print(f"\n完成：{found}/{total} 首成功下载。")

if __name__ == "__main__":
    main()
