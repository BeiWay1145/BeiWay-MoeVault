# BeiWay-MoeVault — 技术细节文档（v0.1）

> 配套：`docs/PLAN.md`（规划）、`docs/UI_DESIGN.md`（UI）。本文档定义数据库、API、模块边界，是搭建骨架的契约基准。

---

## 1. 数据库 DDL（SQLite，WAL）

### 1.1 建表

```sql
-- 基础 PRAGMA（连接建立时执行）
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;

CREATE TABLE images (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  md5             TEXT    NOT NULL UNIQUE,          -- 精确查重键
  phash           INTEGER NOT NULL,                 -- 64-bit DCT pHash
  rel_path        TEXT    NOT NULL UNIQUE,          -- 相对 data/library 的路径
  width           INTEGER NOT NULL,
  height          INTEGER NOT NULL,
  format          TEXT    NOT NULL,                 -- jpg/png/webp/gif/avif/bmp
  size_bytes      INTEGER NOT NULL,
  file_mtime      INTEGER NOT NULL,                 -- epoch seconds
  exif_datetime   INTEGER,                          -- epoch seconds，无 EXIF 为 NULL
  clarity_score   REAL    NOT NULL,                 -- 清晰度（对数归一化 Laplacian 方差）
  aesthetic_score REAL,                             -- 美学分（模型输出，范围待实测）
  dedup_group     INTEGER REFERENCES dedup_groups(id),
  is_redundant    INTEGER NOT NULL DEFAULT 0,       -- 簇内非最优 = 冗余候选
  status          TEXT    NOT NULL DEFAULT 'active'
                    CHECK (status IN ('active','recycled')),
  source          TEXT    NOT NULL DEFAULT 'local', -- danbooru/gelbooru/local
  source_url      TEXT,                             -- 溯源到的原帖 URL
  thumb_rel       TEXT    NOT NULL,                 -- 相对 data/thumbs 的路径
  imported_at     INTEGER NOT NULL
);

CREATE TABLE tags (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  name          TEXT    NOT NULL UNIQUE,            -- 英文标签（danbooru 体系）
  name_cn       TEXT,                               -- 可选中文翻译（仅显示）
  category      TEXT    NOT NULL DEFAULT 'general', -- general/character/copyright/meta/custom
  is_custom     INTEGER NOT NULL DEFAULT 0,
  is_blacklisted INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE image_tags (
  image_id   INTEGER NOT NULL REFERENCES images(id) ON DELETE CASCADE,
  tag_id     INTEGER NOT NULL REFERENCES tags(id)   ON DELETE CASCADE,
  source     TEXT    NOT NULL CHECK (source IN
               ('auto_danbooru','auto_gelbooru','auto_local','manual')),
  confidence REAL,                                  -- 本地打标置信度，其余为 NULL
  created_at INTEGER NOT NULL,
  PRIMARY KEY (image_id, tag_id, source)
);

CREATE TABLE dedup_groups (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  phash_seed  INTEGER NOT NULL,                     -- 簇代表 pHash
  best_image  INTEGER REFERENCES images(id),        -- 簇内清晰度最优
  state       TEXT    NOT NULL DEFAULT 'open',      -- open/resolved
  created_at  INTEGER NOT NULL
);

CREATE TABLE recycle_bin (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  image_id     INTEGER NOT NULL UNIQUE REFERENCES images(id),
  reason       TEXT    NOT NULL,                    -- duplicate/manual/auto
  original_rel TEXT    NOT NULL,                    -- 删除时的 rel_path（恢复用）
  deleted_at   INTEGER NOT NULL
);

CREATE TABLE import_batches (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  source_path TEXT    NOT NULL,
  total       INTEGER NOT NULL DEFAULT 0,
  done        INTEGER NOT NULL DEFAULT 0,
  failed      INTEGER NOT NULL DEFAULT 0,
  state       TEXT    NOT NULL DEFAULT 'pending',   -- pending/scanning/moving/indexing/done/failed
  created_at  INTEGER NOT NULL
);

CREATE TABLE jobs (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  job_type     TEXT    NOT NULL,   -- ingest/dedup/sauce_nao/fetch_tags/aesthetic/thumbnail
  image_id     INTEGER REFERENCES images(id),
  batch_id     INTEGER REFERENCES import_batches(id),
  payload      TEXT,               -- JSON 附加参数
  status       TEXT    NOT NULL DEFAULT 'pending'
                 CHECK (status IN ('pending','running','done','failed','cancelled')),
  attempts     INTEGER NOT NULL DEFAULT 0,
  max_attempts INTEGER NOT NULL DEFAULT 5,
  next_run_at  INTEGER NOT NULL DEFAULT 0,          -- 退避重试 / 断点续跑
  error        TEXT,
  priority     INTEGER NOT NULL DEFAULT 10,         -- 数值小优先
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);

CREATE TABLE sauce_cache (                          -- SauceNAO 溯源缓存
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  image_md5   TEXT    NOT NULL UNIQUE,
  similarity  REAL,
  source      TEXT,                                 -- danbooru/gelbooru/none
  source_url  TEXT,
  raw_json    TEXT,                                 -- 原始响应（调试/审计）
  hit_at      INTEGER NOT NULL
);

CREATE TABLE smart_views (                          -- 保存的筛选表达式
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT    NOT NULL UNIQUE,
  filter_json TEXT    NOT NULL,
  sort        TEXT,
  created_at  INTEGER NOT NULL
);

CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- FTS5 全文索引（应用层维护，非外部内容表）
CREATE VIRTUAL TABLE image_tags_fts USING fts5(
  image_id     UNINDEXED,
  tag_names,        -- 英文标签空格连接
  tag_names_cn      -- 中文翻译空格连接
);
```

