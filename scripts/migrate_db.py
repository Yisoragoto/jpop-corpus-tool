"""
migrate_db.py
SQLite schema migration and backup utility for the Japanese corpus project.

This script is intentionally independent from gui.py so database structure is
not owned by the UI layer. It can create a fresh current schema or migrate an
existing corpus.db in place.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import pathlib
import sqlite3


BASE_DIR = pathlib.Path(__file__).resolve().parents[1]
DB_PATH = BASE_DIR / "corpus.db"
BACKUP_DIR = BASE_DIR / "backups"
SCHEMA_USER_VERSION = 20260507


def _table_exists(conn: sqlite3.Connection, name: str, schema: str = "main") -> bool:
    return conn.execute(
        f"SELECT 1 FROM {schema}.sqlite_master WHERE type='table' AND name=?",
        (name,),
    ).fetchone() is not None


def _index_exists(conn: sqlite3.Connection, name: str) -> bool:
    return conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type='index' AND name=?",
        (name,),
    ).fetchone() is not None


def _columns(conn: sqlite3.Connection, table: str) -> dict[str, sqlite3.Row]:
    return {row[1]: row for row in conn.execute(f"PRAGMA table_info({table})")}


def _add_column_if_missing(conn: sqlite3.Connection, table: str, column: str, ddl: str):
    if column not in _columns(conn, table):
        conn.execute(f"ALTER TABLE {table} ADD COLUMN {ddl}")


def backup_database(db_path: pathlib.Path = DB_PATH) -> pathlib.Path | None:
    """Create a consistent SQLite backup under backups/ and return its path."""
    db_path = pathlib.Path(db_path)
    if not db_path.exists():
        return None
    BACKUP_DIR.mkdir(parents=True, exist_ok=True)
    ts = _dt.datetime.now().strftime("%Y%m%d_%H%M%S")
    backup_path = BACKUP_DIR / f"{db_path.stem}_{ts}.db"

    src = sqlite3.connect(str(db_path))
    try:
        # Make WAL contents visible to the backup API.
        src.execute("PRAGMA wal_checkpoint(FULL)")
        dst = sqlite3.connect(str(backup_path))
        try:
            src.backup(dst)
        finally:
            dst.close()
    finally:
        src.close()
    return backup_path


def rebuild_fts(conn: sqlite3.Connection):
    """Recreate utterances_fts from utterances.text."""
    conn.execute("DROP TABLE IF EXISTS utterances_fts")
    conn.execute("""
        CREATE VIRTUAL TABLE utterances_fts USING fts5(
            text,
            content=utterances,
            content_rowid=id,
            tokenize='trigram'
        )
    """)
    if _table_exists(conn, "utterances"):
        conn.execute(
            "INSERT INTO utterances_fts(rowid, text) SELECT id, text FROM utterances"
        )


def _rebuild_utterances_if_needed(conn: sqlite3.Connection):
    """Make utterances.time_sec nullable and add chapter_id safely."""
    if not _table_exists(conn, "utterances"):
        conn.execute("""
            CREATE TABLE utterances (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                song_id    TEXT NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
                line_idx   INTEGER NOT NULL,
                time_sec   REAL,
                text       TEXT NOT NULL,
                chapter_id INTEGER REFERENCES chapters(id)
            )
        """)
        return

    cols = _columns(conn, "utterances")
    needs_rebuild = (
        "chapter_id" not in cols or
        bool(cols.get("time_sec") and cols["time_sec"][3])
    )
    if not needs_rebuild:
        return

    conn.commit()
    conn.execute("DROP TABLE IF EXISTS utterances_fts")
    conn.execute("PRAGMA foreign_keys=OFF")
    old_cols = _columns(conn, "utterances")
    chapter_expr = "chapter_id" if "chapter_id" in old_cols else "NULL"
    conn.execute("""
        CREATE TABLE utterances_new (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            song_id    TEXT NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
            line_idx   INTEGER NOT NULL,
            time_sec   REAL,
            text       TEXT NOT NULL,
            chapter_id INTEGER REFERENCES chapters(id)
        )
    """)
    conn.execute(f"""
        INSERT INTO utterances_new(id, song_id, line_idx, time_sec, text, chapter_id)
        SELECT id, song_id, line_idx, time_sec, text, {chapter_expr}
        FROM utterances
        ORDER BY id
    """)
    conn.execute("DROP TABLE utterances")
    conn.execute("ALTER TABLE utterances_new RENAME TO utterances")
    conn.execute("PRAGMA foreign_keys=ON")


def _ensure_core_tables(conn: sqlite3.Connection):
    conn.execute("""
        CREATE TABLE IF NOT EXISTS songs (
            id          TEXT PRIMARY KEY,
            title       TEXT NOT NULL,
            artist      TEXT NOT NULL,
            year        TEXT,
            album       TEXT,
            genre       TEXT,
            audio_path  TEXT,
            corpus_type TEXT NOT NULL DEFAULT 'song',
            source_file TEXT
        )
    """)
    _add_column_if_missing(
        conn, "songs", "corpus_type",
        "corpus_type TEXT NOT NULL DEFAULT 'song'",
    )
    _add_column_if_missing(conn, "songs", "source_file", "source_file TEXT")

    conn.execute("""
        CREATE TABLE IF NOT EXISTS chapters (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id   TEXT NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
            chapter_idx INTEGER NOT NULL,
            title       TEXT DEFAULT '',
            utt_start   INTEGER,
            utt_end     INTEGER
        )
    """)
    _rebuild_utterances_if_needed(conn)

    conn.execute("""
        CREATE TABLE IF NOT EXISTS tokens (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            utterance_id INTEGER NOT NULL REFERENCES utterances(id) ON DELETE CASCADE,
            token_idx    INTEGER NOT NULL,
            surface      TEXT NOT NULL,
            lemma        TEXT NOT NULL,
            pos          TEXT,
            dep          TEXT,
            head         TEXT
        )
    """)


def _ensure_dictionary_tables(conn: sqlite3.Connection):
    conn.execute("""
        CREATE TABLE IF NOT EXISTS jlpt_cache (
            lemma TEXT PRIMARY KEY,
            level TEXT NOT NULL DEFAULT ''
        )
    """)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS yomitan_zh (
            term    TEXT PRIMARY KEY,
            reading TEXT NOT NULL DEFAULT '',
            zh_defs TEXT NOT NULL DEFAULT '[]'
        )
    """)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS yomitan_pitch (
            term      TEXT NOT NULL,
            reading   TEXT NOT NULL DEFAULT '',
            positions TEXT NOT NULL DEFAULT '[]',
            PRIMARY KEY (term, reading)
        )
    """)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS yomitan_freq (
            term TEXT PRIMARY KEY,
            freq INTEGER NOT NULL DEFAULT 0
        )
    """)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS yomitan_meta (
            key TEXT PRIMARY KEY,
            val TEXT
        )
    """)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS dict_registry (
            name        TEXT PRIMARY KEY,
            zip_path    TEXT NOT NULL DEFAULT '',
            dict_type   TEXT NOT NULL DEFAULT 'terms',
            entry_count INTEGER NOT NULL DEFAULT 0,
            enabled     INTEGER NOT NULL DEFAULT 1,
            sort_order  INTEGER NOT NULL DEFAULT 999,
            revision    TEXT NOT NULL DEFAULT '',
            imported_at TEXT NOT NULL DEFAULT ''
        )
    """)
    for col, ddl in [
        ("sort_order", "sort_order INTEGER NOT NULL DEFAULT 999"),
        ("revision", "revision TEXT NOT NULL DEFAULT ''"),
        ("imported_at", "imported_at TEXT NOT NULL DEFAULT ''"),
    ]:
        _add_column_if_missing(conn, "dict_registry", col, ddl)

    conn.execute("""
        CREATE TABLE IF NOT EXISTS dict_terms (
            id        INTEGER PRIMARY KEY AUTOINCREMENT,
            term      TEXT NOT NULL,
            reading   TEXT NOT NULL DEFAULT '',
            dict_name TEXT NOT NULL,
            defs_json TEXT NOT NULL DEFAULT '[]'
        )
    """)
    if "id" not in _columns(conn, "dict_terms"):
        conn.execute("ALTER TABLE dict_terms RENAME TO dict_terms_old")
        conn.execute("""
            CREATE TABLE dict_terms (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                term      TEXT NOT NULL,
                reading   TEXT NOT NULL DEFAULT '',
                dict_name TEXT NOT NULL,
                defs_json TEXT NOT NULL DEFAULT '[]'
            )
        """)
        conn.execute("""
            INSERT INTO dict_terms(term, reading, dict_name, defs_json)
            SELECT term, reading, dict_name, defs_json FROM dict_terms_old
        """)
        conn.execute("DROP TABLE dict_terms_old")


