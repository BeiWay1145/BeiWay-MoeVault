# -*- coding: utf-8 -*-
"""打标模型推理核心（多探测器抽象）。

支持两种本地探测器：
- cl_tagger（SIGLIP2 ONNX + model_vocabulary.json）：源自 D:/Game/AI/cl_tagger/app.py 推理部分
- wd14（wd14 tagger ONNX + selected_tags.csv）：SmilingWolf 系列标准形态

架构说明：
- 探测器按「模型种类」分派加载/推理（cl_tagger / wd14）
- 后续新增种类（如 wd14 HF torch 版、DeepDanbooru）只需实现对应 _load_*/_infer_* 并登记
- 懒加载 + 线程锁（显存安全）；支持运行时切换模型目录与种类（load_from_dir）
"""
import json
import threading
from pathlib import Path

import numpy as np
from PIL import Image

from .. import config


class TaggerModel:
    """打标模型。懒加载 + 线程锁，显存安全。"""

    def __init__(self):
        self._lock = threading.Lock()
        self._session = None
        self._processor = None
        self._idx_to_tag = {}
        self._is_naflex = False
        self._kind = config.MODEL_KIND_AUTO  # 当前生效种类（加载后确定）
        self._requested_kind = config.MODEL_KIND_AUTO  # 用户指定/自动
        self._load_error = None
        self._model_dir = None
        self._device = "auto"  # auto / cuda / cpu

    # ---------- 加载 ----------
    def load(self) -> None:
        """使用配置默认目录加载（懒加载）。"""
        with self._lock:
            if self._session is not None:
                return
            self._do_load(config.TAGGER_MODEL_DIR, config.TAGGER_MODEL_KIND)

    def load_from_dir(self, model_dir, kind=None, device="auto") -> None:
        """切换并重新加载指定模型目录（线程安全，先清空旧模型释放显存）。

        kind：cl_tagger / wd14 / auto（None=auto，按目录内容判定）
        """
        with self._lock:
            self._session = None
            self._processor = None
            self._idx_to_tag = {}
            self._is_naflex = False
            self._load_error = None
            self._kind = config.MODEL_KIND_AUTO
            self._requested_kind = kind or config.MODEL_KIND_AUTO
            self._device = device
            self._do_load(model_dir, self._requested_kind)

    @property
    def model_dir(self):
        return self._model_dir

    @property
    def kind(self):
        return self._kind

    def _do_load(self, model_dir, kind=None) -> None:
        model_dir = str(model_dir)
        try:
            # 种类判定：显式指定优先，否则按目录内容
            resolved = kind or config.MODEL_KIND_AUTO
            if resolved == config.MODEL_KIND_AUTO:
                resolved = config.detect_kind(model_dir)
            if resolved == config.MODEL_KIND_AUTO:
                raise ValueError(
                    f"无法判定模型种类（目录缺失已知标志文件）: {model_dir}。"
                    "cl_tagger 需 model_vocabulary.json；wd14 需 selected_tags.csv"
                )

            if resolved == config.MODEL_KIND_WD14:
                self._load_wd14(model_dir)
            elif resolved == config.MODEL_KIND_CL_TAGGER:
                self._load_cl_tagger(model_dir)
            else:
                raise ValueError(f"未知模型种类: {resolved}")
            self._kind = resolved
            self._model_dir = model_dir
            self._load_error = None
        except Exception as e:  # noqa: BLE001
            self._load_error = f"{type(e).__name__}: {e}"
            raise

    # ---------- cl_tagger（SIGLIP2 ONNX） ----------
    def _load_cl_tagger(self, model_dir) -> None:
        import onnxruntime as ort
        from transformers import AutoProcessor

        paths = {
            "onnx": Path(model_dir) / "model.onnx",
            "vocab": Path(model_dir) / "model_vocabulary.json",
            "meta": Path(model_dir) / "model_metadata.json",
        }
        for name, p in paths.items():
            if not p.exists():
                raise FileNotFoundError(f"cl_tagger 模型文件缺失: {p}")

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

        providers = self._resolve_providers()
        self._session = ort.InferenceSession(str(paths["onnx"]), providers=providers)

    # ---------- wd14（ONNX + selected_tags.csv） ----------
    def _load_wd14(self, model_dir) -> None:
        import csv

        import onnxruntime as ort

        paths = {
            "onnx": Path(model_dir) / "model.onnx",
            "tags": Path(model_dir) / "selected_tags.csv",
        }
        for name, p in paths.items():
            if not p.exists():
                raise FileNotFoundError(f"wd14 模型文件缺失: {p}")

        # 词表：selected_tags.csv，首列 tag 名（首行为表头 name/category）
        tags: list[str] = []
        with open(paths["tags"], "r", encoding="utf-8") as f:
            reader = csv.reader(f)
            for i, row in enumerate(reader):
                if i == 0:
                    continue  # 表头
                if row and row[0].strip():
                    tags.append(row[0].strip())
        if not tags:
            raise ValueError("selected_tags.csv 为空或无有效标签")
        self._idx_to_tag = {int(i): t for i, t in enumerate(tags)}

        providers = self._resolve_providers()
        self._session = ort.InferenceSession(str(paths["onnx"]), providers=providers)

    # ---------- 公共 ----------
    def _resolve_providers(self):
        import onnxruntime as ort

        device = self._device
        if device == "auto":
            device = "cuda" if "CUDAExecutionProvider" in ort.get_available_providers() else "cpu"
        return (
            ["CUDAExecutionProvider", "CPUExecutionProvider"]
            if device == "cuda"
            else ["CPUExecutionProvider"]
        )

    @property
    def loaded(self) -> bool:
        return self._session is not None

    @property
    def load_error(self):
        return self._load_error

    # ---------- 推理 ----------
    def _preprocess_cl_tagger(self, image: Image.Image):
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

    @staticmethod
    def _preprocess_wd14(image: Image.Image, size: int = 448):
        """wd14 标准预处理：resize 到 448x448 + CLIP 归一化。"""
        mean = np.array([0.48145466, 0.4578275, 0.40821073], dtype=np.float32)
        std = np.array([0.26862954, 0.26130258, 0.27577711], dtype=np.float32)
        img = image.resize((size, size), Image.BILINEAR)
        arr = np.asarray(img, dtype=np.float32) / 255.0
        arr = (arr - mean) / std
        # NCHW
        arr = arr.transpose(2, 0, 1)[None]
        return {"input": arr}

    def infer(self, image_path, threshold=None, kind=None) -> dict:
        """返回 {tag: confidence}，按置信度降序。kind 与当前加载一致（默认 None 用当前）。"""
        self.load()
        image = Image.open(image_path).convert("RGB")

        if self._kind == config.MODEL_KIND_WD14:
            # wd14 默认阈值更低（0.35）；调用方显式传 threshold 时不覆盖
            if threshold is None:
                threshold = config.TAGGER_WD14_THRESHOLD
            inputs = self._preprocess_wd14(image)
        else:
            if threshold is None:
                threshold = config.TAGGER_DEFAULT_THRESHOLD
            inputs = self._preprocess_cl_tagger(image)

        outputs = self._session.run(["logits"], inputs)
        logits = outputs[0][0]
        probs = 1.0 / (1.0 + np.exp(-logits))
        result = {}
        for idx, prob in enumerate(probs):
            if prob >= threshold:
                tag = self._idx_to_tag.get(int(idx), f"class_{idx}")
                result[tag] = float(prob)
        return dict(sorted(result.items(), key=lambda kv: kv[1], reverse=True))