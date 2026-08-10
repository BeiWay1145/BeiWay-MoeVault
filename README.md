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

## 当前状态（M3 查重完成）

- [x] 规划文档（3 份）
- [x] 前端骨架：9 个路由页面、布局、图片墙组件、mock 数据，`npm run build` 通过
- [x] 推理服务骨架：`/health`、`/infer/tags`、`/infer/aesthetic`（含批量），已验证可启动
- [x] Rust 主后端（M1）：workspace（core/db/api/app）+ SQLite 迁移 + REST/WS + 前端静态托管
- [x] M2 导入与索引：扫描/移动进库（哈希分片）/MD5/pHash/清晰度/EXIF/WebP 缩略图/批量入库/去重计数，`POST /api/v1/import` + 批次查询 + WS `batch.done` 广播
- [x] M3 查重与回收站：pHash 聚类（增量+全量）、模糊/清晰对判定（best/redundant）、查重管理 API（stats/groups/scan/resolve）、回收站（recycle/restore/purge）、缩略图静态托管 `/thumbs/*`、前端查重页/回收站页接真实 API；`cargo test` 22 过 / `clippy` 0 警告
- [x] M4 打标流水线：SauceNAO 溯源 → danbooru/gelbooru 标签爬取 → 本地 cl_tagger 回退；`/api/v1/tagging/*`（run/stats/keys）+ `/images/{id}/tags|retag`
- [x] 多 API key 轮换调度：`ApiKeyPool`（round-robin 轮换 + 冷却容错延时 + 短/日配额追踪 + 日配额预警<10 停用）、配额状态 JSON 持久化（`data/sauce_keys.json`，重启恢复）、`GET /api/v1/tagging/keys` 查看状态
- [x] 不可溯源标记：`images.no_auto_sauce`（相似度不足或无结果自动标记）、自动打标跳过、`retag` 手动强制；`cargo test` 33 过 / `clippy` 0 警告
- [x] M5 美学评分流水线：本地 Q-Align（trojblue/distill-q-align-aesthetic-siglip2-base）批量评分写库；`/api/v1/aesthetic/run|stats` + `/images/{id}/rescore`；`cargo test` 33 过 / `clippy` 0 警告
- [ ] 搜索筛选（组合筛选/排序 API + 前端）
- [ ] Tauri 桌面壳（M8）

## 启动

```bash
# 1) 主后端（Rust，需已安装 rustup 工具链）
cd backend
cargo run                      # 默认 http://127.0.0.1:9178
# 环境变量：MOEVAULT_PORT / MOEVAULT_DATA_DIR / MOEVAULT_DB_PATH /
#           MOEVAULT_STATIC_DIR（前端 dist 目录）/ MOEVAULT_INFER_BASE
curl http://127.0.0.1:9178/health

# 2) 推理服务（需 Python 3.10+，推荐用 cl_tagger 的 .venv 或自建 .venv）
cd python && run_server.bat    # 或 python -m uvicorn server.main:app --port 8001
curl http://127.0.0.1:8001/health

# 3) 前端开发服务器（开发期用，后端已托管 dist 时不需要）
cd frontend && npm install && npm run dev
# 打开 http://localhost:5173
```

> **端口**：主服务默认 `9178`（M1 曾用 8000 因本机 node.exe 占用弃用）。
> npm 全局缓存目录 `D:\Node\cache` 权限异常，安装依赖时需指定缓存：
> `npm install --cache "$PWD/.npm-cache"`

## 关键外部依赖（本地）

- cl_tagger 模型：`D:/Game/AI/cl_tagger/models`（SIGLIP2 ONNX 打标）
- 美学模型：`trojblue/distill-q-align-aesthetic-siglip2-base`（首次运行联网下载；**需 torch + torchvision**，已装于 cl_tagger venv 的 CPU 版）
- SauceNAO API key：settings 表 `saucenao_api_keys`（**逗号分隔多 key**，兼容旧 `saucenao_api_key`）；配额状态自动持久化到 `data/sauce_keys.json`，重启恢复
