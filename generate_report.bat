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

"%PY%" "%~dp0generate_report.py" %*
if %errorlevel% equ 0 (
    start "" "%~dp0output\corpus_report.html"
) else (
    pause
)
