# -*- coding: utf-8 -*-
"""cl_tagger 核心推理抽取（不含 Gradio UI）。

来源：D:/Game/AI/cl_tagger/app.py 的推理部分，仅保留模型加载与单图推理，
供本推理服务以 HTTP 方式复用。模型文件仍用原 cl_tagger 的 models 目录。
"""
import json
import threading
from pathlib import Path

import numpy as np
from PIL import Image

from .. import config


class TaggerModel:
    """SIGLIP2（ONNX）打标模型。懒加载 + 线程锁，显存安全。"""

    def __init__(self):
        self._lock = threading.Lock()
        self._session = None
        self._processor = None
        self._idx_to_tag = {}
        self._is_naflex = False
        self._load_error = None

    # ---------- 加载 ----------
    def load(self) -> None:
        with self._lock:
            if self._session is not None:
                return
            try:
                import onnxruntime as ort
                from transformers import AutoProcessor

                paths = config.tagger_paths()
                for name, p in paths.items():
                    if not p.exists():
                        raise FileNotFoundError(f"打标模型文件缺失: {p}")

                # processor 仓库名：优先 metadata，缺省用 cl_tagger 默认
                meta = {}
                try:
                    with open(paths["meta"], "r", encoding="utf-8") as f:
                        meta = json.load(f)
                except Exception:
                    pass
                self._is_naflex = bool(meta.get("is_naflex", False))
                processor_repo = meta.get(
                    "vision_encoder_repo", "google/siglip2-so400m-patch14-384"
                )

                # 优先离线加载（本地 HF 缓存），失败再走联网
                try:
                    self._processor = AutoProcessor.from_pretrained(
                        processor_repo, local_files_only=True
                    )
                except Exception:
                    self._processor = AutoProcessor.from_pretrained(processor_repo)

                # 词表 idx -> tag
                with open(paths["vocab"], "r", encoding="utf-8") as f:
                    vocab_data = json.load(f)

                def vocab_get(key):
                    if key in vocab_data:
                        return vocab_data[key]
                    suffix = f"/{key}"
                    for k, v in vocab_data.items():
                        if isinstance(k, str) and k.endswith(suffix):
                            return v
                    return None

                raw = vocab_get("idx_to_tag")
                if raw is None:
                    tag_to_idx = vocab_get("tag_to_idx")
                    if tag_to_idx is None:
                        raise ValueError("词表缺少 idx_to_tag / tag_to_idx")
                    self._idx_to_tag = {int(i): t for t, i in tag_to_idx.items()}
                else:
                    self._idx_to_tag = {int(k): v for k, v in raw.items()}

                providers = ["CUDAExecutionProvider", "CPUExecutionProvider"]
                self._session = ort.InferenceSession(str(paths["onnx"]), providers=providers)
                self._load_error = None
            except Exception as e:  # noqa: BLE001
                self._load_error = f"{type(e).__name__}: {e}"
                raise

    @property
    def loaded(self) -> bool:
        return self._session is not None

    @property
    def load_error(self):
        return self._load_error

    # ---------- 推理 ----------
    def _preprocess(self, image: Image.Image):
        if self._is_naflex:
            inputs = self._processor(
                images=image, return_tensors="np", max_num_patches=256
            )
            return {
                "pixel_values": inputs["pixel_values"],
                "pixel_attention_mask": inputs["pixel_attention_mask"],
                "spatial_shapes": inputs["spatial_shapes"],
            }
        inputs = self._processor(images=image, return_tensors="np")
        return {"pixel_values": inputs["pixel_values"]}

    def infer(self, image_path, threshold=None) -> dict:
        """返回 {tag: confidence}，按置信度降序。"""
        if threshold is None:
            threshold = config.TAGGER_DEFAULT_THRESHOLD
        self.load()
        image = Image.open(image_path).convert("RGB")
        inputs = self._preprocess(image)
        outputs = self._session.run(["logits"], inputs)
        logits = outputs[0][0]
        probs = 1.0 / (1.0 + np.exp(-logits))
        result = {}
        for idx, prob in enumerate(probs):
            if prob >= threshold:
                tag = self._idx_to_tag.get(int(idx), f"class_{idx}")
                result[tag] = float(prob)
        return dict(sorted(result.items(), key=lambda kv: kv[1], reverse=True))
