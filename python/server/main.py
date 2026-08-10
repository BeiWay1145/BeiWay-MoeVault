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
    }


# ---------- 打标 ----------
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
