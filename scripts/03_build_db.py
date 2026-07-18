"""
03_build_db.py
重建 SQLite 数据库，并建 FTS5 全文索引。

默认行为（--source auto）：
  - 如果 corpus.db 已存在，先创建 SQLite 一致备份，再从备份恢复核心语料表
    songs / chapters / utterances / tokens。这样不会依赖可能过期的
    metadata/songs.csv 或 processed/utterances.jsonl。
  - 如果 corpus.db 不存在，才从 songs.csv + utterances.jsonl 初次构建。

也可显式指定 --source files 从中间产物重建。
重建后会尽量恢复词典/JLPT/分词校正等用户数据。

表结构：
  songs       — 歌曲元数据
  utterances  — 每行歌词（含时间戳）
  tokens      — 每个 token 的分词结果
  utterances_fts — FTS5 虚拟表，对歌词原文做全文检索
"""

import csv
import json
import sqlite3
import pathlib
import argparse

import migrate_db

# ---------- 路径配置 ----------
BASE_DIR    = pathlib.Path(__file__).resolve().parents[1]
CSV_PATH    = BASE_DIR / "metadata" / "songs.csv"
JSONL_PATH  = BASE_DIR / "processed" / "utterances.jsonl"
DB_PATH     = BASE_DIR / "corpus.db"

CORE_TABLES = [
    "songs",
    "chapters",
    "utterances",
    "tokens",
]

USER_TABLES = [
    "jlpt_cache",
    "yomitan_zh",
    "yomitan_pitch",
    "yomitan_freq",
    "yomitan_meta",
    "dict_registry",
    "dict_terms",
    "token_corrections",
]


def _delete_db_files(db_path: pathlib.Path = DB_PATH):
    for suffix in ("", "-wal", "-shm"):
        path = pathlib.Path(str(db_path) + suffix)
        if path.exists():
            path.unlink()


def _table_exists(conn: sqlite3.Connection, schema: str, table: str) -> bool:
    return conn.execute(
        f"SELECT 1 FROM {schema}.sqlite_master WHERE type='table' AND name=?",
        (table,),
    ).fetchone() is not None


def _table_cols(conn: sqlite3.Connection, schema: str, table: str) -> list[str]:
    return [r[1] for r in conn.execute(f"PRAGMA {schema}.table_info({table})")]


def _restore_rows_from_backup(
    conn: sqlite3.Connection,
    table: str,
    *,
    mode: str,
    exclude_cols: set[str] | None = None,
) -> int:
    if not _table_exists(conn, "old", table) or not _table_exists(conn, "main", table):
        return 0
    old_cols = _table_cols(conn, "old", table)
    new_cols = _table_cols(conn, "main", table)
    exclude_cols = exclude_cols or set()
    common = [c for c in new_cols if c in old_cols and c not in exclude_cols]
    if not common:
        return 0
    col_sql = ", ".join(common)
    if mode == "replace":
        sql = f"INSERT OR REPLACE INTO {table}({col_sql}) SELECT {col_sql} FROM old.{table}"
    elif mode == "ignore":
        sql = f"INSERT OR IGNORE INTO {table}({col_sql}) SELECT {col_sql} FROM old.{table}"
    else:
        sql = f"INSERT INTO {table}({col_sql}) SELECT {col_sql} FROM old.{table}"
    before = conn.total_changes
    conn.execute(sql)
    return conn.total_changes - before


def _restore_core_tables(conn: sqlite3.Connection, backup_path: pathlib.Path | None):
    if not backup_path or not backup_path.exists():
        return []
    conn.execute("ATTACH DATABASE ? AS old", (str(backup_path),))
    try:
        restored = []
        for table in CORE_TABLES:
            n = _restore_rows_from_backup(conn, table, mode="replace")
            if n:
                restored.append(f"{table}({n})")
        conn.commit()
        return restored
    finally:
        conn.execute("DETACH DATABASE old")


def _restore_user_tables(conn: sqlite3.Connection, backup_path: pathlib.Path | None):
    if not backup_path or not backup_path.exists():
        return []
    conn.execute("ATTACH DATABASE ? AS old", (str(backup_path),))
    try:
        restored = []
        for table in USER_TABLES:
            if table == "dict_terms" and _table_exists(conn, "old", table):
                n = _restore_rows_from_backup(conn, table, mode="insert", exclude_cols={"id"})
            elif table == "token_corrections" and _table_exists(conn, "old", table):
                old_cols = _table_cols(conn, "old", table)
                new_cols = _table_cols(conn, "main", table)
                common = [c for c in new_cols if c in old_cols]
                if not common:
                    continue
                col_sql = ", ".join(common)
                before = conn.total_changes
                conn.execute(f"""
                    INSERT OR IGNORE INTO token_corrections({col_sql})
                    SELECT {col_sql} FROM old.token_corrections
                    WHERE utterance_id IN (SELECT id FROM utterances)
                """)
                n = conn.total_changes - before
            else:
                n = _restore_rows_from_backup(conn, table, mode="replace")
            if n:
                restored.append(table)
        conn.commit()
        if restored:
            print("已恢复用户数据表：" + "、".join(restored))
        return restored
    finally:
        conn.execute("DETACH DATABASE old")


