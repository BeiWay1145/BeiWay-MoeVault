# BeiWay-MoeVault

本地优先的大型图片管理软件：自动查重（含模糊/清晰对判定）、SauceNAO 溯源打标 + 本地模型回退、Q-Align 美学评分、按日期/标签/质量组合筛选排序。

## 文档

| 文档 | 内容 |
|---|---|
| [docs/PLAN.md](docs/PLAN.md) | 总体规划：决策清单、功能规格、架构、数据模型、路线图、风险 |
| [docs/UI_DESIGN.md](docs/UI_DESIGN.md) | UI 设计：布局、页面线框、组件树、交互流程、状态管理 |
| [docs/TECH_DETAILS.md](docs/TECH_DETAILS.md) | 技术细节：DDL、REST/WS 接口契约、推理服务契约、Rust 模块边界 |

## 技术栈

- **前端**：Vue 3 + Vite + TypeScript + Element Plus + Pinia（`frontend/`）
- **主后端**：Rust（axum + SQLite，workspace 见 `backend/`）
- **推理服务**：Python FastAPI（`python/`），承载 cl_tagger 打标 + Q-Align 美学评分

## 当前状态（M1 骨架完成）

- [x] 规划文档（3 份）
- [x] 前端骨架：9 个路由页面、布局、图片墙组件、mock 数据，`npm run build` 通过
- [x] 推理服务骨架：`/health`、`/infer/tags`、`/infer/aesthetic`（含批量），已验证可启动
- [x] Rust 主后端（M1）：workspace（core/db/api/app）+ SQLite 迁移 + `/health` `/api/v1/images` `/api/v1/stats` `/ws` + 前端静态托管，`cargo test`/`clippy` 通过
- [ ] 导入/查重/打标/评分流水线（M2–M5）
- [ ] Tauri 桌面壳（M8）

## 启动

```bash
# 1) 主后端（Rust，需已安装 rustup 工具链）
cd backend
cargo run                      # 默认 http://127.0.0.1:8000
# 环境变量：MOEVAULT_PORT / MOEVAULT_DATA_DIR / MOEVAULT_DB_PATH /
#           MOEVAULT_STATIC_DIR（前端 dist 目录）/ MOEVAULT_INFER_BASE
curl http://127.0.0.1:8000/health

# 2) 推理服务（需 Python 3.10+，推荐用 cl_tagger 的 .venv 或自建 .venv）
cd python && run_server.bat    # 或 python -m uvicorn server.main:app --port 8001
curl http://127.0.0.1:8001/health

# 3) 前端开发服务器（开发期用，后端已托管 dist 时不需要）
cd frontend && npm install && npm run dev
# 打开 http://localhost:5173
```

> **端口注意**：本机 8000 端口曾被一个 node.exe 进程占用（os error 10048），
> 若启动后端报端口被占，用 `MOEVAULT_PORT=<其他端口>` 覆盖，并同步改
> `frontend/vite.config.ts` 的 proxy 目标。
> npm 全局缓存目录 `D:\Node\cache` 权限异常，安装依赖时需指定缓存：
> `npm install --cache "$PWD/.npm-cache"`

## 关键外部依赖（本地）

- cl_tagger 模型：`D:/Game/AI/cl_tagger/models`（SIGLIP2 ONNX 打标）
- 美学模型：`trojblue/distill-q-align-aesthetic-siglip2-base`（首次运行联网下载）
- SauceNAO API key（设置页配置，溯源功能用）
