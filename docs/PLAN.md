# BeiWay-MoeVault — 项目规划文档（v0.1）

> 状态：初期规划。本文档汇总已确认的架构决策与功能规格，后续开发以本文档为基准。
> 所有"默认值"均可通过软件设置界面调整。

---

## 0. 已确认决策清单

| 维度 | 决策 |
|---|---|
| 产品形态 | 本地 Web 应用 + 后期 Tauri 桌面壳 |
| 前端 | Vue 3 + Vite + Element Plus（Pinia / Vue Router） |
| 主后端 | Rust（axum + tokio + SQLite） |
| 推理服务 | 独立 Python 服务（FastAPI），承载打标 / 美学 / 图像分析 |
| 数据规模 | 10万 ~ 100万张 |
| 数据库 | SQLite（WAL + FTS5） |
| 文件管理 | 移动进库目录，按哈希分片组织；删除进软件内置回收站 |
| 查重粒度 | 完全重复（pHash）+ 同内容模糊/清晰对；近重复识别列为后期扩展 |
| 标签策略 | 英文存储 + 可选开源中文翻译字典显示；支持自定义标签/合并/黑名单 |
| 打标流水线 | SauceNAO 溯源（可配置限流）→ 命中 danbooru/gelbooru 链接则爬取标签 → 否则本地 cl_tagger 打标 |
| 美学评分 | 本地 distill-q-align-aesthetic-siglip2-base |
| 导入工作流 | 可配置，默认自动全流水线（查重→溯源→打标→评分→入库） |
| sidecar | 可选导出同名 .txt（兼容外部 AI 工具），默认关闭 |

---

## 1. 项目概述

本地优先的大型图片管理软件，面向个人图库（尤其二次元/插画收藏）。核心能力：

1. **自动查重**：感知哈希识别完全重复；同内容下自动判定模糊/清晰对，模糊图标记为冗余候选（最终删除永远由用户确认）。
2. **自动打标签**：SauceNAO 溯源优先（命中 danbooru/gelbooru 则爬取原图标签），溯源失败回退本地 SIGLIP2 打标模型。
3. **自动美学评分**：本地 Q-Align（SIGLIP2 蒸馏版）统一评分。
4. **分类 / 搜索 / 排序**：按日期、标签、质量分数组合筛选与排序。

质量目标：良好 UI、可维护性与可扩展性、稳定性、性能（百万级规模可用）。

---

## 2. 功能需求规格

### 2.1 图库管理
- **导入**：支持添加任意来源文件夹/文件（拖拽、路径、多选）。
  - 导入前先校验（格式支持 jpg/jpeg/png/webp/bmp/gif/avif 等）。
  - **移动进库**：文件移动进库目录（默认 `data/library/`），按哈希分片存放（`xx/xx/<md5前32>.<ext>`），移动成功后才删除源文件引用；移动失败回滚。
  - 断点续传：中断后可继续未完成的导入批次。
- **增量更新**：定期/手动重新扫描库目录，发现外部删除/变更时同步元数据。
- **库目录结构**：哈希分片（利于去重与不可变），人眼浏览依赖 UI 虚拟组织，不依赖物理目录。
- **导出**：图片导出（复制/移动出库）、元数据导出（JSON/CSV）。

### 2.2 自动查重
- **完全重复识别**：
  - 先按 **MD5** 精确分组（快，命中绝大多数完全一致文件）。
  - 再按 **pHash（64-bit DCT 感知哈希）** 汉明距离 ≤ 阈值（默认 8，可配置）聚类，捕获"内容相同、格式/分辨率/压缩率不同"的情况。
- **模糊/清晰对**：同一 pHash 簇内，按**清晰度分数**（Laplacian 方差）排序，保留清晰度最高者，其余标记为"冗余候选"。
- **删除决策**：候选组在 UI 中展示（对比视图），**由用户确认**后才进入回收站。支持组内一键"保留最优，其余入回收站"。
- 查重为幂等流程：导入后自动跑，也可手动对全库/选区重跑；结果增量更新（已有分组不重复计算）。