### 1.2 索引

```sql
CREATE INDEX idx_images_status      ON images(status);
CREATE INDEX idx_images_md5         ON images(md5);
CREATE INDEX idx_images_phash       ON images(phash);
CREATE INDEX idx_images_dedup_group ON images(dedup_group) WHERE dedup_group IS NOT NULL;
CREATE INDEX idx_images_exif_date   ON images(exif_datetime);
CREATE INDEX idx_images_imported_at ON images(imported_at);
CREATE INDEX idx_images_aesthetic   ON images(aesthetic_score);
CREATE INDEX idx_images_clarity     ON images(clarity_score);

CREATE INDEX idx_image_tags_tag   ON image_tags(tag_id);
CREATE INDEX idx_image_tags_image ON image_tags(image_id);

CREATE INDEX idx_jobs_queue ON jobs(status, next_run_at, priority);
CREATE INDEX idx_recycle_deleted_at ON recycle_bin(deleted_at);
```

### 1.3 迁移策略

- **实现**：轻量自研迁移器（`backend/crates/db/src/migration.rs`），SQL 文件编译期嵌入（`include_str!`），`schema_migrations` 表记录版本；每个迁移单事务执行、幂等可重跑。未采用 refinery 以规避与 rusqlite 的版本耦合。
- 迁移文件按 `V{n}__name.sql` 命名递增（`backend/crates/db/migrations/`）。
- 启动时自动跑未应用的迁移；迁移失败则拒绝启动（日志明确报错）。
- 索引/统计为增量维护，破坏性重建走"手动全量重建"（管理命令）。

### 1.4 数据一致性规则

- `is_redundant=1` 必须同时有 `dedup_group`；入回收站时 `dedup_group` 保留（恢复后仍是候选）。
- 回收站恢复：`rel_path` 还原 + `status='active'` + 删除 recycle_bin 记录。
- 永久删除：删除 images 行（级联删 image_tags）+ 物理删文件 + 缩略图。
- 重复导入同一 md5：跳过并记入批次 `duplicate` 计数，不入库。

---

## 2. REST API（`/api/v1`）

约定：JSON；错误统一 `{"error":{"code":"...","message":"..."}}`；列表统一游标分页
`?limit=100&cursor=...`（cursor 由返回的 `next_cursor` 提供，编码 `(排序键, id)`）。
WS 鉴权：本地服务，无鉴权；绑定 127.0.0.1。

### 2.1 图片与筛选