def _ensure_token_corrections(conn: sqlite3.Connection):
    conn.execute("""
        CREATE TABLE IF NOT EXISTS token_corrections (
            utterance_id INTEGER PRIMARY KEY,
            tokens_json  TEXT NOT NULL,
            orig_json    TEXT NOT NULL DEFAULT '[]',
            text         TEXT NOT NULL DEFAULT '',
            song_artist  TEXT NOT NULL DEFAULT '',
            song_title   TEXT NOT NULL DEFAULT '',
            updated_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
    """)
    for col, ddl in [
        ("orig_json", "orig_json TEXT NOT NULL DEFAULT '[]'"),
        ("text", "text TEXT NOT NULL DEFAULT ''"),
        ("song_artist", "song_artist TEXT NOT NULL DEFAULT ''"),
        ("song_title", "song_title TEXT NOT NULL DEFAULT ''"),
        ("updated_at", "updated_at TEXT NOT NULL DEFAULT ''"),
    ]:
        _add_column_if_missing(conn, "token_corrections", col, ddl)


def _ensure_indexes(conn: sqlite3.Connection):
    indexes = [
        ("idx_tokens_lemma", "CREATE INDEX IF NOT EXISTS idx_tokens_lemma ON tokens(lemma)"),
        ("idx_tokens_surface", "CREATE INDEX IF NOT EXISTS idx_tokens_surface ON tokens(surface)"),
        ("idx_tokens_pos", "CREATE INDEX IF NOT EXISTS idx_tokens_pos ON tokens(pos)"),
        ("idx_utt_song_id", "CREATE INDEX IF NOT EXISTS idx_utt_song_id ON utterances(song_id)"),
        ("idx_songs_corpus_type", "CREATE INDEX IF NOT EXISTS idx_songs_corpus_type ON songs(corpus_type)"),
        ("idx_chapters_source", "CREATE INDEX IF NOT EXISTS idx_chapters_source ON chapters(source_id)"),
        ("idx_dt_term_dict", "CREATE INDEX IF NOT EXISTS idx_dt_term_dict ON dict_terms(term, dict_name)"),
        ("idx_dt_reading", "CREATE INDEX IF NOT EXISTS idx_dt_reading ON dict_terms(reading)"),
        ("idx_yomitan_zh_reading", "CREATE INDEX IF NOT EXISTS idx_yomitan_zh_reading ON yomitan_zh(reading)"),
        (
            "idx_token_corr_lookup",
            "CREATE INDEX IF NOT EXISTS idx_token_corr_lookup "
            "ON token_corrections(song_artist, song_title, text)",
        ),
    ]
    for name, ddl in indexes:
        if not _index_exists(conn, name):
            conn.execute(ddl)


