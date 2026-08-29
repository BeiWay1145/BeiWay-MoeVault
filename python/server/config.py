# -*- coding: utf-8 -*-
"""推理服务配置。全部支持环境变量覆盖，便于主服务启动时注入。

模型路径约定（优先级从高到低，自动探测，消除「依赖强绑定本机」）：
1. 环境变量显式指定（TAGGER_MODEL_DIR / TAGGER_MODEL_KIND / AESTHETIC_MODEL）
2. 项目内 models/（推荐：本项目根目录 models/tagger/<kind>、models/aesthetic）
3. 旧硬编码位置（D:/Game/AI/cl_tagger/models，仅作兼容迁移回退）

模型种类（TAGGER_MODEL_KIND）：
- cl_tagger：SIGLIP2 ONNX + model_vocabulary.json（idx_to_tag 词表）
- wd14：wd14 tagger ONNX + selected_tags.csv（标准词表）
- auto：按目录内容自动判定（含 model_vocabulary.json → cl_tagger；含 selected_tags.csv → wd14）
"""
import os
from pathlib import Path

HOST = os.environ.get("INFER_HOST", "127.0.0.1")
PORT = int(os.environ.get("INFER_PORT", "8001"))

# 项目根：python/server/config.py → 上溯 3 级 = 仓库根（或运行时数据目录）
PROJECT_ROOT = Path(__file__).resolve().parents[2]

# ---- 模型种类与文件清单 ----
MODEL_KIND_AUTO = "auto"
MODEL_KIND_CL_TAGGER = "cl_tagger"
MODEL_KIND_WD14 = "wd14"

# 每种模型种类的必需文件（区别于其他种类）
MODEL_KIND_MARKERS = {
    MODEL_KIND_CL_TAGGER: {
        "onnx": "model.onnx",
        "vocab": "model_vocabulary.json",
        "meta": "model_metadata.json",
    },
    MODEL_KIND_WD14: {
        "onnx": "model.onnx",
        "tags": "selected_tags.csv",
    },
}

# 默认子目录约定：models/tagger/<kind>/
TAGGER_KIND_DIRS = {
    MODEL_KIND_CL_TAGGER: "cl_tagger",
    MODEL_KIND_WD14: "wd14",
}

# 旧硬编码位置（仅兼容回退；属于 cl_tagger）
LEGACY_TAGGER_DIR = Path(r"D:/Game/AI/cl_tagger/models")


def detect_kind(model_dir: str | Path) -> str:
    """按目录内容判定模型种类；无法判定返回 auto。"""
    d = Path(model_dir)
    if (d / "model_vocabulary.json").is_file() and (d / "model.onnx").is_file():
        return MODEL_KIND_CL_TAGGER
    if (d / "selected_tags.csv").is_file() and (d / "model.onnx").is_file():
        return MODEL_KIND_WD14
    return MODEL_KIND_AUTO


def _find_tagger_dir() -> Path:
    """自动探测打标模型目录。返回 Path（可能不存在，由 /health 报 failed）。"""
    env_dir = os.environ.get("TAGGER_MODEL_DIR")
    if env_dir:
        return Path(env_dir)

    env_kind = os.environ.get("TAGGER_MODEL_KIND")
    # 候选目录：项目内 models/tagger/<kind> 下各子目录 + 通用 models/tagger + models
    candidates: list[Path] = []
    root_candidates = [
        PROJECT_ROOT / "models" / "tagger",
        PROJECT_ROOT / "python" / "models" / "tagger",
    ]
    if env_kind and env_kind != MODEL_KIND_AUTO:
        # 指定种类：优先该种类的标准子目录
        sub = TAGGER_KIND_DIRS.get(env_kind)
        if sub:
            for rc in root_candidates:
                candidates.append(rc / sub)
    for rc in root_candidates:
        candidates.append(rc)
    candidates.append(PROJECT_ROOT / "models")

    # 依次检查：候选目录里能找到任一已知种类的文件即命中（auto 判定种类）
    for d in candidates:
        if detect_kind(d) != MODEL_KIND_AUTO:
            return d

    # 兼容迁移：旧硬编码位置仍然存在则继续使用
    if detect_kind(LEGACY_TAGGER_DIR) != MODEL_KIND_AUTO:
        return LEGACY_TAGGER_DIR
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


TAGGER_MODEL_DIR = _find_tagger_dir()
TAGGER_MODEL_KIND = os.environ.get("TAGGER_MODEL_KIND") or detect_kind(TAGGER_MODEL_DIR)
TAGGER_DEFAULT_THRESHOLD = float(os.environ.get("TAGGER_DEFAULT_THRESHOLD", "0.5"))
# wd14 常用阈值（0.35~0.4）；可用 TAGGER_WD14_THRESHOLD env 覆盖
TAGGER_WD14_THRESHOLD = float(os.environ.get("TAGGER_WD14_THRESHOLD", "0.35"))

AESTHETIC_MODEL = _detect_aesthetic_model()
# 输出变换：0=直接取 logit 并 clamp 到 [1,5]；1=sigmoid(logit)*4+1（若实测输出接近 [0,1] 再开启）
AESTHETIC_SIGMOID = int(os.environ.get("AESTHETIC_SIGMOID", "0"))
AESTHETIC_RANGE = (1.0, 5.0)


def tagger_paths() -> dict:
    """当前种类所需的模型文件路径。"""
    files = MODEL_KIND_MARKERS.get(TAGGER_MODEL_KIND, MODEL_KIND_MARKERS[MODEL_KIND_CL_TAGGER])
    d = Path(TAGGER_MODEL_DIR)
    return {k: d / v for k, v in files.items()}


def detected_paths() -> dict:
    """当前生效的模型路径与种类（供 /health 暴露，前端状态卡片展示）。"""
    return {
        "tagger_model_dir": str(TAGGER_MODEL_DIR),
        "tagger_model_kind": TAGGER_MODEL_KIND,
        "aesthetic_model": AESTHETIC_MODEL,
    }