| 方法/路径 | 说明 | 关键参数 |
|---|---|---|
| `GET /images` | 列表（可组合筛选） | `q`(FTS)、`tags`(逗号分隔, `+`=AND)、`exclude_tags`、`date_from/date_to`、`aesthetic_min/max`、`clarity_min/max`、`source`、`format`、`min_width/min_height`、`is_redundant`、`status`、`sort`(`date\|aesthetic\|clarity\|size\|imported\|random`)、`order`、`limit`、`cursor` |
| `GET /images/:id` | 详情（含 tags/source/相似图摘要） | |
| `GET /images/:id/file` | 原图（支持 Range） | |
| `GET /images/:id/thumb?size=card\|thumb` | 缩略图 | |
| `POST /images/:id/tags` | 手动加标签 | `{tag_ids:[]}` 或 `{tag_names:[]}` |
| `DELETE /images/:id/tags/:tag_id?source=manual` | 删除手动标签 | |
| `POST /images/:id/recycle` | 入回收站 | `{reason}` |
| `POST /images/:id/restore` | 从回收站恢复 | |
| `POST /images/:id/retag` | 重新溯源+打标（单张手动） | `{force_sauce:bool}` |
| `GET /images/:id/similar` | pHash 邻近图（按汉明距离升序） | `limit` |
| `POST /images/batch/recycle` | 批量入回收站 | `{ids:[], reason}` |
| `POST /images/batch/tags` | 批量加标签 | `{ids:[], tag_ids:[]}` |
| `POST /images/batch/sidecar` | 批量生成 sidecar .txt | `{ids:[], overwrite:bool}` |
| `POST /images/batch/aesthetic` | 批量重算美学分 | `{ids:[]}`（空=全库） |

### 2.2 导入

| 方法/路径 | 说明 |
|---|---|
| `POST /import` | 创建批次 `{paths:["D:/a", "D:/b.png", ...]}` → `{batch_id}`；服务端扫描→移动→索引异步执行 |
| `GET /import/batches` | 批次列表 |
| `GET /import/batches/:id` | 批次详情（total/done/failed/duplicate/state） |
| `POST /import/batches/:id/cancel` | 取消（已入队的 job 标记 cancelled） |

### 2.3 查重

| 方法/路径 | 说明 |
|---|---|
| `GET /dedup/stats` | `{group_count, involved_images, redundant_count}` |
| `GET /dedup/groups` | 组列表（含代表图摘要、成员数、冗余数）`?state=open&limit&cursor` |
| `GET /dedup/groups/:id` | 组详情（全部成员：缩略图+清晰度+美学分+is_redundant+best 标记） |
| `POST /dedup/groups/:id/resolve` | `{mode:"best_only"}` 保留最优其余入回收站；`{mode:"specific", keep_ids:[], recycle_ids:[]}` 精确指定 |
| `POST /dedup/scan` | 手动全库重扫（建后台任务） |

### 2.4 回收站

| 方法/路径 | 说明 |
|---|---|
| `GET /trash` | 列表 `?reason&q&limit&cursor` |
| `POST /trash/:image_id/restore` | 恢复 |
| `POST /trash/:image_id/purge` | 永久删除（物理） |
| `POST /trash/purge-all` | 清空（二次确认在前端） |

### 2.5 任务

| 方法/路径 | 说明 |
|---|---|
| `GET /tasks` | `?status=pending\|running\|done\|failed&job_type&limit&cursor` |
| `GET /tasks/:id` | 详情（含 error 全文） |
| `POST /tasks/:id/retry` | 重试（attempts 清零、状态回 pending） |
| `POST /tasks/:id/cancel` | 取消 |
| `POST /tasks/retry-failed` | 全部失败重试 |

### 2.6 标签

| 方法/路径 | 说明 |
|---|---|
| `GET /tags` | `?q&category&blacklisted&limit&cursor` |
| `POST /tags` | 创建 `{name, category?, name_cn?}` |
| `PUT /tags/:id` | `{name?, name_cn?, category?}`（重命名同步全部关联） |
| `POST /tags/:id/merge` | 合并到 `{target_tag_id}`（事务：转移 image_tags、删本标签） |
| `DELETE /tags/:id` | 删除（级联解除关联） |
| `PUT /tags/:id/blacklist` | `{blacklisted:bool}` |
| `GET /tags/autocomplete` | 联想 `?q&limit=20` |
| `GET /tags/export` / `POST /tags/import` | CSV 导入导出（含 name_cn/category） |

