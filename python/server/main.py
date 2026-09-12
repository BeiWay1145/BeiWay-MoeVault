# -*- coding: utf-8 -*-
"""推理服务入口：FastAPI，供 Rust 主服务通过 127.0.0.1 调用。

接口契约见 docs/TECH_DETAILS.md 第 4 节。
- 输入传本地绝对路径（同机部署，避免 base64 传输开销）
- 打标与美学共用显存，全部经同一线程锁串行执行（GPU 安全）
"""
import threading
import time
from pathlib import Path

# torch 在 uvicorn 多线程 worker 里首次加载模型可能死锁（OpenMP/内部线程池竞争）。
# 强制单线程推理可避免；打标用 onnxruntime 不受影响。
try:
    import torch

    torch.set_num_threads(1)
except Exception:
    pass

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field

from . import config
from .models.aesthetic_model import AestheticModel, time_ms
from .models.tagger_model import TaggerModel

app = FastAPI(title="Image Manager Inference Server", version="0.1.0")


@app.middleware("http")
async def close_connection(request, call_next):
    """修复：reqwest/keep-alive 客户端调用 /infer/aesthetic 时 uvicorn 挂起（模型加载线程与
    keep-alive 连接交互死锁）。强制响应 Connection: close，避免连接复用触发问题。"""
    import time as _t

    try:
        response = await call_next(request)
    except Exception:
        raise
    response.headers["Connection"] = "close"
    return response

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
            "tagger": {
                "state": tagger_state,
                "error": _tagger.load_error,
                "kind": _tagger.kind,
            },
            "aesthetic": {"state": aesthetic_state, "error": _aesthetic.load_error},
        },
        "paths": config.detected_paths(),
    }


def _friendly_cpu_name() -> str:
    """Windows 下从注册表读 CPU 友好名（如 Intel(R) Core(TM) Ultra 9 285H）。"""
    try:
        import winreg

        with winreg.OpenKey(
            winreg.HKEY_LOCAL_MACHINE,
            r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
        ) as k:
            name, _ = winreg.QueryValueEx(k, "ProcessorNameString")
            return str(name).strip()
    except Exception:
        try:
            import platform

            return platform.processor() or "CPU"
        except Exception:
            return "CPU"


@app.get("/devices")
def devices():
    """列出可用的推理设备（供设置页下拉选择；增强1：GPU 编号+全名、CPU 友好名）。"""
    result = []
    # 物理 GPU 列表（torch 为准；打标 onnxruntime 与美学 torch 共用同一批 GPU）
    gpus: list[tuple[int, str]] = []
    torch_cuda = False
    try:
        import torch

        torch_cuda = torch.cuda.is_available()
        if torch_cuda:
            for i in range(torch.cuda.device_count()):
                gpus.append((i, torch.cuda.get_device_name(i)))
    except Exception:
        pass

    cpu_name = _friendly_cpu_name()

    # 打标：onnxruntime 可用 providers（CUDA provider 存在即 GPU 可用）
    ort_cuda = False
    try:
        import onnxruntime as ort

        for p in ort.get_available_providers():
            if "CUDA" in p:
                ort_cuda = True
    except Exception:
        pass

    # 打标设备：GPU 存在（onnxruntime CUDA 或 torch CUDA）→ 逐卡列出；否则 CPU
    if ort_cuda or torch_cuda:
        if gpus:
            for i, name in gpus:
                result.append({"id": f"cuda:{i}", "name": f"CUDA GPU{i + 1}（{name}）", "kind": "tagger"})
        else:
            result.append({"id": "cuda", "name": "CUDA GPU", "kind": "tagger"})
    result.append({"id": "cpu", "name": f"CPU（{cpu_name}）", "kind": "tagger"})

    # 美学设备：torch cuda 逐卡；否则 CPU
    if torch_cuda:
        for i, name in gpus:
            result.append({"id": f"cuda:{i}", "name": f"CUDA GPU{i + 1}（{name}）", "kind": "aesthetic"})
    else:
        result.append({"id": "cpu", "name": f"CPU（{cpu_name}）", "kind": "aesthetic"})
    return {"devices": result}


# ---------- 打标 ----------
class TaggerConfigRequest(BaseModel):
    model_dir: str
    # 可选：cl_tagger / wd14 / auto（缺省 auto=按目录内容判定）
    model_kind: str | None = None


@app.post("/infer/tagger/config")
def tagger_config(req: TaggerConfigRequest):
    """切换打标模型目录/种类（重新加载模型）。"""
    import os

    if not os.path.isdir(req.model_dir):
        raise HTTPException(status_code=404, detail=f"模型目录不存在: {req.model_dir}")
    try:
        with _infer_lock:
            _tagger.load_from_dir(req.model_dir, kind=req.model_kind)
        return {
            "ok": True,
            "model_dir": _tagger.model_dir,
            "model_kind": _tagger.kind,
            "tags": len(_tagger._idx_to_tag),
        }
    except Exception as e:  # noqa: BLE001
        raise HTTPException(status_code=500, detail=f"模型切换失败: {e}") from e


class AestheticConfigRequest(BaseModel):
    kind: str  # qalign / anime（议题4：二次元美学模型切换）


@app.post("/infer/aesthetic/config")
def aesthetic_config(req: AestheticConfigRequest):
    """切换美学模型种类（qalign/anime，下次评分生效）。"""
    _aesthetic.set_kind(req.kind)
    return {"ok": True, "kind": _aesthetic.kind}


@app.post("/infer/tags")
def infer_tags(req: TagRequest):
    _check_path(req.path)
    t0 = time.perf_counter()
    try:
        with _infer_lock:
            tags = _tagger.infer(req.path, req.threshold)
        return {
            "tags": [{"name": k, "confidence": v} for k, v in tags.items()],
            "model": _tagger.kind or "unknown",
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