### 2.3 自动打标
- **流水线顺序**（每张图独立任务）：
  1. **SauceNAO 溯源**：上传图片（multipart）或 URL，解析响应。
  2. **有效判定**：相似度 ≥ 阈值（默认 75%，可配置）且 `ext_urls` 中存在 danbooru（`danbooru.donmai.us`）或 gelbooru 链接。
  3. **爬取标签**：优先走官方 API（danbooru `/posts/{id}.json`、gelbooru dapi），API 失败回退页面解析（scraper）。
  4. **溯源失败**（无有效链接/相似度过低）→ 调用本地 cl_tagger（SIGLIP2 ONNX）打标，置信度阈值默认 0.5（可配置）。
- **缓存**：同一图片（按内容哈希）的溯源结果缓存到数据库，避免重复请求与重复爬取。
- **限流**：SauceNAO 请求频率可配置（默认按免费 API 约 30s/次），任务队列自动排队、退避重试、断点续跑。
- **标签来源标记**：`danbooru` / `gelbooru` / `local_cl` / `manual`，UI 可筛选/回溯。

### 2.4 美学评分
- 本地模型 `distill-q-align-aesthetic-siglip2-base` 批量推理，输出美学分数。
- 输出格式需实测确认（Q-Align 系为 1–5 连续分）；存储原始分 + 可配置归一化展示。
- GPU 推理队列与打标共用，避免显存竞争。
- 支持全量/增量/选区重算。

### 2.5 分类 / 搜索 / 排序
- **筛选维度**（可任意组合）：
  - 日期：EXIF 拍摄日期优先，缺失回退文件修改时间（入库时落库）。
  - 标签：自动标签、手动标签、黑名单排除、AND/OR 组合。
  - 质量：美学分区间、清晰度区间。
  - 其他：来源（danbooru/gelbooru/本地）、文件格式、宽高、大小、是否冗余候选、是否在回收站。
- **搜索**：SQLite FTS5 全文索引标签名（含中文翻译字段）。
- **排序**：日期、美学分、清晰度、大小、导入时间、随机。
- 结果集保存为"智能筛选视图"，可固定收藏。

### 2.6 回收站
- 删除（查重确认 / 手动）→ 软删除：`images.status = recycled`，文件移入 `data/recycle/`，保留元数据与路径记录。
- UI 提供恢复 / 永久删除 / 清空回收站；永久删除前二次确认。
- 可选自动清空策略（按保留天数，默认关闭）。

### 2.7 标签管理
- 手动为单图/多图添加自定义标签。
- 标签重命名（自动同步到所有关联图）、合并、删除、黑白名单。
- 黑名单标签：默认隐藏/模糊相关图片（如 NSFW 过滤），可随时切换。
- 中文翻译字典：可开关（加载开源 danbooru 中文翻译表，仅影响显示，不改存储）。

### 2.8 sidecar 导出
- 可选：为选中图片生成同名 `.txt`（逗号分隔标签，与 cl_tagger 格式一致），供 ComfyUI / lora 训练等外部工具复用。
- 默认关闭；支持批量导出。

### 2.9 设置
- 库目录、回收站策略、SauceNAO（API key / 限流 / 相似度阈值）、打标阈值、查重阈值、清晰度阈值、美学模型路径、中文字典开关、sidecar 开关、缩略图规格、任务并发数等。

---

## 3. 技术架构

### 3.1 总体架构

```
┌─────────────────────────────┐
│  Browser (Vue3 + Element Plus)│  ← 本地 Web UI（后期 Tauri 壳包裹）
└──────────────┬──────────────┘
               │ HTTP(REST) + WebSocket/SSE（任务进度、事件推送）
┌──────────────▼──────────────┐
│      Rust 主服务 (axum)       │
│  ┌────────────────────────┐  │
│  │ 任务队列（持久化，SQLite） │  │
│  │ 流水线编排（导入/查重/打标/ │  │
│  │  美学/回收站/导出）        │  │
│  └────────────────────────┘  │
│  ┌──────────┬───────────────┐│
│  │ SQLite    │ 文件系统       ││
│  │ WAL+FTS5  │ library/      ││
│  │ 元数据/标签│ thumbs/       ││
│  │ 队列/设置  │ recycle/      ││
│  └──────────┴───────┬───────┘│
└─────────────────────┼────────┘
                      │ HTTP（127.0.0.1 内部端口）
┌─────────────────────▼────────┐
│   Python 推理服务 (FastAPI)   │
│  ├─ cl_tagger（SIGLIP2 ONNX 打标）│
│  ├─ 美学模型（Q-Align SIGLIP2）   │
│  └─ （扩展：OCR / NSFW / 向量特征）│
└──────────────────────────────┘
```

