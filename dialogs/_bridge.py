"""
Temporary bridge for dialog modules during the gui.py split.

The dialogs have moved out of gui.py first; shared helpers will be moved into
dedicated service modules in later steps. Keeping this bridge explicit avoids
large circular-import rewrites in the same change.
"""

from __future__ import annotations

import gui as _gui


def install_gui_symbols(namespace: dict) -> None:
    namespace.update({
        name: getattr(_gui, name)
        for name in dir(_gui)
        if not name.startswith("__")
    })
