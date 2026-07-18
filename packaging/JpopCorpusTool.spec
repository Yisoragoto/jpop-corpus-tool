# -*- mode: python ; coding: utf-8 -*-

from pathlib import Path

from PyInstaller.utils.hooks import collect_all, collect_submodules


SPEC_DIR = Path(SPECPATH).resolve()
ROOT = SPEC_DIR if (SPEC_DIR / "gui.py").exists() else SPEC_DIR.parent

datas = []
binaries = []
hiddenimports = []


def add_collect_all(package_name: str):
    try:
        pkg_datas, pkg_binaries, pkg_hidden = collect_all(package_name)
    except Exception:
        return
    datas.extend(pkg_datas)
    binaries.extend(pkg_binaries)
    hiddenimports.extend(pkg_hidden)


for package in (
    "spacy",
    "ginza",
    "ja_ginza",
    "sudachipy",
    "sudachidict_core",
    "thinc",
    "blis",
    "wordcloud",
    "qfluentwidgets",
    "qframelesswindow",
):
    add_collect_all(package)

for package in (
    "spacy.lang.ja",
    "PyQt6.QtMultimedia",
    "PyQt6.QtMultimediaWidgets",
):
    try:
        hiddenimports.extend(collect_submodules(package))
    except Exception:
        pass

hiddenimports.append("migrate_db")


a = Analysis(
    [str(ROOT / "gui.py")],
    pathex=[str(ROOT), str(ROOT / "scripts")],
    binaries=binaries,
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=["python-vlc"],
    noarchive=False,
    optimize=0,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="JpopCorpusTool",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    console=False,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=True,
    upx_exclude=[],
    name="JpopCorpusTool",
)
