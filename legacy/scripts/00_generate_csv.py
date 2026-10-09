"""
扫描 raw/audio/ 里所有音频文件，生成 metadata/songs.csv
从文件名 "艺人 - 歌名.flac" 解析元数据
"""
from pathlib import Path
import csv
import re
from mutagen import File as MutagenFile

# ============ 配置 ============
# In the repository these scripts live in legacy/scripts/ and the corpus is at the
# repository root, one level above legacy/. In a 0.1.x release package they sit in
# <app>/scripts/ with no legacy/ level.
_SCRIPTS_PARENT = Path(__file__).resolve().parent.parent
PROJECT_ROOT = _SCRIPTS_PARENT.parent if _SCRIPTS_PARENT.name == "legacy" else _SCRIPTS_PARENT
AUDIO_DIR = PROJECT_ROOT / "raw" / "audio"
CSV_PATH = PROJECT_ROOT / "metadata" / "songs.csv"

AUDIO_EXTENSIONS = {".flac", ".mp3", ".m4a", ".wav"}
# =============================


def parse_from_filename(path):
    """从文件名 '艺人 - 歌名' 解析"""
    stem = path.stem
    match = re.match(r"^(.+?)\s*[-–—]\s*(.+)$", stem)
    if match:
        return {
            "artist": match.group(1).strip(),
            "title": match.group(2).strip(),
        }
    return None


def parse_from_tags(path):
    """从音频文件的标签读取 album、year"""
    try:
        audio = MutagenFile(path)
        if audio is None:
            return {}
        
        def get_tag(keys):
            for key in keys:
                val = audio.get(key)
                if val:
                    return str(val[0]) if isinstance(val, list) else str(val)
            return ""
        
        return {
            "album": get_tag(["album", "TALB", "\xa9alb"]),
            "year": get_tag(["date", "TDRC", "\xa9day"])[:4],
        }
    except Exception as e:
        print(f"  标签读取失败：{e}")
        return {}


def main():
    if not AUDIO_DIR.exists():
        print(f"错误：找不到 {AUDIO_DIR}")
        return
    
    audio_files = []
    for ext in AUDIO_EXTENSIONS:
        audio_files.extend(AUDIO_DIR.glob(f"*{ext}"))
    audio_files = sorted(audio_files)
    
    print(f"扫描到 {len(audio_files)} 个音频文件\n")
    
    rows = []
    for idx, path in enumerate(audio_files, start=1):
        song_id = f"{idx:03d}"
        
        meta = parse_from_filename(path)
        tags = parse_from_tags(path)
        
        if not meta:
            print(f"[{song_id}] ✗ 无法解析文件名：{path.name}")
            meta = {"title": path.stem, "artist": ""}
        else:
            print(f"[{song_id}] ✓ {meta['artist']} / {meta['title']}")
        
        rows.append({
            "id": song_id,
            "title": meta.get("title", ""),
            "artist": meta.get("artist", ""),
            "year": tags.get("year", ""),
            "album": tags.get("album", ""),
            "genre": "J-POP",
            "audio_path": str(path),
        })
    
    CSV_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(CSV_PATH, "w", encoding="utf-8-sig", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=[
            "id", "title", "artist", "year", "album", "genre", "audio_path"
        ])
        writer.writeheader()
        writer.writerows(rows)
    
    print(f"\n{'='*50}")
    print(f"✓ 生成完成：{CSV_PATH}")
    print(f"  共 {len(rows)} 首歌")


if __name__ == "__main__":
    main()