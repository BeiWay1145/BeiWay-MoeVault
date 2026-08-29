@echo off
rem ============================================================
rem  BeiWay-MoeVault inference env bootstrap (one-click)
rem  Creates python/.venv and installs all deps
rem  (torch/transformers/onnxruntime etc).
rem  After setup, start with run_server.bat, or the desktop
rem  shell will spawn the server automatically.
rem  NOTE: keep this file ASCII-only. cmd.exe mis-parses UTF-8
rem        batch files (chcp 65001 line-split bug).
rem ============================================================
setlocal
cd /d "%~dp0"

echo [1/4] Checking Python 3.10+ ...
set "PY=python"
where py >nul 2>nul && set "PY=py -3"
%PY% --version >nul 2>nul
if errorlevel 1 (
    echo [ERROR] Python not found. Install Python 3.10+ and add it to PATH, then retry.
    pause
    exit /b 1
)

echo [2/4] Creating virtual env .venv ...
if not exist ".venv\Scripts\python.exe" (
    %PY% -m venv .venv
    if errorlevel 1 (
        echo [ERROR] venv creation failed.
        pause
        exit /b 1
    )
)
set "VENV_PY=%CD%\.venv\Scripts\python.exe"

echo [3/4] Installing deps (torch/transformers/onnxruntime etc, may take minutes)...
rem Tsinghua mirror first (mainland-network friendly), official PyPI as fallback
%VENV_PY% -m pip install --upgrade pip -i https://pypi.tuna.tsinghua.edu.cn/simple
%VENV_PY% -m pip install -r requirements.txt -i https://pypi.tuna.tsinghua.edu.cn/simple
if errorlevel 1 (
    echo [INFO] Mirror install failed, falling back to official PyPI ...
    %VENV_PY% -m pip install -r requirements.txt
    if errorlevel 1 (
        echo [ERROR] Dep install failed. Check your network and retry.
        pause
        exit /b 1
    )
)

echo [4/4] Verifying key deps ...
%VENV_PY% -c "import fastapi, uvicorn, transformers, onnxruntime, PIL, numpy; print('  base deps OK')"
%VENV_PY% -c "import torch; print('  torch', torch.__version__)" 2>nul || echo  [WARN] torch missing (aesthetic scoring disabled; install torch manually for GPU)

echo.
echo [DONE] Environment ready.
echo   Start server: run_server.bat
echo   Or directly:  .venv\Scripts\python.exe -m uvicorn server.main:app --port 8001
echo   Model dirs auto-detected: models/tagger, models/aesthetic (env overridable)
echo.
pause