- 主服务与推理服务均仅监听 `127.0.0.1`（Web UI 同源或走主服务代理）。
- Rust 主服务持有全部业务逻辑；Python 服务只做"图 → 分数/标签"的纯推理，通过 JSON-RPC 风格 HTTP 接口交互。
- 前端静态资源由 Rust 主服务托管（生产），开发期走 Vite dev server 代理。

### 3.2 技术选型

| 层 | 选型 | 说明 |
|---|---|---|
| Rust 框架 | axum + tokio | 异步 HTTP/WS |
| DB | rusqlite（bundled）+ 版本化迁移（refinery 或 rusqlite_migration） | WAL、FTS5、预编译语句 |
| 图像处理 | image crate（feature: webp/avif） | 解码、缩放、生成缩略图、pHash 基础 |
| pHash/清晰度 | 自实现（DCT 64-bit pHash + Laplacian 方差），纯 Rust | 可控、无额外依赖 |
| HTTP 客户端 | reqwest | SauceNAO API / danbooru / gelbooru |
| HTML 解析 | scraper | 爬取回退路径 |
| 文件监听 | notify（Windows 上可靠性一般，配轮询兜底） | 增量扫描 |
| 并行 | rayon | 批量哈希/缩略图 |
| 日志 | tracing + tracing-subscriber | 结构化日志 |
| 序列化 | serde / serde_json | |
| 前端 | Vue 3 + Vite + TypeScript + Element Plus + Pinia + Vue Router | |
| 虚拟滚动 | vue-virtual-scroller | 万级缩略图流畅滚动 |
| 推理服务 | FastAPI + onnxruntime-gpu + Pillow + numpy | 复用 cl_tagger 核心逻辑（抽离 Gradio 依赖） |

> 备注：`cl_tagger` 现位于 `D:\Game\AI\cl_tagger`（SIGLIP2 系 ONNX 模型 + 现成推理函数 + 独立 `.venv`）。集成策略：把其**核心推理代码**抽取为可 import 的 Python 模块，供推理服务加载，不再依赖 Gradio；模型文件保持原位引用或复制进本项目 `python/models/`。

### 3.3 数据流（导入流水线）

```
导入源 → 扫描校验 → 移动进库(哈希分片) → 生成缩略图 → 计算 MD5 + pHash + 清晰度
      → 查重分组(增量) → 溯源任务排队(SauceNAO 限流) → 命中? 爬取标签 / 本地打标
      → 美学评分(推理队列) → 元数据落库 → WS 推送进度 → UI 更新
```

### 3.4 任务队列设计
- SQLite 持久化队列（`jobs` 表），状态机：`pending → running → done | failed(retry) | cancelled`。
- 失败重试：指数退避，最大次数可配置；`next_run_at` 支持断点续跑。
- 两个独立调度组：
  - **网络组**（SauceNAO/爬虫）：限流令牌桶，可配置速率。
  - **推理组**（打标/美学）：串行或按显存并发上限（默认 1，避免 OOM）。
- 优先级：用户手动触发 > 单张 > 批量后台。
- 全流程进度通过 WebSocket 推送（任务条数、当前项、失败列表）。

---

## 4. 数据模型（SQLite Schema 草案）