def migrate_connection(conn: sqlite3.Connection, rebuild_fts_index: bool = True):
    conn.execute("PRAGMA foreign_keys=ON")
    conn.execute("PRAGMA journal_mode=WAL")
    _ensure_core_tables(conn)
    _ensure_dictionary_tables(conn)
    _ensure_token_corrections(conn)
    _ensure_indexes(conn)
    if rebuild_fts_index:
        rebuild_fts(conn)
    conn.execute(f"PRAGMA user_version={SCHEMA_USER_VERSION}")
    conn.commit()


def migrate_database(
    db_path: pathlib.Path = DB_PATH,
    create_backup: bool = True,
    rebuild_fts_index: bool = True,
) -> pathlib.Path | None:
    db_path = pathlib.Path(db_path)
    backup_path = backup_database(db_path) if create_backup and db_path.exists() else None
    conn = sqlite3.connect(str(db_path))
    try:
        migrate_connection(conn, rebuild_fts_index=rebuild_fts_index)
    finally:
        conn.close()
    return backup_path


def main():
    parser = argparse.ArgumentParser(description="Migrate corpus.db to the current schema.")
    parser.add_argument("--db", default=str(DB_PATH), help="Path to corpus.db")
    parser.add_argument("--no-backup", action="store_true", help="Do not create a backup first")
    parser.add_argument("--no-fts", action="store_true", help="Do not rebuild FTS index")
    args = parser.parse_args()

    db_path = pathlib.Path(args.db)
    backup = migrate_database(
        db_path,
        create_backup=not args.no_backup,
        rebuild_fts_index=not args.no_fts,
    )
    if backup:
        print(f"已备份：{backup}")
    print(f"迁移完成：{db_path}")


if __name__ == "__main__":
    main()
