# BeiWay-MoeVault

本地优先的大型图片管理软件：自动查重（含模糊/清晰对判定）、SauceNAO 溯源打标 + 本地模型回退、Q-Align 美学评分、按日期/标签/质量组合筛选排序、后台任务中心、多选批量操作。

> **本项目由 Vibe Coding 驱动**：全程通过 AI 结对编程完成（自然语言描述需求 → AI 生成/修改代码 → 自动测试验证 → 自动构建打包 → QQ 机器人通知），从骨架到全部功能迭代均在 AI 辅助下开发。开发者通过中文需求驱动开发，代码、构建、测试、打包全链路自动化。

## 产品功能

### 图库管理
- **导入索引**：目录扫描 + 移动进库（MD5 哈希分片存储），自动生成 WebP 缩略图、计算清晰度、读取 EXIF
- **多视图浏览**：网格 / 瀑布流 / 列表三种视图，视图模式与浏览位置自动记忆（重启后还原）
- **组合筛选**：文件名关键字、标签（AND/排除）、日期、美学分/清晰度范围、来源、格式、尺寸、冗余候选、AI 生成显示
- **分页模式**：可选游标分页（25/50/75/100 每页，照顾低配置 PC）

### 智能查重（dedup）
- pHash 感知哈希聚类（增量 + 全量），模糊/清晰对自动判定最优保留
- 查重管理页：只看含冗余的组，一键"保留最优、其余入回收站"

### 打标与溯源
- **SauceNAO 溯源** → danbooru/gelbooru 标签爬取 → 本地 cl_tagger 模型回退（三级流水线）
- **多 API key 调度**：round-robin 轮换 + 冷却容错延时 + 短/日配额追踪 + 日配额预警停用；配额状态持久化，重启恢复；密钥管理支持手动改当日额度/状态/删除
- **标签分类显示**：仿 danbooru 分类 —— 画师（红）/ 系列（紫）/ 角色（金）/ 常规（蓝）
- AI 生成图片自动跳过打标与溯源（prompt 标签更准确）

### 美学评分
- 本地 Q-Align（SIGLIP2）模型批量评分，图片角标显示星级

### 任务中心
- 打标/美学/溯源/检测 AI 全部后台任务化：提交即顶部提示"已添加任务"，完成/失败再通知
- 历史任务持久化，支持查看详情（错误信息、请求负载、各 API key 消耗的额度）、清空历史

### 多选批量操作
- 画廊/搜索多选模式：批量删除 / 打标 / 美学 / 溯源 / 检测 AI
- 批量打标/溯源/检测 AI 自动跳过带 AI 生成标签的图片

### 图片详情
- 大图查看 + 点击进入全屏（ESC/叉号退出、左右键切换）
- 原图链接显示与手动编辑、AI 元信息读取（PNG tEXt）、手动标记/取消 AI
- 图片状态一目了然：已溯源/未溯源、已打标/无需打标/未打标、冗余候选、AI 生成

### 桌面体验
- Tauri 2 桌面壳：启动自动拉起后端，打包 MSI / NSIS 安装包
- 浏览器也可直接访问 `http://127.0.0.1:9178` 使用完整功能
- 暗黑/亮色主题切换、导入拖拽

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
- [x] M6 搜索筛选：`/api/v1/images` 组合筛选（标签 AND/排除、关键字、日期、美学/清晰度范围、来源、格式、尺寸、冗余候选）+ 排序（imported/date/aesthetic/clarity/size/random）；前端图库/搜索页接真实 API + 缩略图显示；`cargo test` 34 过 / `clippy` 0 警告
- [x] M8 Tauri 桌面壳：窗口加载后端 URL（启动时自动拉起后端 sidecar）；构建产出 MSI + NSIS 安装包
- [x] 设置增强：多 SauceNAO key 管理（名称/等级/配额查看/删除）、暗黑主题切换、打标模型选择（运行时切换模型目录）、总览真实统计（含平均美学分/本月导入）
- [ ] 标签管理 API（自定义标签/合并/黑名单）