```
images
  id            INTEGER PK
  md5           TEXT  UNIQUE          -- 精确查重键
  phash         INTEGER               -- 64-bit DCT pHash
  rel_path      TEXT  UNIQUE          -- 相对库目录路径
  width, height INTEGER
  format        TEXT                  -- jpg/png/webp/...
  size_bytes    INTEGER
  file_mtime    INTEGER               -- 文件修改时间(epoch)
  exif_datetime INTEGER NULL          -- 拍摄日期，缺失为 NULL
  clarity_score REAL                  -- 清晰度（Laplacian 方差归一化）
  aesthetic_score REAL NULL           -- 美学分（Q-Align 输出）
  dedup_group   INTEGER NULL          -- 查重簇 id（冗余候选非 NULL）
  is_redundant  INTEGER DEFAULT 0     -- 簇内非最优 = 冗余候选
  status        TEXT DEFAULT 'active' -- active / recycled
  source        TEXT                  -- danbooru / gelbooru / local / manual
  source_url    TEXT NULL             -- 溯源到的原帖 URL
  thumb_path    TEXT                  -- 缩略图相对路径
  imported_at   INTEGER

tags
  id         INTEGER PK
  name       TEXT UNIQUE              -- 英文标签（danbooru 体系）
  name_cn    TEXT NULL                -- 可选中文翻译（仅显示）
  category   TEXT                     -- general/character/copyright/meta/custom
  is_custom  INTEGER DEFAULT 0
  is_blacklisted INTEGER DEFAULT 0

image_tags
  image_id   INTEGER
  tag_id     INTEGER
  source     TEXT    -- auto_danbooru/auto_gelbooru/auto_local/manual
  confidence REAL NULL                -- 本地打标置信度
  created_at INTEGER
  PRIMARY KEY (image_id, tag_id, source)

dedup_groups
  id           INTEGER PK
  phash_seed   INTEGER                -- 簇代表哈希
  best_image   INTEGER                -- 簇内清晰度最优 image_id
  state        TEXT                   -- open / resolved
  created_at   INTEGER

recycle_bin
  id          INTEGER PK
  image_id    INTEGER UNIQUE
  reason      TEXT    -- duplicate / manual / auto
  original_rel TEXT   -- 原相对路径（用于恢复）
  deleted_at  INTEGER

jobs
  id          INTEGER PK
  job_type    TEXT    -- ingest/dedup/sauce_nao/fetch_tags/aesthetic/...
  image_id    INTEGER NULL
  payload     TEXT    -- JSON
  status      TEXT    -- pending/running/done/failed/cancelled
  attempts    INTEGER DEFAULT 0
  next_run_at INTEGER
  error       TEXT NULL
  created_at  INTEGER
  updated_at  INTEGER

import_batches
  id, source_path, total, done, failed, state, created_at

settings
  key   TEXT PK
  value TEXT

-- FTS5 虚拟表：聚合 image 的标签名（含中文）用于全文搜索
image_tags_fts(image_id, tag_names, tag_names_cn)
```

**索引与优化**
- `images(md5)`、`images(phash)`、`images(status)`、`images(dedup_group)`、`images(exif_datetime)`、`images(aesthetic_score)`。
- `image_tags(tag_id)`、`jobs(status, next_run_at)`。
- PRAGMA：`journal_mode=WAL`、`synchronous=NORMAL`、`cache_size` 调大、必要时 `mmap_size`。
- 批量导入使用单事务 + 预编译语句批量插入。

---

## 5. 核心算法设计

### 5.1 pHash 查重
- 64-bit DCT 感知哈希：缩放 32×32 → 灰度 → DCT → 取低频 8×8 → 与均值比较得 64 bit。
- 聚类策略（百万级）：
  1. MD5 精确分组（O(n)）。
  2. pHash 近似查重：全量 pHash 载入内存（8MB/百万），分批按汉明距离 ≤ 阈值聚类；阈值默认 8，可配置。
  3. 幂等增量：新图只与现有簇代表比对，命中则入簇，否则新簇。
- 删除判定：簇内按 `clarity_score` 降序，非首位标记 `is_redundant=1` 并入回收站候选。

### 5.2 清晰度分数
- Laplacian 方差（灰度图 3×3 卷积），快速且对二次元线条图有效；可选 BRISQUE 扩展点。
- 存储时做对数归一化（不同分辨率可比）。

### 5.3 SauceNAO 溯源
- 请求：`POST https://saucenao.com/search.php`，multipart 上传图片或 `url` 参数，`output_type=2`（JSON），带 `api_key`。
- 解析：`results[].header.similarity`、`index_id`、`ext_urls[]`。
- 有效判定：`similarity ≥ 阈值`（默认 75%）且 `ext_urls` 含 `danbooru.donmai.us` / `gelbooru.com`。
- 标签爬取：
  - danbooru：`GET https://danbooru.donmai.us/posts/{id}.json` → `tag_string`（官方 API，无需 key）。
  - gelbooru：`GET https://gelbooru.com/index.php?page=dapi&s=post&q=index&id={id}` → XML → `tags`。
  - API 失败 → scraper 解析帖子页面 HTML。
- 缓存：按图片 `md5` 缓存溯源结果与标签，重复导入不重复请求。
- 限流：令牌桶，速率可配置；失败（429/超时）指数退避重试。