### 2.7 智能视图 / 统计 / 设置

| 方法/路径 | 说明 |
|---|---|
| `GET/POST/PUT/DELETE /views` | 智能视图 CRUD（`filter_json` 与 `/images` 查询参数同构） |
| `GET /stats` | 总览：总数/本月导入/冗余候选/待打标/待评分/平均美学分 |
| `GET /settings` | 全量设置 |
| `PUT /settings` | 批量更新（增量 diff，返回生效项） |
| `POST /settings/test-saucenao` | 测试 SauceNAO key（发起一次最小请求） |
| `POST /settings/reindex-fts` | 重建 FTS5（管理操作） |
| `POST /export/metadata` | `{ids?}|{filter?}, format:"json"\|"csv"` |

---

## 3. WebSocket（`/ws`）

事件推送（Rust 侧 Hub 广播，前端按事件刷新 store，避免轮询）：

```json
{ "type": "task.progress",   "ts": 1718000000, "payload": { "job_id": 42, "job_type": "sauce_nao", "status": "running", "progress": 0.31 } }
{ "type": "task.done",       "ts": ..., "payload": { "job_id": 42, "job_type": "sauce_nao" } }
{ "type": "task.failed",     "ts": ..., "payload": { "job_id": 42, "error": "..." } }
{ "type": "batch.progress",  "ts": ..., "payload": { "batch_id": 12, "done": 128, "total": 164, "state": "indexing" } }
{ "type": "library.updated", "ts": ..., "payload": { "count": 5 } }
{ "type": "dedup.updated",   "ts": ..., "payload": { "group_count": 1024, "redundant_count": 3287 } }
{ "type": "stats.updated",   "ts": ..., "payload": { } }
```

---

## 4. 推理服务契约（Python FastAPI，`127.0.0.1:<port>`）

- 输入一律传**本地绝对路径**（同机，避免 base64 传输开销）；服务校验路径存在与扩展名。
- 统一响应错误：`{"error": {"code", "message"}}`，HTTP 4xx/5xx。

| 方法/路径 | 说明 | 请求 | 响应 |
|---|---|---|---|
| `GET /health` | 存活 + 模型加载状态 | — | `{"status":"ok","models":{"tagger":true,"aesthetic":true}}` |
| `POST /infer/tags` | cl_tagger 打标 | `{"path":"...","threshold":0.5}` | `{"tags":[{"name":"1girl","confidence":0.92}],"model":"siglip2","elapsed_ms":123}` |
| `POST /infer/aesthetic` | 美学评分 | `{"path":"..."}` | `{"score":4.6,"model":"q-align-siglip2","range":[1,5],"elapsed_ms":88}` |
| `POST /infer/tags_batch` | 批量打标 | `{"paths":[...],"threshold":0.5}` | `{"results":[{...}]}`（GPU 串行，服务内排队） |
| `POST /infer/aesthetic_batch` | 批量评分 | `{"paths":[...]}` | `{"results":[{...}]}` |

- 打标与美学共用 GPU：服务内单队列串行执行（默认），并发上限可配置。
- 加载模型失败时 `/health` 返回对应 `models.*=false`，主服务据此降级（溯源/打标任务标记 failed 并提示）。

---

## 5. Rust workspace 模块边界

```
backend/
├── crates/
│   ├── core/      # 领域类型、配置、错误（纯数据，零依赖外部服务）
│   ├── db/        # 连接池(单连接+WAL 或 r2d2)、refinery 迁移、仓储 trait 与实现
│   ├── ingest/    # 扫描、移动进库、缩略图、MD5/pHash/清晰度、EXIF 提取
│   ├── dedup/     # pHash 聚类、簇维护、冗余判定
│   ├── pipeline/  # JobQueue（SQLite 持久化）、PipelineStep trait、调度器、令牌桶限流
│   ├── tagger/    # SauceNaoClient、DanbooruClient、GelbooruClient、InferenceClient(HTTP→Python)
│   ├── api/       # axum 路由、DTO、WS Hub、静态资源托管
│   └── app/       # main：配置加载、启动编排（含 spawn 推理服务子进程）、优雅退出
```

