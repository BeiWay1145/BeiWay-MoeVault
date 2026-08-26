# -*- coding: utf-8 -*-
"""推理服务配置。全部支持环境变量覆盖，便于主服务启动时注入。

模型路径约定（优先级从高到低，自动探测，消除「依赖强绑定本机」）：
1. 环境变量显式指定（TAGGER_MODEL_DIR / AESTHETIC_MODEL）
2. 项目内 models/（推荐：本项目根目录 models/tagger、models/aesthetic）
3. 旧硬编码位置（D:/Game/AI/cl_tagger/models，仅作兼容迁移回退）
"""
import os
from pathlib import Path

HOST = os.environ.get("INFER_HOST", "127.0.0.1")
PORT = int(os.environ.get("INFER_PORT", "8001"))

# 项目根：python/server/config.py → 上溯 3 级 = 仓库根
PROJECT_ROOT = Path(__file__).resolve().parents[2]

# 本地模型目录里可能存在的相对路径
TAGGER_FILES = {
    "onnx": "model.onnx",
    "vocab": "model_vocabulary.json",
    "meta": "model_metadata.json",
}


def _detect_tagger_dir() -> Path:
    """自动探测打标模型目录。返回 Path（可能不存在，由 /health 报 failed）。"""
    env_dir = os.environ.get("TAGGER_MODEL_DIR")
    if env_dir:
        return Path(env_dir)
    candidates = [
        PROJECT_ROOT / "models" / "tagger",
        PROJECT_ROOT / "python" / "models" / "tagger",
        PROJECT_ROOT / "models",
    ]
    for d in candidates:
        if all((d / f).is_file() for f in TAGGER_FILES.values()):
            return d
    # 兼容迁移：旧硬编码位置仍然存在则继续使用
    legacy = Path(r"D:/Game/AI/cl_tagger/models")
    if all((legacy / f).is_file() for f in TAGGER_FILES.values()):
        return legacy
    # 全部未命中：返回首选候选路径（缺失详情由 /health 报告）
    return candidates[0]


def _detect_aesthetic_model() -> str:
    """自动探测美学模型：本地目录（含 config.json）或 HF 仓库名（首次联网下载）。"""
    env_model = os.environ.get("AESTHETIC_MODEL")
    if env_model:
        return env_model
    candidates = [
        PROJECT_ROOT / "models" / "aesthetic",
        PROJECT_ROOT / "python" / "models" / "aesthetic",
    ]
    for d in candidates:
        if (d / "config.json").is_file():
            return str(d)
    return "trojblue/distill-q-align-aesthetic-siglip2-base"


TAGGER_MODEL_DIR = _detect_tagger_dir()
TAGGER_DEFAULT_THRESHOLD = float(os.environ.get("TAGGER_DEFAULT_THRESHOLD", "0.5"))

AESTHETIC_MODEL = _detect_aesthetic_model()
# 输出变换：0=直接取 logit 并 clamp 到 [1,5]；1=sigmoid(logit)*4+1（若实测输出接近 [0,1] 再开启）
AESTHETIC_SIGMOID = int(os.environ.get("AESTHETIC_SIGMOID", "0"))
AESTHETIC_RANGE = (1.0, 5.0)


def tagger_paths() -> dict:
    d = Path(TAGGER_MODEL_DIR)
    return {k: d / v for k, v in TAGGER_FILES.items()}


def detected_paths() -> dict:
    """当前生效的模型路径（供 /health 暴露，前端状态卡片展示）。"""
    return {
        "tagger_model_dir": str(TAGGER_MODEL_DIR),
        "aesthetic_model": AESTHETIC_MODEL,
    }