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

"%PY%" "%~dp0generate_report.py" %*
if %errorlevel% equ 0 (
    start "" "%ROOT%output\corpus_report.html"
) else (
    pause
)
