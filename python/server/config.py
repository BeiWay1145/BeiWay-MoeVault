# -*- coding: utf-8 -*-
"""推理服务配置。全部支持环境变量覆盖，便于主服务启动时注入。"""
import os
from pathlib import Path

HOST = os.environ.get("INFER_HOST", "127.0.0.1")
PORT = int(os.environ.get("INFER_PORT", "8001"))

# ---- 打标模型（cl_tagger 抽取）----
# 默认指向现有 cl_tagger 的模型目录；模型文件缺失时 /health 会报告 failed
TAGGER_MODEL_DIR = os.environ.get(
    "TAGGER_MODEL_DIR", r"D:/Game/AI/cl_tagger/models"
)
TAGGER_DEFAULT_THRESHOLD = float(os.environ.get("TAGGER_DEFAULT_THRESHOLD", "0.5"))

# ---- 美学模型 ----
# 可传本地目录路径（含 config.json）或 HF 仓库名；首次运行会联网下载
AESTHETIC_MODEL = os.environ.get(
    "AESTHETIC_MODEL", "trojblue/distill-q-align-aesthetic-siglip2-base"
)
# 输出变换：0=直接取 logit 并 clamp 到 [1,5]；1=sigmoid(logit)*4+1（若实测输出接近 [0,1] 再开启）
AESTHETIC_SIGMOID = int(os.environ.get("AESTHETIC_SIGMOID", "0"))
AESTHETIC_RANGE = (1.0, 5.0)

# 本地模型目录里可能存在的相对路径
TAGGER_FILES = {
    "onnx": "model.onnx",
    "vocab": "model_vocabulary.json",
    "meta": "model_metadata.json",
}


def tagger_paths() -> dict:
    d = Path(TAGGER_MODEL_DIR)
    return {k: d / v for k, v in TAGGER_FILES.items()}
