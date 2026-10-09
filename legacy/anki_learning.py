"""
anki_learning.py
Read JPOP Corpus learning state from Anki's collection.anki2 safely.
"""

from __future__ import annotations

import glob
import html
import os
import re
import shutil
import sqlite3
import tempfile
from dataclasses import dataclass, field
from pathlib import Path


NOTE_TYPE = "JPOP Corpus"
FIELD_SEP = "\x1f"
F_EXPRESSION = 0
TAG_RE = re.compile(r"<[^>]+>")


@dataclass
class WordLearningStatus:
    exists: bool = False
    studied: bool = False
    note_count: int = 0
    card_count: int = 0
    max_reps: int = 0
    decks: set[str] = field(default_factory=set)


def find_collection() -> Path | None:
    """Auto-detect Anki collection.anki2 on Windows, macOS, Linux, or cwd."""
    candidates: list[Path] = []

    appdata = os.environ.get("APPDATA", "")
    if appdata:
        candidates += [
            Path(p)
            for p in glob.glob(os.path.join(appdata, "Anki2", "*", "collection.anki2"))
        ]

    home = Path.home()
    candidates += list(
        (home / "Library/Application Support/Anki2").glob("*/collection.anki2")
    )
    candidates += list((home / ".local/share/Anki2").glob("*/collection.anki2"))

    local = Path("collection.anki2")
    if local.exists():
        candidates.append(local)

    existing = [p for p in candidates if p.exists() and p.stat().st_size > 0]
    if not existing:
        return None
    existing.sort(key=lambda p: p.stat().st_mtime, reverse=True)
    return existing[0]


def snapshot_collection(db_path: Path) -> tuple[tempfile.TemporaryDirectory, Path]:
    """Copy Anki DB and sidecar WAL/SHM files before reading."""
    td = tempfile.TemporaryDirectory(prefix="jpop-anki-state-")
    dst = Path(td.name) / db_path.name
    shutil.copy2(db_path, dst)
    for suffix in ("-wal", "-shm"):
        sidecar = db_path.with_name(db_path.name + suffix)
        if sidecar.exists():
            shutil.copy2(sidecar, dst.with_name(dst.name + suffix))
    return td, dst


def clean_field(raw: str) -> str:
    return TAG_RE.sub("", html.unescape(raw or "")).strip()


def _register_unicase(conn: sqlite3.Connection) -> None:
    def unicase(left: str, right: str) -> int:
        lval, rval = (left or "").casefold(), (right or "").casefold()
        return (lval > rval) - (lval < rval)
    conn.create_collation("unicase", unicase)


def load_learning_status(
    collection_path: Path | None = None,
    deck_filter: str = "",
) -> tuple[dict[str, WordLearningStatus], Path]:
    """
    Return expression -> learning status for JPOP Corpus notes.

    studied means at least one card for that expression has reps > 0.
    """
    db_path = collection_path or find_collection()
    if db_path is None:
        raise FileNotFoundError("找不到 Anki collection.anki2")

    temp_dir, snapshot_path = snapshot_collection(db_path)
    conn: sqlite3.Connection | None = None
    try:
        conn = sqlite3.connect(str(snapshot_path))
        _register_unicase(conn)
        conn.row_factory = sqlite3.Row

        row = conn.execute(
            "SELECT id FROM notetypes WHERE name=?", (NOTE_TYPE,)
        ).fetchone()
        if not row:
            return {}, db_path
        model_id = row["id"]

        deck_filter = deck_filter.strip().lower()
        rows = conn.execute(
            """
            SELECT n.id AS note_id,
                   n.flds AS fields,
                   d.name AS deck,
                   COUNT(c.id) AS card_count,
                   COALESCE(MAX(c.reps), 0) AS max_reps
            FROM notes n
            JOIN cards c ON c.nid = n.id
            JOIN decks d ON d.id = c.did
            WHERE n.mid = ?
            GROUP BY n.id
            """,
            (model_id,),
        ).fetchall()

        status: dict[str, WordLearningStatus] = {}
        for r in rows:
            deck = r["deck"] or ""
            if deck_filter and deck_filter not in deck.lower():
                continue
            fields = (r["fields"] or "").split(FIELD_SEP)
            expression = clean_field(fields[F_EXPRESSION] if fields else "")
            if not expression:
                continue

            item = status.setdefault(expression, WordLearningStatus())
            card_count = int(r["card_count"] or 0)
            max_reps = int(r["max_reps"] or 0)
            item.exists = True
            item.note_count += 1
            item.card_count += card_count
            item.max_reps = max(item.max_reps, max_reps)
            item.studied = item.studied or max_reps > 0
            if deck:
                item.decks.add(deck)

        return status, db_path
    finally:
        if conn is not None:
            conn.close()
        temp_dir.cleanup()
