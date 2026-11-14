# BeiWay-MoeVault

本地优先的大型图片管理软件：自动查重（含模糊/清晰对判定）、SauceNAO 溯源打标 + 本地模型回退、Q-Align 美学评分、按日期/标签/质量组合筛选排序、后台任务中心、多选批量操作。

> **本项目由 Vibe Coding 驱动**：全程通过 AI 结对编程完成（自然语言描述需求 → AI 生成/修改代码 → 自动测试验证 → 自动构建打包 → QQ 机器人通知），从骨架到全部功能迭代均在 AI 辅助下开发。开发者通过中文需求驱动开发，代码、构建、测试、打包全链路自动化。

## 产品功能

### 图库管理
- **导入索引**：目录扫描 + 移动/复制进库（MD5 哈希分片存储），自动生成 WebP 缩略图、计算清晰度、读取 EXIF
- **桌面壳拖入导入**：文件/文件夹从资源管理器直接拖入窗口 → 确认框（路径预览 + 移动/复制）→ 后台导入
- **多视图浏览**：网格 / 瀑布流 / 列表三种视图，视图模式与浏览位置自动记忆（重启后还原）
- **搜索式筛选（danbooru 风格）**：工具栏搜索框联想（标签/中文名/别名/状态 + 词频），选中即色块 chips 即时筛选；逗号连续多标签；标签分类着色
- **组合筛选**：文件名关键字、标签（AND/排除/归一化）、日期、美学分/清晰度范围、来源、格式、尺寸、冗余候选、AI 生成、打标状态
- **分页模式**：图库/标签页各自独立每页条数（localStorage 持久化）

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
- 画廊多选模式（含全选，选中集随筛选自动收缩）：批量删除 / 打标 / 美学 / 溯源 / 检测 AI（下拉框多选 + 两击确认执行）
- 批量打标/溯源/检测 AI 自动跳过带 AI 生成标签的图片

### 图片详情
- 大图查看 + 点击进入全屏（ESC/叉号退出、左右键切换）
- **左侧标签栏**（danbooru 风格）：画师/系列/角色/常规分组、词频、中文名、点击跳转该标签筛选、一键复制（danbooru 文本格式）、修改标签二级窗口（含修改原始标签文本三级窗口）
- **左右栏整栏收起/展开**（宽度过渡动画 + 状态持久化），放大图片区
- 浏览上下文导航：上/下一张在来源目录组/筛选结果内循环
- **对比原图**：支持 danbooru/gelbooru/yande.re/konachan/lolibooru 解析 + 后端代理加载网络原图
- 原图链接显示与手动编辑、AI 元信息读取（PNG tEXt）、手动标记/取消 AI
- 图片状态一目了然：已溯源/未溯源、已打标/未打标、冗余候选、AI 生成

### 标签管理
- 标签列表：分类筛选、未设中文别名筛选、独立分页（50/100/200/500）
- **多中文别名**：一个标签多条中文别名（参与搜索联想）、主别名编辑、中文字典一键导入（ffdfkj Danbooru 对照表）

### 桌面体验
- Tauri 2 桌面壳：启动自动拉起后端与推理服务，打包 MSI / NSIS 安装包
- 浏览器也可直接访问 `http://127.0.0.1:9178` 使用完整功能
- 暗黑/亮色主题切换、导入拖拽、侧边栏悬停展开

### BUG 追踪器
- 三态（关闭/本会话开启/开启）全量追踪：API 请求（脱敏/耗时/状态码/错误体）+ 前端错误堆栈 + 关键操作
- 应用退出自动转储为 txt（`data/logs/bug_tracker_*.txt`）

## 文档

**现行文档**（与当前实现一致，优先看这几份）：

| 文档 | 内容 |
|---|---|
| [docs/ARCH_REVIEW_2026-11.md](docs/ARCH_REVIEW_2026-11.md) | 架构审查（2026-11）：模块边界、重复代码、超长文件/函数、错误处理、前端状态、性能隐患、测试覆盖、P0–P3 分级与修复顺序 |
| [docs/RUST_INFERENCE_FEASIBILITY.md](docs/RUST_INFERENCE_FEASIBILITY.md) | 打标/美学推理从 Python 迁到 Rust 的可行性检验：ort / candle / 混合三条路线、GPU 与部署复杂度、数值一致性验证方案（**注意：其中 R3「ort 需 CUDA 13.2 / cuDNN 9.23」只对 ort rc.11+ 成立，打标部分的更正见下一行**） |
| [docs/RUST_TAGGER_REWRITE_EVAL_2026-11.md](docs/RUST_TAGGER_REWRITE_EVAL_2026-11.md) | 打标环节单独做 Rust 重写的实测评估：已在 `ort rc.10` 跑通（Jaccard 1.0000、76.8 ms/张）、R3 更正（只需 CUDA 12 + cuDNN 9，可复用 torch/lib 零体积）、体积/吞吐三口径收益、以及「不建议做」的结论与替代方案（并发化 / 锁细化 / 打标与溯源解耦） |
| [docs/EXPLORER_STYLE_SELECTION_PLAN.md](docs/EXPLORER_STYLE_SELECTION_PLAN.md) | 图库资源管理器式交互（选择/框选/拖出/右键菜单）的方案与架构建议 |
| [docs/MULTI_EXIT_IP_PLAN.md](docs/MULTI_EXIT_IP_PLAN.md) | SauceNAO 多出口 IP 轮换（Clash 多入站端口）方案 |
| [docs/DESIGN_FILTER_NAVIGATION.md](docs/DESIGN_FILTER_NAVIGATION.md) | 搜索式筛选 v2 设计 |
| [docs/SKILL_AUDIT_2026-11.md](docs/SKILL_AUDIT_2026-11.md) | 开发会话所用 Skill 的时效性审计（2026-11） |
| [CHANGELOG.md](CHANGELOG.md) | 更新日志 |

