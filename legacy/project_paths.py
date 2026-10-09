"""
project_paths.py
Canonical filesystem paths for the Japanese corpus project.
"""

from __future__ import annotations

import os
import pathlib
import sys


def _source_root() -> pathlib.Path:
    """Project root when running from source.

    The 0.1.x code now lives in ``legacy/`` while the corpus (``corpus.db``,
    ``raw/``, ``metadata/``, ``output/``) and ``assets/`` stay at the repository
    root, shared with the 0.2.x app. So when this file sits in a directory named
    ``legacy``, the root is one level up; in any other layout it is this
    directory, as before.
    """
    here = pathlib.Path(__file__).resolve().parent
    return here.parent if here.name == "legacy" else here


def _default_data_dir() -> pathlib.Path:
    """Return a writable data directory for source and frozen builds."""
    if not getattr(sys, "frozen", False):
        return _source_root()

    local_app_data = os.environ.get("LOCALAPPDATA")
    if local_app_data:
        return pathlib.Path(local_app_data) / "JpopCorpusTool"
    return pathlib.Path.home() / "AppData" / "Local" / "JpopCorpusTool"


BASE_DIR = pathlib.Path(
    os.environ.get("JPOP_CORPUS_HOME") or _default_data_dir()
).resolve()
SETTINGS_PATH = BASE_DIR / "settings.json"
DB_PATH = BASE_DIR / "corpus.db"
LRC_DIR = BASE_DIR / "raw" / "lyrics_lrc"
CSV_PATH = BASE_DIR / "metadata" / "songs.csv"
AUDIO_DIR = BASE_DIR / "raw" / "audio"
PROCESSED_JSONL_PATH = BASE_DIR / "processed" / "utterances.jsonl"
BACKUP_DIR = BASE_DIR / "backups"

for directory in (
    BASE_DIR,
    LRC_DIR,
    AUDIO_DIR,
    CSV_PATH.parent,
    PROCESSED_JSONL_PATH.parent,
    BACKUP_DIR,
    BASE_DIR / "output",
):
    directory.mkdir(parents=True, exist_ok=True)


def app_dir() -> pathlib.Path:
    """Directory that contains the source entry point or frozen executable."""
    if getattr(sys, "frozen", False):
        return pathlib.Path(sys.executable).resolve().parent
    return _source_root()
