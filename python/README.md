# 推理服务（Python）

承载打标（cl_tagger 抽取）与美学评分（Q-Align SIGLIP2 蒸馏版）两个本地模型，
通过 HTTP 供 Rust 主服务调用。接口契约见 `docs/TECH_DETAILS.md` 第 4 节。

## 目录

```
python/
├── server/
│   ├── config.py            # 配置（环境变量可覆盖）
│   ├── main.py              # FastAPI 入口（/health /infer/tags /infer/aesthetic ...）
│   └── models/
│       ├── tagger_model.py  # cl_tagger 核心推理抽取（ONNX，不含 Gradio）
│       └── aesthetic_model.py
├── requirements.txt
├── run_server.bat           # Windows 启动脚本
└── README.md
```

## 启动

直接运行 `run_server.bat`，或手动：

```bash
# 复用现有 cl_tagger 的 .venv（有 fastapi/transformers/onnxruntime，缺 torch → 美学会报错）
D:/Game/AI/cl_tagger/.venv/Scripts/python.exe -m uvicorn server.main:app --port 8001

# 或自建独立 venv（推荐，全功能）
python -m venv .venv
.venv/Scripts/pip install -r requirements.txt
.venv/Scripts/python -m uvicorn server.main:app --port 8001
```

## 配置（环境变量）

| 变量 | 默认 | 说明 |
|---|---|---|
| `INFER_HOST` / `INFER_PORT` | `127.0.0.1` / `8001` | 监听地址 |
| `TAGGER_MODEL_DIR` | `D:/Game/AI/cl_tagger/models` | cl_tagger 模型目录（model.onnx + vocabulary） |
| `TAGGER_DEFAULT_THRESHOLD` | `0.5` | 打标置信度阈值 |
| `AESTHETIC_MODEL` | `trojblue/distill-q-align-aesthetic-siglip2-base` | 美学模型（HF 仓库名或本地目录） |
| `AESTHETIC_SIGMOID` | `0` | `1` 时评分改为 `sigmoid(logit)*4+1`（实测后按需切换） |

> 美学模型基座为 `google/siglip2-base-patch16-512`，首次运行会联网下载（模型 + processor，约数百 MB）。已下载到本地后可用本地目录路径替代仓库名离线加载。

## 快速验证

```bash
curl http://127.0.0.1:8001/health
curl -X POST http://127.0.0.1:8001/infer/tags -H "Content-Type: application/json" -d "{\"path\":\"D:/path/to/test.png\"}"
curl -X POST http://127.0.0.1:8001/infer/aesthetic -H "Content-Type: application/json" -d "{\"path\":\"D:/path/to/test.png\"}"
```

## 已知限制（骨架阶段）

- 打标与美学共用显存，服务内单锁串行（批量接口内部逐张）；后续可改为独立推理队列。
- 美学模型输出范围待实测（当前默认直接取 logit 并 clamp 到 [1,5]）。
- 模型加载失败时 `/health` 返回 `failed` 与原因，主服务据此降级。
