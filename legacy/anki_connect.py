"""
anki_connect.py
Small AnkiConnect HTTP client used by the corpus GUI.

Keep this module free of PyQt and project UI state so it is safe to test and
reuse from workers or maintenance scripts.
"""

from __future__ import annotations

import json
import urllib.request


ANKI_CONNECT_URL = "http://127.0.0.1:8765"
ANKI_CONNECT_VERSION = 6


def request(action: str, **params):
    """Call AnkiConnect and return the result payload."""
    payload = json.dumps(
        {"action": action, "version": ANKI_CONNECT_VERSION, "params": params}
    ).encode()
    try:
        with urllib.request.urlopen(ANKI_CONNECT_URL, payload, timeout=5) as r:
            resp = json.loads(r.read())
        if resp.get("error"):
            raise RuntimeError(resp["error"])
        return resp.get("result")
    except OSError as e:
        raise ConnectionError(
            f"AnkiConnect に接続できません（Anki を起動してください）: {e}"
        )
