# 推理服务（Python）

承载打标（cl_tagger 抽取）与美学评分（Q-Align SIGLIP2 蒸馏版）两个本地模型，
通过 HTTP 供 Rust 主服务调用。接口契约见 `docs/TECH_DETAILS.md` 第 4 节。

## 目录

```
python/
├── server/
│   ├── config.py            # 配置（环境变量可覆盖 + 模型路径自动探测）
│   ├── main.py              # FastAPI 入口（/health /infer/tags /infer/aesthetic ...）
│   └── models/
│       ├── tagger_model.py  # cl_tagger 核心推理抽取（ONNX，不含 Gradio）
│       └── aesthetic_model.py
├── requirements.txt
├── setup.bat                # 一键创建 .venv 并安装全部依赖（推荐首次运行）
├── run_server.bat           # 启动脚本（优先项目内 .venv）
└── README.md
```

## 首次搭建（推荐）

```bat
cd python
setup.bat
```

创建 `python/.venv` 并安装全部依赖（torch / transformers / onnxruntime 等，走清华镜像，失败自动回退官方 PyPI）。

## 启动

```bash
run_server.bat               # 优先 python/.venv，其次系统 python（py -3 / python）
# 或手动
.venv\Scripts\python.exe -m uvicorn server.main:app --port 8001
```

Python 解释器解析顺序：`python/.venv` → `py -3` → `python`。**不再依赖任何外部固定路径**（如 cl_tagger/.venv）。

## 模型路径自动探测（消除「依赖强绑定本机」）

| 优先级 | 打标模型 `TAGGER_MODEL_DIR` | 美学模型 `AESTHETIC_MODEL` |
|---|---|---|
| 1 | 环境变量 `TAGGER_MODEL_DIR` | 环境变量 `AESTHETIC_MODEL` |
| 2 | 项目根 `models/tagger/`（含 model.onnx + vocabulary） | 项目根 `models/aesthetic/`（含 config.json） |
| 3 | 旧位置 `D:/Game/AI/cl_tagger/models`（兼容迁移回退） | HF 仓库 `trojblue/distill-q-align-aesthetic-siglip2-base`（首次联网下载） |

`/health` 返回 `paths` 字段（`tagger_model_dir` / `aesthetic_model`），前端设置页据此展示当前生效路径。

## 配置（环境变量）

| 变量 | 默认 | 说明 |
|---|---|---|
| `INFER_HOST` / `INFER_PORT` | `127.0.0.1` / `8001` | 监听地址 |
| `TAGGER_MODEL_DIR` | 自动探测（见上表） | 打标模型目录（model.onnx + vocabulary） |
| `TAGGER_DEFAULT_THRESHOLD` | `0.5` | 打标置信度阈值 |
| `AESTHETIC_MODEL` | 自动探测（见上表） | 美学模型（HF 仓库名或本地目录） |
| `AESTHETIC_SIGMOID` | `0` | `1` 时评分改为 `sigmoid(logit)*4+1`（实测后按需切换） |

> 美学模型基座为 `google/siglip2-base-patch16-512`，未放置本地目录时首次运行会联网下载（模型 + processor，约数百 MB）。已下载到本地后可用本地目录路径替代仓库名离线加载。

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