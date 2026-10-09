@echo off
cd /d "%~dp0"

rem This file lives in legacy\. The virtual environment stays at the repository root.
set "ROOT=%~dp0..\"
set "PY="
if exist "%ROOT%.venv\Scripts\python.exe" set "PY=%ROOT%.venv\Scripts\python.exe"
if not defined PY if exist "%ROOT%venv\Scripts\python.exe" set "PY=%ROOT%venv\Scripts\python.exe"
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
    echo   python -m pip install -r legacy\requirements.txt
    pause
    exit /b 1
)

"%PY%" "%~dp0gui.py"
if %errorlevel% neq 0 pause
