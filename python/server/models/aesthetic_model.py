# -*- coding: utf-8 -*-
"""美学评分模型（双后端）：

- qalign：trojblue/distill-q-align-aesthetic-siglip2-base（默认；偏真人摄影）
- anime：Disty0/aesthetic-shadow-v2（shadowlilac，二次元特化 ViT 分类器，probs[0,0] 为美学概率 0-1）

两种后端统一输出 1-5 分制（后端筛选/显示沿用该区间）：
- qalign：logit 直接取（或 sigmoid*4+1），clamp [1,5]
- anime：score01 ∈ [0,1]（阈值：≥0.925 very / ≥0.911 highly / ≥0.875 moderate / ≥0.825 low / ≥0.750 bad）
  线性映射到 1-5：1 + score01 * 4（0.925 → 4.7，分布拉宽、排序一致），raw 保留原始 0-1
"""
import os
import threading
import time

from .. import config


class AestheticModel:
    def __init__(self):
        self._lock = threading.Lock()
        self._kind = config.AESTHETIC_KIND if config.AESTHETIC_KIND in ("qalign", "anime") else "qalign"
        self._model = None
        self._processor = None
        self._load_error = None
        # 推理设备：auto / cuda / cuda:N / cpu（auto = 有 CUDA 就用）
        self._device = "auto"
        # anime 后端独立的模型句柄（切换 kind 时互不干扰）
        self._anime_model = None
        self._anime_processor = None
        self._anime_load_error = None

    # ---------- kind ----------
    @property
    def kind(self) -> str:
        return self._kind

    def set_kind(self, kind: str) -> None:
        kind = (kind or "").strip().lower()
        if kind in ("qalign", "anime"):
            self._kind = kind

    # ---------- device ----------
    @property
    def device(self) -> str:
        return self._device

    def set_device(self, device: str) -> None:
        """切换推理设备；已加载的模型立即迁移到新设备（不重新下载权重）。

        device：auto / cuda / cuda:N / cpu
        """
        device = (str(device) or "").strip().lower() or "auto"
        with self._lock:
            if device == self._device:
                return
            self._device = device
            try:
                import torch

                target = self._torch_device(torch)
                if self._model is not None:
                    self._model = self._model.to(target)
                if self._anime_model is not None:
                    self._anime_model = self._anime_model.to(target)
            except Exception as e:  # noqa: BLE001
                self._load_error = f"{type(e).__name__}: {e}"

    def _torch_device(self, torch):
        """把 device 字符串解析成 torch.device（不可用时回落 cpu）。"""
        want = self._device or "auto"
        has_cuda = torch.cuda.is_available()
        if want == "auto":
            return torch.device("cuda" if has_cuda else "cpu")
        if want.startswith("cuda"):
            if not has_cuda:
                return torch.device("cpu")
            if ":" in want:
                try:
                    return torch.device("cuda", int(want.split(":", 1)[1]))
                except (ValueError, RuntimeError):
                    return torch.device("cuda")
            return torch.device("cuda")
        return torch.device("cpu")

    # ---------- 加载 ----------
    def load(self) -> None:
        with self._lock:
            if self._kind == "anime":
                self._load_anime()
            else:
                self._load_qalign()

    def _load_qalign(self) -> None:
        if self._model is not None:
            return
        try:
            import torch  # noqa: F401
            from transformers import AutoImageProcessor, AutoModelForImageClassification

            model_ref = config.AESTHETIC_MODEL
            self._processor = AutoImageProcessor.from_pretrained(model_ref)
            self._model = AutoModelForImageClassification.from_pretrained(model_ref)
            self._model.eval()
            self._model = self._model.to(self._torch_device(torch))
            self._load_error = None
        except Exception as e:  # noqa: BLE001
            self._load_error = f"{type(e).__name__}: {e}"
            raise

    def _load_anime(self) -> None:
        if self._anime_model is not None:
            return
        try:
            import torch  # noqa: F401

            # torch 多线程首次加载模型可能死锁（uvicorn worker 环境）；强制单线程
            torch.set_num_threads(1)

            from transformers import ViTForImageClassification, ViTImageProcessor

            # 本地目录优先（models/aesthetic_anime），否则 HF 下载
            model_ref = config.ANIME_AESTHETIC_MODEL
            for cand in (
                config.PROJECT_ROOT / "models" / "aesthetic_anime",
                config.PROJECT_ROOT / "python" / "models" / "aesthetic_anime",
            ):
                if (cand / "config.json").is_file():
                    model_ref = str(cand)
                    break
            self._anime_processor = ViTImageProcessor.from_pretrained(model_ref, use_fast=True)
            self._anime_model = ViTForImageClassification.from_pretrained(model_ref, torch_dtype=torch.float32)
            self._anime_model.eval()
            self._anime_model = self._anime_model.to(self._torch_device(torch))
            self._anime_load_error = None
        except Exception as e:  # noqa: BLE001
            self._anime_load_error = f"{type(e).__name__}: {e}"
            raise

    @property
    def loaded(self) -> bool:
        if self._kind == "anime":
            return self._anime_model is not None
        return self._model is not None

    @property
    def load_error(self):
        return self._anime_load_error if self._kind == "anime" else self._load_error

    # ---------- 推理 ----------
    def score(self, image_path) -> dict:
        if self._kind == "anime":
            return self._score_anime(image_path)
        return self._score_qalign(image_path)

    def _score_qalign(self, image_path) -> dict:
        import torch
        from PIL import Image

        self.load()
        image = Image.open(image_path).convert("RGB")
        inputs = self._processor(images=image, return_tensors="pt")
        target = self._torch_device(torch)
        if target.type == "cuda":
            inputs = {k: v.to(target) for k, v in inputs.items()}
        with torch.no_grad():
            out = self._model(**inputs)
        raw = float(out.logits.reshape(-1)[0])

        if config.AESTHETIC_SIGMOID:
            score = 1.0 + 4.0 * (1.0 / (1.0 + float(torch.sigmoid(torch.tensor(raw)))))
        else:
            score = raw
        lo, hi = config.AESTHETIC_RANGE
        score = max(lo, min(hi, score))
        return {
            "score": round(score, 4),
            "raw": round(raw, 6),
            "range": [lo, hi],
            "model": config.AESTHETIC_MODEL,
            "kind": "qalign",
            "transform": "sigmoid*4+1" if config.AESTHETIC_SIGMOID else "identity",
        }

    def _score_anime(self, image_path) -> dict:
        import torch
        from PIL import Image

        self.load()
        # RGBA → 白底合成 → RGB（官方 Space 预处理）
        image = Image.open(image_path).convert("RGBA")
        background = Image.new("RGBA", image.size, (255, 255, 255))
        image = Image.alpha_composite(background, image).convert("RGB")
        inputs = self._anime_processor(images=image, return_tensors="pt")
        target = self._torch_device(torch)
        if target.type == "cuda":
            inputs = {k: v.to(target) for k, v in inputs.items()}
        with torch.no_grad():
            logits = self._anime_model(**inputs).logits
            probs = torch.softmax(logits, dim=-1)
        score01 = float(probs[0, 0].item())
        # 线性映射到 1-5（分布拉宽、与后端区间一致；排序不变）
        score = 1.0 + score01 * 4.0
        return {
            "score": round(score, 4),
            "raw": round(score01, 6),
            "range": [1.0, 5.0],
            "model": config.ANIME_AESTHETIC_MODEL,
            "kind": "anime",
            "transform": "1+score01*4",
        }


def time_ms(t0: float) -> int:
    return int(round((time.perf_counter() - t0) * 1000))