**历史文档**（v0.1 规划期产物，保留作决策记录；架构/接口细节已有漂移，勿据此改代码）：

| 文档 | 内容 |
|---|---|
| [docs/PLAN.md](docs/PLAN.md) | 总体规划：决策清单、功能规格、架构、数据模型、路线图、风险 |
| [docs/UI_DESIGN.md](docs/UI_DESIGN.md) | UI 设计：布局、页面线框、组件树、交互流程、状态管理 |
| [docs/TECH_DETAILS.md](docs/TECH_DETAILS.md) | 技术细节：DDL、REST/WS 接口契约、推理服务契约、Rust 模块边界 |
| [docs/ENHANCEMENTS.md](docs/ENHANCEMENTS.md) | 增强项待办清单（多数已完成） |

## 技术栈

- **前端**：Vue 3 + Vite + TypeScript + Element Plus + Pinia（`frontend/`）
- **主后端**：Rust（axum + SQLite，workspace 见 `backend/`）
- **推理服务**：Python FastAPI（`python/`），承载 cl_tagger 打标 + Q-Align 美学评分

## 代码结构

Rust 后端是 7 个 crate 的 workspace（`backend/crates/`），依赖方向单向无循环：

```
core  ← db ← ingest
  ↑      ↑     ↑
  └── dedup    tagger        （dedup / ingest 依赖 core+db；tagger 依赖 core+db+ingest）
       ↑        ↑
       └── api ┘             （api 依赖全部业务 crate）
             ↑
            app              （app 只依赖 core/db/api，是 bin 入口）
```

| crate | 职责 |
|---|---|
| `core` | 领域模型、错误类型（ErrorKind/AppError）、常量 |
| `db` | SQLite 封装与迁移（单连接 + Mutex，WAL） |
| `ingest` | 目录扫描、导入、去重计数、缩略图、EXIF、清晰度、特征提取 |
| `dedup` | pHash 聚类、模糊/清晰对判定 |
| `tagger` | 打标/溯源流水线、SauceNAO 客户端、多 key 池、多出口 IP 池、推理客户端 |
| `api` | axum HTTP 层：17 个路由模块、AppState、WS 广播 |
| `app` | 可执行入口：读环境变量、建库、起 axum server、托管前端 dist |

前端 `frontend/`（Vue 3 + Vite + TS + Element Plus + Pinia），推理服务 `python/`（FastAPI），桌面壳 `src-tauri/`（Tauri 2，独立 workspace）。

## 当前状态（v0.1.x）

功能面已完整可用，迭代重心转向稳定性与可维护性：

- [x] M1–M8 全部里程碑（workspace / 导入索引 / 查重回收站 / 打标流水线 / 美学评分 / 搜索筛选 / 桌面壳）
- [x] 桌面壳拖入导入、搜索式筛选、标签多别名、中文字典缓存、BUG 追踪器
- [x] 图库资源管理器式交互：单选 / Ctrl 加选 / Shift 连选 / 框选 / 右键批量菜单 / 原生拖出 / 复制图片
- [x] SauceNAO 多 API key + 多出口 IP 轮换，配额与限流（429/403）识别
- [x] 任务中心：中断/继续、失败可见、WS 实时事件 + 轮询兜底
- [x] 内存与性能（待持续）：见 `docs/ARCH_REVIEW_2026-11.md` 的 P1/P2 清单
- [ ] 标签管理 API（自定义标签/合并/黑名单）

> 架构现状、已知技术债与修复优先级以 `docs/ARCH_REVIEW_2026-11.md` 为准 —— README 不再逐条维护里程碑勾选。

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
- 运行时数据目录按以下优先级探测（`src-tauri/src/lib.rs` 的 `runtime_root()`）：`MOEVAULT_RUNTIME_DIR` → 安装目录下的 `runtime/`（默认，可写校验通过才采用）→ `%LOCALAPPDATA%\BeiWay-MoeVault`

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