### 核心 trait（草案）

```rust
// pipeline
#[async_trait]
pub trait PipelineStep: Send + Sync {
    fn job_type(&self) -> JobType;
    async fn run(&self, ctx: &StepContext, job: &Job) -> Result<StepOutcome, StepError>;
}

// 调度器伪码
loop {
    let job = queue.claim_next().await?;         // pending 且 next_run_at <= now，按 priority
    let step = registry.get(job.job_type)?;
    match step.run(&ctx, &job).await {
        Ok(outcome) => queue.finish(job.id, outcome).await?,
        Err(e) if job.attempts < max => queue.retry(job.id, backoff(job.attempts), e).await?,
        Err(e) => queue.fail(job.id, e).await?,
    }
}
```

```rust
// tagger
pub struct SauceNaoClient { /* base_url, api_key, rate_limiter(令牌桶) */ }
impl SauceNaoClient {
    pub async fn search(&self, path: &Path) -> Result<SauceNaoResult, SauceError>;
    // SauceNaoResult { similarity: f32, ext_urls: Vec<String>, index_id: u32 }
}

pub struct DanbooruClient;   // GET /posts/{id}.json -> tag_string
pub struct GelbooruClient;   // GET dapi -> XML -> tags
```

```rust
// core 领域类型（与 DB 表一一对应）
pub struct Image { /* ... */ }
pub struct Tag { /* ... */ }
pub struct Filter { /* tags: Vec<String>, and: bool, date_range, aesthetic, ... -> 编译为 SQL */ }
```

- `api` 只依赖 `core`/`db`/`pipeline` 的公开接口；`pipeline` 依赖 `ingest`/`dedup`/`tagger`；依赖方向单向（`app` → 全部）。
- `Filter` 编译为安全 SQL（rusqlite 参数绑定，无字符串拼接）。

---

## 6. 关键实现要点

1. **游标分页**：`next_cursor = base64(join(":", sort_key, id))`；排序键建立索引（`imported_at`/`aesthetic_score` 等）。
2. **pHash 聚类增量**：新图只与 `dedup_groups.phash_seed` 比对（内存中全部 seed 约 8KB/万组），命中入簇并可能更新 best；未命中开新簇。
3. **模糊清晰对判定**：`dedup_groups` 内 `clarity_score` 降序，非首位 `is_redundant=1`；resolve 时按用户选择批量 `recycle`。
4. **SauceNAO 限流**：令牌桶（速率 = 设置项，默认 1/30s）；429/超时指数退避；`sauce_cache` 命中直接跳过网络。
5. **推理降级**：推理服务不可用时打标/评分任务标记 failed（不阻塞导入/查重）；健康恢复后重试。
6. **FTS 维护**：image_tags 变更后由 db 层在同一事务内更新 `image_tags_fts`（删除旧行+插入新行）。
7. **缩略图**：导入时生成 `thumb`（256px）/`card`（512px）两级 WebP，路径 `data/thumbs/<md5 前2位>/<md5>.webp`。
8. **移动进库**：先读文件计算 md5 → 目标路径 `data/library/<md5 前2位>/<md5 前32位>.<ext>` → 若目标已存在（重复）则删源文件并记 duplicate → 否则 move，失败回滚并记 failed。
9. **EXIF**：读取 EXIF DateTimeOriginal；失败回退 `file_mtime` 写入 `exif_datetime`（NULL）与 `file_mtime` 区分。
10. **优雅退出**：收到 Ctrl+C/SIGTERM → 停止调度 → 等待 running job 落盘 → 关库。

---

## 7. 待实测确认项（影响契约字段）

- 美学模型输出范围与批处理格式（`range` 字段按实测修正）。
- cl_tagger 抽取后模型加载方式（复用原 `.venv` 的 onnxruntime-gpu 与 transformers）。
- danbooru/gelbooru API 的限流与字段版本（tag_string 格式、dapi XML 结构）。
