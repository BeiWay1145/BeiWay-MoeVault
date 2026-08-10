@echo off
chcp 65001 >nul
rem 推理服务启动脚本
rem Python 优先级：1) 本项目 python/.venv  2) 现有 cl_tagger/.venv（缺 torch，美学会降级） 3) 系统 python
setlocal
cd /d "%~dp0"

set PY=
if exist ".venv\Scripts\python.exe" (
    set PY=%CD%\.venv\Scripts\python.exe
) else if exist "D:\Game\AI\cl_tagger\.venv\Scripts\python.exe" (
    set PY=D:\Game\AI\cl_tagger\.venv\Scripts\python.exe
) else (
    where python >nul 2>nul && set PY=python
)

if not defined PY (
    echo [错误] 未找到 Python。请安装 Python 3.10+ 或创建 .venv。
    pause
    exit /b 1
)

echo [信息] 使用 Python: %PY%
echo [信息] 检查依赖（fastapi/uvicorn）...
%PY% -c "import fastapi, uvicorn" 2>nul
if errorlevel 1 (
    echo [信息] 安装依赖（首次运行，可能耗时）...
    %PY% -m pip install -r requirements.txt -i https://pypi.tuna.tsinghua.edu.cn/simple
    if errorlevel 1 (
        echo [错误] 依赖安装失败。
        pause
        exit /b 1
    )
)

echo.
echo [信息] 启动推理服务: http://127.0.0.1:8001
echo [信息] 健康检查:     curl http://127.0.0.1:8001/health
echo [信息] Ctrl+C 停止
echo.
%PY% -m uvicorn server.main:app --host 127.0.0.1 --port 8001
pause
