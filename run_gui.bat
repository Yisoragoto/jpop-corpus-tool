@echo off
cd /d "%~dp0"

set "PY="
if exist "%~dp0.venv\Scripts\python.exe" set "PY=%~dp0.venv\Scripts\python.exe"
if not defined PY if exist "%~dp0venv\Scripts\python.exe" set "PY=%~dp0venv\Scripts\python.exe"
if not defined PY (
    where python >nul 2>nul
    if %errorlevel% equ 0 set "PY=python"
)

if not defined PY (
    echo [ERROR] Python not found.
    echo Install Python 3.12 first, then run setup_windows.bat.
    pause
    exit /b 1
)

"%PY%" -c "import PyQt6, spacy; from PyQt6.QtMultimedia import QMediaPlayer" >nul 2>nul
if %errorlevel% neq 0 (
    echo [ERROR] Required Python packages are missing.
    echo Run setup_windows.bat first, or install dependencies manually:
    echo   python -m pip install -r requirements.txt
    pause
    exit /b 1
)

"%PY%" "%~dp0gui.py"
if %errorlevel% neq 0 pause