def _import_core_from_files(
    conn: sqlite3.Connection,
    csv_path: pathlib.Path = CSV_PATH,
    jsonl_path: pathlib.Path = JSONL_PATH,
):
    cur = conn.cursor()
    with open(csv_path, encoding="utf-8-sig", newline="") as f:
        reader = csv.DictReader(f)
        songs = [(r["id"], r["title"], r["artist"], r["year"],
                  r["album"], r["genre"], r["audio_path"]) for r in reader]

    cur.executemany(
        """
        INSERT INTO songs(id, title, artist, year, album, genre, audio_path, corpus_type)
        VALUES (?,?,?,?,?,?,?,'song')
        """,
        songs
    )
    print(f"导入 songs：{len(songs)} 条")

    utt_count = 0
    tok_count = 0

    with open(jsonl_path, encoding="utf-8") as f:
        for line in f:
            rec = json.loads(line)

            cur.execute(
                "INSERT INTO utterances (song_id, line_idx, time_sec, text) VALUES (?,?,?,?)",
                (rec["song_id"], rec["line_idx"], rec["time_sec"], rec["text"])
            )
            utt_id = cur.lastrowid

            token_rows = [
                (utt_id, i, t["text"], t["lemma"], t["pos"], t["dep"], t["head"])
                for i, t in enumerate(rec["tokens"])
            ]
            cur.executemany(
                "INSERT INTO tokens (utterance_id, token_idx, surface, lemma, pos, dep, head) VALUES (?,?,?,?,?,?,?)",
                token_rows
            )

            utt_count += 1
            tok_count += len(token_rows)

    print(f"导入 utterances：{utt_count} 条")
    print(f"导入 tokens：{tok_count} 条")


def build_db(
    db_path: pathlib.Path = DB_PATH,
    source: str = "auto",
    csv_path: pathlib.Path = CSV_PATH,
    jsonl_path: pathlib.Path = JSONL_PATH,
):
    db_path = pathlib.Path(db_path)
    backup_path = None
    old_db_exists = db_path.exists()
    if old_db_exists:
        backup_path = migrate_db.backup_database(db_path)
        print(f"已备份旧数据库：{backup_path}")
        _delete_db_files(db_path)
        print(f"已删除旧数据库：{db_path}")

    if source == "auto":
        source = "db" if backup_path else "files"
    if source == "db" and not backup_path:
        raise SystemExit("错误：--source db 需要已有数据库可供备份恢复。")

    conn = sqlite3.connect(db_path)
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA foreign_keys=ON")

    # ---------- 建当前最新版 schema ----------
    migrate_db.migrate_connection(conn, rebuild_fts_index=False)

    if source == "db":
        restored = _restore_core_tables(conn, backup_path)
        print("已从旧数据库快照恢复核心语料表：" + "、".join(restored))
    elif source == "files":
        print(f"从中间产物导入：{csv_path} / {jsonl_path}")
        _import_core_from_files(conn, csv_path, jsonl_path)
    else:
        raise SystemExit(f"错误：未知 source={source!r}")

    # ---------- 填充 FTS5 索引 ----------
    migrate_db.rebuild_fts(conn)
    print("FTS5 索引建立完成")

    # ---------- 恢复用户数据 ----------
    _restore_user_tables(conn, backup_path)

    conn.commit()
    conn.close()

    size_kb = db_path.stat().st_size // 1024
    print(f"\n数据库已保存到 {db_path}（{size_kb} KB）")

def smoke_test(db_path: pathlib.Path = DB_PATH):
    """简单验证：搜索「涙」，打印前 3 条结果。"""
    conn = sqlite3.connect(db_path)
    cur = conn.cursor()

    print("\n--- 验证：FTS5 搜索「涙」---")
    cur.execute("""
        SELECT s.artist, s.title, u.time_sec, u.text
        FROM utterances_fts f
        JOIN utterances u ON u.id = f.rowid
        JOIN songs s ON s.id = u.song_id
        WHERE utterances_fts MATCH '涙'
        LIMIT 3
    """)
    for row in cur.fetchall():
        print(f"  [{row[0]}《{row[1]}》{row[2]}s] {row[3]}")

    print("\n--- 验证：按词元（lemma）搜索「泣く」---")
    cur.execute("""
        SELECT s.artist, s.title, u.time_sec, u.text
        FROM tokens t
        JOIN utterances u ON u.id = t.utterance_id
        JOIN songs s ON s.id = u.song_id
        WHERE t.lemma = '泣く'
        LIMIT 3
    """)
    for row in cur.fetchall():
        print(f"  [{row[0]}《{row[1]}》{row[2]}s] {row[3]}")

    conn.close()

def main():
    parser = argparse.ArgumentParser(
        description="Rebuild corpus.db from current DB snapshot or files."
    )
    parser.add_argument("--db", default=str(DB_PATH), help="Output corpus.db path.")
    parser.add_argument(
        "--source",
        choices=["auto", "db", "files"],
        default="auto",
        help=(
            "auto: existing DB snapshot if available, otherwise files; "
            "db: restore core corpus from existing DB backup; "
            "files: import metadata/songs.csv and processed/utterances.jsonl."
        ),
    )
    parser.add_argument("--csv", default=str(CSV_PATH), help="songs.csv path for --source files.")
    parser.add_argument("--jsonl", default=str(JSONL_PATH), help="utterances.jsonl path for --source files.")
    parser.add_argument(
        "--no-smoke",
        action="store_true",
        help="Skip smoke-test searches after rebuilding.",
    )
    args = parser.parse_args()
    db_path = pathlib.Path(args.db)
    build_db(
        db_path=db_path,
        source=args.source,
        csv_path=pathlib.Path(args.csv),
        jsonl_path=pathlib.Path(args.jsonl),
    )
    if not args.no_smoke:
        smoke_test(db_path)


if __name__ == "__main__":
    main()