## 启动

```bash
# 1) 主后端（Rust，需已安装 rustup 工具链）
cd backend
cargo run                      # 默认 http://127.0.0.1:9178
# 环境变量：MOEVAULT_PORT / MOEVAULT_DATA_DIR / MOEVAULT_DB_PATH /
#           MOEVAULT_STATIC_DIR（前端 dist 目录）/ MOEVAULT_INFER_BASE
curl http://127.0.0.1:9178/health

# 2) 推理服务（需 Python 3.10+，推荐用 setup.bat 自动建 .venv）
cd python
setup.bat                     # 首次：一键创建 .venv 并安装全部依赖（打标+美学）
run_server.bat                # 启动（优先项目内 .venv，无硬编码外部路径）
# 或手动: .venv\Scripts\python.exe -m uvicorn server.main:app --port 8001
curl http://127.0.0.1:8001/health

# 3) 前端开发服务器（开发期用，后端已托管 dist 时不需要）
cd frontend && npm install && npm run dev
# 打开 http://localhost:5173
```

> **端口**：主服务默认 `9178`（M1 曾用 8000 因本机 node.exe 占用弃用）。
> npm 全局缓存目录 `D:\Node\cache` 权限异常，安装依赖时需指定缓存：
> `npm install --cache "$PWD/.npm-cache"`

## 桌面壳（Tauri）

```bash
# 构建安装包（需 rustup + tauri-cli：cargo install tauri-cli --version ^2）
cargo tauri build
# 产出：
#   src-tauri/target/release/bundle/msi/BeiWay-MoeVault_0.1.0_x64_en-US.msi
#   src-tauri/target/release/bundle/nsis/BeiWay-MoeVault_0.1.0_x64-setup.exe
```

- 桌面壳启动时自动拉起后端 `moevault-app.exe`（sidecar）与推理服务（Python uvicorn，端口 8001），窗口加载 `http://127.0.0.1:9178`
- 推理服务由桌面壳托管生命周期：应用退出自动停止；8001 已有外部实例时跳过拉起
- 推理服务若因缺 Python/依赖启动失败，顶栏圆点显示「未启动」，设置页「本地推理」可一键启动/停止（仅桌面版），或手动运行 `python/run_server.bat`
- 后端数据目录：生产模式为 exe 所在目录（安装后为安装目录），后续可改为用户数据目录

## 模型目录（自动探测，不再绑定本机路径）

打标（cl_tagger）与美学模型目录按以下优先级自动探测，可在设置页「本地推理」查看当前生效路径：

1. 环境变量显式指定：`TAGGER_MODEL_DIR` / `AESTHETIC_MODEL`
2. 项目内目录：`models/tagger/`、`models/aesthetic/`（推荐，git 已忽略）
3. 旧位置兼容回退：`D:/Game/AI/cl_tagger/models`（已配置旧环境无需迁移；挪动后可把模型复制到 `models/tagger/`）
4. 设置页「自定义目录」写入后，重跑打标任务时运行时热切换（`/infer/tagger/config`）

> 留空自动探测时后端不会强制切换模型目录，推理服务保持自动探测结果。

## 关键外部依赖（本地）

- cl_tagger 模型：`models/tagger/`（自动探测；原 `D:/Game/AI/cl_tagger/models` 兼容回退）——SIGLIP2 ONNX 打标
- 美学模型：`models/aesthetic/`（自动探测）；未放置时回退 HF 仓库 `trojblue/distill-q-align-aesthetic-siglip2-base`（首次运行联网下载；**需 torch + torchvision**，setup.bat 已含）
- SauceNAO API key：settings 表 `saucenao_api_keys`（**逗号分隔多 key**，兼容旧 `saucenao_api_key`）；配额状态自动持久化到 `data/sauce_keys.json`，重启恢复
