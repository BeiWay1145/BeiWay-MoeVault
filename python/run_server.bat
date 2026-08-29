@echo off
rem BeiWay-MoeVault inference server launcher.
rem Python priority: 1) ./.venv  2) %%LOCALAPPDATA%%\BeiWay-MoeVault\python\.venv (app-managed)
rem                  3) py -3    4) python on PATH
rem NOTE: keep this file ASCII-only. cmd.exe mis-parses UTF-8 batch files
rem       (chcp 65001 re-read splits lines mid-byte -> comment fragments run as commands).
setlocal
cd /d "%~dp0"

set "PY="
if exist ".venv\Scripts\python.exe" set "PY=%CD%\.venv\Scripts\python.exe"
if not defined PY if exist "%LOCALAPPDATA%\BeiWay-MoeVault\python\.venv\Scripts\python.exe" set "PY=%LOCALAPPDATA%\BeiWay-MoeVault\python\.venv\Scripts\python.exe"
if not defined PY where py >nul 2>nul && set "PY=py -3"
if not defined PY where python >nul 2>nul && set "PY=python"

if not defined PY (
    echo [ERROR] Python not found. Install Python 3.10+ and add to PATH, or run setup.bat first.
    pause
    exit /b 1
)

echo [INFO] Python: %PY%
echo [INFO] Checking deps (fastapi/uvicorn)...
%PY% -c "import fastapi, uvicorn" >nul 2>nul
if errorlevel 1 (
    echo [INFO] Deps missing. Installing via Tsinghua mirror, may take minutes...
    %PY% -m pip install --disable-pip-version-check -r requirements.txt -i https://pypi.tuna.tsinghua.edu.cn/simple
    if errorlevel 1 (
        echo [INFO] Mirror failed, falling back to official PyPI...
        %PY% -m pip install --disable-pip-version-check -r requirements.txt
        if errorlevel 1 (
            echo [ERROR] Dep install failed. Check network, or run setup.bat to create a standalone .venv.
            pause
            exit /b 1
        )
    )
)

echo.
echo [INFO] Starting inference server: http://127.0.0.1:8001
echo [INFO] Health check:  curl http://127.0.0.1:8001/health
echo [INFO] Press Ctrl+C to stop.
echo.
%PY% -m uvicorn server.main:app --host 127.0.0.1 --port 8001
pause
