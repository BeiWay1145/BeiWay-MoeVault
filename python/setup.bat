@echo off
chcp 65001 >nul
rem ============================================================
rem  BeiWay-MoeVault 推理服务一键环境搭建
rem  创建 python/.venv 并安装全部依赖（torch/transformers/onnxruntime 等）
rem  完成后即可 run_server.bat 启动，或由桌面壳自动拉起
rem ============================================================
setlocal
cd /d "%~dp0"

echo [1/4] 检查 Python 3.10+ ...
set "PY=python"
where py >nul 2>nul && set "PY=py -3"
%PY% --version 2>nul
if errorlevel 1 (
    echo [错误] 未找到 Python。请先安装 Python 3.10+ 并加入 PATH，然后重试。
    pause
    exit /b 1
)

echo [2/4] 创建虚拟环境 .venv ...
if not exist ".venv\Scripts\python.exe" (
    %PY% -m venv .venv
    if errorlevel 1 (
        echo [错误] venv 创建失败。
        pause
        exit /b 1
    )
)
set "VENV_PY=%CD%\.venv\Scripts\python.exe"

echo [3/4] 安装依赖（torch/transformers/onnxruntime 等，可能耗时数分钟）...
rem 优先清华镜像（大陆网络友好），失败回退官方 PyPI
%VENV_PY% -m pip install --upgrade pip -i https://pypi.tuna.tsinghua.edu.cn/simple
%VENV_PY% -m pip install -r requirements.txt -i https://pypi.tuna.tsinghua.edu.cn/simple
if errorlevel 1 (
    echo [信息] 镜像安装失败，回退官方 PyPI ...
    %VENV_PY% -m pip install -r requirements.txt
    if errorlevel 1 (
        echo [错误] 依赖安装失败，请检查网络后重试。
        pause
        exit /b 1
    )
)

echo [4/4] 验证关键依赖 ...
%VENV_PY% -c "import fastapi, uvicorn, transformers, onnxruntime, PIL, numpy; print('  基础依赖 OK')"
%VENV_PY% -c "import torch; print('  torch', torch.__version__)" 2>nul || echo  [警告] torch 未装（美学评分不可用；如需 GPU 请按官方方式安装 torch+cuda）

echo.
echo [完成] 环境就绪！
echo   启动推理服务: 运行 run_server.bat
echo   或直接: .venv\Scripts\python.exe -m uvicorn server.main:app --port 8001
echo   模型目录将自动探测：项目根 models/tagger、models/aesthetic（或环境变量覆盖）
echo.
pause