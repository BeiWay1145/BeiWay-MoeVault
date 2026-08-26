# -*- coding: utf-8 -*-
"""推理服务入口：FastAPI，供 Rust 主服务通过 127.0.0.1 调用。

接口契约见 docs/TECH_DETAILS.md 第 4 节。
- 输入传本地绝对路径（同机部署，避免 base64 传输开销）
- 打标与美学共用显存，全部经同一线程锁串行执行（GPU 安全）
"""
import threading
import time
from pathlib import Path

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field

from . import config
from .models.aesthetic_model import AestheticModel, time_ms
from .models.tagger_model import TaggerModel

app = FastAPI(title="Image Manager Inference Server", version="0.1.0")

# 全局单例 + 串行锁（GPU 显存安全）
_tagger = TaggerModel()
_aesthetic = AestheticModel()
_infer_lock = threading.Lock()


# ---------- 请求模型 ----------
class TagRequest(BaseModel):
    path: str
    threshold: float | None = Field(default=None, ge=0.0, le=1.0)


class BatchTagRequest(BaseModel):
    paths: list[str]
    threshold: float | None = Field(default=None, ge=0.0, le=1.0)


class AestheticRequest(BaseModel):
    path: str


class BatchAestheticRequest(BaseModel):
    paths: list[str]


# ---------- 工具 ----------
def _check_path(path: str) -> Path:
    p = Path(path)
    if not p.is_file():
        raise HTTPException(status_code=404, detail=f"文件不存在: {path}")
    return p


# ---------- 健康检查 ----------
@app.get("/health")
def health():
    tagger_state = "ok" if _tagger.loaded else ("failed" if _tagger.load_error else "not_loaded")
    aesthetic_state = (
        "ok" if _aesthetic.loaded else ("failed" if _aesthetic.load_error else "not_loaded")
    )
    return {
        "status": "ok",
        "models": {
            "tagger": {"state": tagger_state, "error": _tagger.load_error},
            "aesthetic": {"state": aesthetic_state, "error": _aesthetic.load_error},
        },
        "paths": config.detected_paths(),
    }


@app.get("/devices")
def devices():
    """列出可用的推理设备（供设置页下拉选择）。"""
    result = []
    # 打标：onnxruntime 可用 providers
    try:
        import onnxruntime as ort
        for p in ort.get_available_providers():
            if "CUDA" in p:
                result.append({"id": "cuda", "name": f"GPU ({p})", "kind": "tagger"})
            elif "CPU" in p:
                result.append({"id": "cpu", "name": "CPU", "kind": "tagger"})
    except Exception:
        pass
    # 美学：torch cuda
    try:
        import torch
        if torch.cuda.is_available():
            for i in range(torch.cuda.device_count()):
                name = torch.cuda.get_device_name(i)
                result.append({"id": f"cuda:{i}", "name": f"GPU ({name})", "kind": "aesthetic"})
        else:
            result.append({"id": "cpu", "name": "CPU", "kind": "aesthetic"})
    except Exception:
        pass
    return {"devices": result}


# ---------- 打标 ----------
class TaggerConfigRequest(BaseModel):
    model_dir: str


@app.post("/infer/tagger/config")
def tagger_config(req: TaggerConfigRequest):
    """切换打标模型目录（重新加载模型）。"""
    import os

    if not os.path.isdir(req.model_dir):
        raise HTTPException(status_code=404, detail=f"模型目录不存在: {req.model_dir}")
    try:
        with _infer_lock:
            _tagger.load_from_dir(req.model_dir)
        return {
            "ok": True,
            "model_dir": _tagger.model_dir,
            "tags": len(_tagger._idx_to_tag),
        }
    except Exception as e:  # noqa: BLE001
        raise HTTPException(status_code=500, detail=f"模型切换失败: {e}") from e


@app.post("/infer/tags")
def infer_tags(req: TagRequest):
    _check_path(req.path)
    t0 = time.perf_counter()
    try:
        with _infer_lock:
            tags = _tagger.infer(req.path, req.threshold)
        return {
            "tags": [{"name": k, "confidence": v} for k, v in tags.items()],
            "model": "siglip2-tagger",
            "elapsed_ms": time_ms(t0),
        }
    except Exception as e:  # noqa: BLE001
        raise HTTPException(status_code=500, detail=f"打标失败: {e}") from e


@app.post("/infer/tags_batch")
def infer_tags_batch(req: BatchTagRequest):
    if not req.paths:
        raise HTTPException(status_code=422, detail="paths 不能为空")
    results = []
    for p in req.paths:
        _check_path(p)
        t0 = time.perf_counter()
        try:
            with _infer_lock:
                tags = _tagger.infer(p, req.threshold)
            results.append(
                {
                    "path": p,
                    "ok": True,
                    "tags": [{"name": k, "confidence": v} for k, v in tags.items()],
                    "elapsed_ms": time_ms(t0),
                }
            )
        except Exception as e:  # noqa: BLE001
            results.append({"path": p, "ok": False, "error": str(e)})
    return {"results": results}


# ---------- 美学评分 ----------
@app.post("/infer/aesthetic")
def infer_aesthetic(req: AestheticRequest):
    _check_path(req.path)
    t0 = time.perf_counter()
    try:
        with _infer_lock:
            out = _aesthetic.score(req.path)
        out["elapsed_ms"] = time_ms(t0)
        return out
    except Exception as e:  # noqa: BLE001
        raise HTTPException(status_code=500, detail=f"美学评分失败: {e}") from e


@app.post("/infer/aesthetic_batch")
def infer_aesthetic_batch(req: BatchAestheticRequest):
    if not req.paths:
        raise HTTPException(status_code=422, detail="paths 不能为空")
    results = []
    for p in req.paths:
        _check_path(p)
        t0 = time.perf_counter()
        try:
            with _infer_lock:
                out = _aesthetic.score(p)
            out["elapsed_ms"] = time_ms(t0)
            results.append({"path": p, "ok": True, **out})
        except Exception as e:  # noqa: BLE001
            results.append({"path": p, "ok": False, "error": str(e)})
    return {"results": results}
