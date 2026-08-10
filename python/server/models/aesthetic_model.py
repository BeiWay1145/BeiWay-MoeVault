# -*- coding: utf-8 -*-
"""美学评分模型：trojblue/distill-q-align-aesthetic-siglip2-base。

transformers regression 模型（SiglipForImageClassification，problem_type=regression），
输出单个 logit，代表 Q-Align 1-5 质量分。默认直接取 logit 并 clamp 到 [1,5]；
若实测输出接近 [0,1]，可将环境变量 AESTHETIC_SIGMOID=1 切换为 sigmoid*4+1。
"""
import threading
import time

from .. import config


class AestheticModel:
    def __init__(self):
        self._lock = threading.Lock()
        self._model = None
        self._processor = None
        self._load_error = None

    # ---------- 加载 ----------
    def load(self) -> None:
        with self._lock:
            if self._model is not None:
                return
            try:
                import torch  # noqa: F401  # 确保 torch 可用，给出明确报错
                from transformers import AutoImageProcessor, AutoModelForImageClassification

                model_ref = config.AESTHETIC_MODEL
                # 用 AutoImageProcessor（纯视觉），避免 Siglip2 的 AutoProcessor
                # 在 transformers 5.x 下尝试构建 tokenizer 失败
                self._processor = AutoImageProcessor.from_pretrained(model_ref)
                self._model = AutoModelForImageClassification.from_pretrained(model_ref)
                self._model.eval()
                if torch.cuda.is_available():
                    self._model = self._model.to("cuda")
                self._load_error = None
            except Exception as e:  # noqa: BLE001
                self._load_error = f"{type(e).__name__}: {e}"
                raise

    @property
    def loaded(self) -> bool:
        return self._model is not None

    @property
    def load_error(self):
        return self._load_error

    # ---------- 推理 ----------
    def score(self, image_path) -> dict:
        import torch
        from PIL import Image

        self.load()
        image = Image.open(image_path).convert("RGB")
        inputs = self._processor(images=image, return_tensors="pt")
        if torch.cuda.is_available():
            inputs = {k: v.to("cuda") for k, v in inputs.items()}
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
            "transform": "sigmoid*4+1" if config.AESTHETIC_SIGMOID else "identity",
        }


def time_ms(t0: float) -> int:
    return int(round((time.perf_counter() - t0) * 1000))