### 5.4 本地打标（cl_tagger）
- 抽离 `D:\Game\AI\cl_tagger` 的核心推理（ONNX session + SIGLIP2 processor + 阈值过滤）为 Python 模块。
- 输出：`{tag: confidence}`，按阈值（默认 0.5）过滤，存 `image_tags(source=auto_local, confidence)`。
- 显存与美学模型共用，走推理队列。

### 5.5 美学评分
- 模型：`distill-q-align-aesthetic-siglip2-base`（transformers 加载）。
- 输出验证项：确认分数范围（预期 1–5 连续值）与批处理输入格式；存储原始分，UI 按可配置映射展示（如星级/百分比）。
- 支持 GPU 批量、失败重试、选区重算。

---

## 6. UI 设计

### 6.1 页面结构
| 页面 | 内容 |
|---|---|
| 总览 | 统计卡片（总量/冗余候选/待打标/待评分）、最近导入、快捷筛选入口、当前任务进度 |
| 图片浏览 | 网格/瀑布流/列表三种视图；虚拟滚动；缩略图懒加载；多选与批量操作（删除/加标签/导出/评分） |
| 单图详情 | 大图、元数据、标签面板（增删/来源标记）、来源链接跳转、相似图片（pHash 邻近）、加入回收站/恢复 |
| 筛选搜索 | 组合筛选器（日期/标签/质量/来源/格式…）+ 排序；保存智能视图 |
| 查重管理 | 重复组列表（簇大小/冗余数）、组内对比查看（并排/叠加）、一键"保留最优，其余入回收站"、逐张确认 |
| 回收站 | 列表、恢复、永久删除、清空（二次确认） |
| 任务中心 | 流水线进度条、失败列表（重试/忽略）、历史 |
| 标签管理 | 自定义标签增删改、批量合并/重命名、黑名单管理、中文字典开关 |
| 设置 | 全部可配置项（见 2.9） |

### 6.2 交互与体验
- 键盘快捷键（方向键翻页、`Delete` 入回收站、`Ctrl+A` 全选等）。
- 深色/浅色主题，跟随系统。
- 空状态/加载骨架屏/错误提示统一。
- 大图预览支持缩放、旋转、翻页（前/后一张）。

### 6.3 前端性能
- 缩略图分级：`thumb`（列表，~256px）/ `card`（网格，~512px）/ 原图按需。
- 统一 WebP 编码，缩略图缓存于 `data/thumbs/`（哈希命名）。
- 万级列表虚拟滚动 + 图片解码节流（`decoding="async"`）。

---

## 7. 性能设计（10万~100万级）

1. **缩略图缓存**：导入时同步生成，浏览零解码大图。
2. **SQLite 调优**：WAL、批量事务、预编译语句、针对性索引、FTS5。
3. **内存缓存**：热点标签、统计计数（可失效）、最近查询结果。
4. **并行**：扫描/哈希/缩略图用 rayon 多线程；推理走 GPU 队列；网络 IO 走 tokio。
5. **异步化**：一切重操作进任务队列，UI 只读快照 + WS 推送进度。
6. **批量 API**：列表/筛选接口分页 + 游标，避免大结果集。
7. **增量**：查重、FTS、统计均增量维护，不做全量重算（提供手动全量重建兜底）。

---

## 8. 可维护性与扩展性

- **Rust workspace 分层**：`core / db / ingest / dedup / pipeline / tagger / api / app`，模块职责单一。
- **流水线插件化**：`PipelineStep` trait，后续可插拔加入 OCR、NSFW 检测、向量近重复索引、人脸聚类等。
- **推理服务独立**：模型增删不影响主服务；接口稳定（图 → 结果）。
- **配置中心化**：`settings` 表 + 启动配置文件双通道。
- **日志**：tracing 结构化日志，任务级 trace_id。
- **测试**：单元（算法/仓储）、集成（流水线端到端）、前端组件测试；查重/限流等核心逻辑重点覆盖。
- **数据库迁移**：版本化迁移脚本，升级不丢数据。
- **错误处理**：任务失败可重试、可跳过；导入校验先行，移动失败回滚。

---

## 9. 项目目录结构（规划）

