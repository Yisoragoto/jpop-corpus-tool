@echo off
cd /d "%~dp0"

set "PY_BOOT="
where py >nul 2>nul
if %errorlevel% equ 0 set "PY_BOOT=py -3.12"

if not defined PY_BOOT (
    where python >nul 2>nul
    if %errorlevel% equ 0 set "PY_BOOT=python"
)

if not defined PY_BOOT (
    echo [ERROR] Python not found.
    echo Install Python 3.12 from https://www.python.org/downloads/windows/
    pause
    exit /b 1
)

if not exist ".venv\Scripts\python.exe" (
    echo Creating .venv ...
    %PY_BOOT% -m venv .venv
    if %errorlevel% neq 0 (
        echo [ERROR] Failed to create virtual environment.
        pause
        exit /b 1
    )
)

echo Installing dependencies ...
".venv\Scripts\python.exe" -m pip install --upgrade pip
if %errorlevel% neq 0 (
    pause
    exit /b 1
)

".venv\Scripts\python.exe" -m pip install -r requirements.txt
if %errorlevel% neq 0 (
    echo [ERROR] Dependency installation failed.
    pause
    exit /b 1
)

echo.
echo Setup complete.
echo You can now run run_gui.bat.
pause
