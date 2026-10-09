"""
04_export_processed_from_db.py
Export the current corpus.db utterances/tokens back to processed/utterances.jsonl.

Use this when corpus.db is the trusted source of truth and the intermediate
JSONL has fallen behind. The script backs up the existing JSONL before writing.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import json
import pathlib
import shutil
import sqlite3


# In the repository these scripts live in legacy/scripts/ and the corpus is at the
# repository root, one level above legacy/. In a 0.1.x release package they sit in
# <app>/scripts/ with no legacy/ level.
_SCRIPTS_PARENT = pathlib.Path(__file__).resolve().parents[1]
BASE_DIR = _SCRIPTS_PARENT.parent if _SCRIPTS_PARENT.name == "legacy" else _SCRIPTS_PARENT
DB_PATH = BASE_DIR / "corpus.db"
OUTPUT_PATH = BASE_DIR / "processed" / "utterances.jsonl"
BACKUP_DIR = BASE_DIR / "backups"


def backup_jsonl(path: pathlib.Path = OUTPUT_PATH) -> pathlib.Path | None:
    if not path.exists():
        return None
    BACKUP_DIR.mkdir(parents=True, exist_ok=True)
    ts = _dt.datetime.now().strftime("%Y%m%d_%H%M%S")
    backup_path = BACKUP_DIR / f"{path.stem}_{ts}{path.suffix}"
    shutil.copy2(path, backup_path)
    return backup_path


def export_processed(db_path: pathlib.Path = DB_PATH, output_path: pathlib.Path = OUTPUT_PATH):
    db_path = pathlib.Path(db_path)
    output_path = pathlib.Path(output_path)
    output_path.parent.mkdir(parents=True, exist_ok=True)

    backup_path = backup_jsonl(output_path)
    tmp_path = output_path.with_suffix(output_path.suffix + ".tmp")

    conn = sqlite3.connect(str(db_path))
    try:
        conn.row_factory = sqlite3.Row
        utterances = conn.execute("""
            SELECT id, song_id, line_idx, time_sec, text
            FROM utterances
            ORDER BY id
        """).fetchall()

        utt_count = 0
        tok_count = 0
        with open(tmp_path, "w", encoding="utf-8", newline="\n") as f:
            for utt in utterances:
                token_rows = conn.execute("""
                    SELECT surface, lemma, pos, dep, head
                    FROM tokens
                    WHERE utterance_id=?
                    ORDER BY token_idx
                """, (utt["id"],)).fetchall()
                tokens = [
                    {
                        "text": row["surface"],
                        "lemma": row["lemma"],
                        "pos": row["pos"],
                        "dep": row["dep"],
                        "head": row["head"],
                    }
                    for row in token_rows
                ]
                rec = {
                    "song_id": utt["song_id"],
                    "line_idx": utt["line_idx"],
                    "time_sec": utt["time_sec"],
                    "text": utt["text"],
                    "tokens": tokens,
                }
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
                utt_count += 1
                tok_count += len(tokens)
        tmp_path.replace(output_path)
    finally:
        conn.close()

    return backup_path, utt_count, tok_count


def main():
    parser = argparse.ArgumentParser(
        description="Export processed/utterances.jsonl from current corpus.db."
    )
    parser.add_argument("--db", default=str(DB_PATH), help="Path to corpus.db")
    parser.add_argument("--out", default=str(OUTPUT_PATH), help="Output JSONL path")
    args = parser.parse_args()

    backup_path, utt_count, tok_count = export_processed(
        pathlib.Path(args.db),
        pathlib.Path(args.out),
    )
    if backup_path:
        print(f"已备份旧 JSONL：{backup_path}")
    print(f"已导出：{args.out}")
    print(f"utterances: {utt_count}")
    print(f"tokens: {tok_count}")


if __name__ == "__main__":
    main()