```
image/
├── docs/PLAN.md              # 本文档
├── backend/                  # Rust workspace
│   ├── Cargo.toml
│   └── crates/
│       ├── core/             # 领域模型、配置、错误类型
│       ├── db/               # 连接、迁移、仓储
│       ├── ingest/           # 扫描、移动进库、缩略图、哈希
│       ├── dedup/            # pHash 聚类、清晰度
│       ├── pipeline/         # 任务队列、流水线编排、限流
│       ├── tagger/           # SauceNAO 客户端、danbooru/gelbooru 爬虫、推理客户端
│       ├── api/              # axum 路由、WS/SSE
│       └── app/              # 二进制入口、配置加载
├── frontend/                 # Vue3 + Vite + TS + Element Plus
│   ├── src/{views,components,stores,api,router}
│   └── package.json
├── python/                   # 推理服务
│   ├── server/               # FastAPI 应用（打标/美学/清晰度接口）
│   └── cl_tagger_core/       # 自 cl_tagger 抽取的核心推理模块（不含 Gradio）
├── data/                     # 运行时数据（gitignore）
│   ├── library/              # 库目录（哈希分片）
│   ├── thumbs/               # 缩略图缓存
│   ├── recycle/              # 回收站文件
│   └── app.db                # SQLite
├── desktop/                  # 后期 Tauri 壳（包 Web UI + 拉起主服务）
└── README.md
```

---

## 10. 开发路线图

| 里程碑 | 内容 | 验收标准 |
|---|---|---|
| M0 环境准备 | 安装 Rust 工具链；搭建 Python 推理服务 venv；确认两个模型可用；SauceNAO key 配置 | 推理服务可对样例图返回标签/分数 |
| M1 骨架 | Rust 服务骨架 + SQLite 迁移 + 前端脚手架；图片浏览（空库） | 前后端连通，UI 框架就位 |
| M2 导入与索引 | 扫描/移动进库/缩略图/MD5/pHash/清晰度/日期提取 | 10万张模拟库可完整入库 |
| M3 查重 | 聚类 + 模糊清晰对 + 回收站 + 查重管理 UI | 构造重复样本集验证分组正确 |
| M4 打标 | 推理服务整合 cl_tagger；SauceNAO 溯源 + danbooru/gelbooru 爬取 + 缓存限流 | 二次元样本溯源命中率高；无网络时回退本地打标 |
| M5 美学 + 搜索 | 美学评分流水线；FTS5 搜索 + 组合筛选/排序 UI | 评分/筛选端到端可用 |
| M6 标签管理 | 手动标签/合并/黑名单/中文字典/sidecar/设置页 | 功能齐备 |
| M7 性能与稳定 | 百万级压测、索引调优、任务断点续跑、日志、测试补全 | 10万级日常操作流畅，任务可恢复 |
| M8 桌面壳 | Tauri 打包（启动主服务 + 推理服务 + Web UI），安装包 | 双击即用 |

---

## 11. 风险与开放项

| 项 | 说明 / 默认处理 |
|---|---|
| Rust 工具链未安装 | M0 前置：安装 rustup（当前环境未检出 rustc/cargo） |
| 美学模型输出格式 | 需实测确认分数范围与批处理接口；按 1–5 假设，可配置归一化 |
| SauceNAO 免费限流 | 10万张全量溯源很慢（约 30s/次 → 数天）；限流/缓存/断点续跑已设计，建议升级会员或选区溯源 |
| danbooru/gelbooru 稳定性 | 官方 API 优先 + 页面解析回退；爬取失败不影响本地打标 |
| 中文字典来源 | 采用开源 danbooru 中文翻译表（如 sd-webui 翻译插件数据），仅显示层 |
| EXIF 缺失 | 日期回退文件修改时间；历史文件重扫可补 |
| 移动进库的破坏性 | 先校验/哈希后移动，失败回滚；回收站兜底 |
| Windows 文件监控 | notify 在部分场景不可靠，提供定时轮询兜底 |
| 移动进库与"源文件夹继续使用"冲突 | 导入后源位置为空；如用户需要可后期加"复制模式"选项 |

---

## 12. 待用户进一步确认的细节（当前用默认值）

- 缩略图规格（默认 thumb 256px / card 512px WebP）
- 查重汉明距离阈值（默认 8）
- 溯源相似度阈值（默认 75%）
- 打标置信度阈值（默认 0.5）
- 回收站自动清空天数（默认关闭）
- 平台范围（默认 Windows 优先，Rust/Web 天然跨平台）
