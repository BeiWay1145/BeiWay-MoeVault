# 任务 2 设计方案：搜索式筛选（v2，2026-08-28 用户重定向）

> 状态：方案随实施演进。2026-08-28 用户决定改为 danbooru 风格的**搜索式筛选**，
> 原「分类浏览」面板（P3）**暂作废案隐藏**（后端接口保留备用），待「图集」设计敲定后再以独立路由登场。
> 关联：增强 2 的 `viewerContext` 已落地，所有列表视图接入后详情页导航自动正确。

---

## 一、目标与范围

用户点名诉求（对应 docs/ENHANCEMENTS.md P1-5/P1-6）：

1. **保存/命名筛选方案**——当前筛选条件无法保存复用
2. **标签多选 OR**——当前多标签是 AND（必须同时命中），缺"任一命中"模式
3. **空结果友好引导**——无匹配时给出可能原因与一键调整
4. **筛选状态 URL 可分享/可恢复**——query 参数承载筛选，可收藏/发给别人
5. **分类导航**——画师/系列/角色/常规标签分组浏览（数据已有 category 字段）

设计原则：**后端小步演进、前端组件化、复用已有数据与机制**（`smart_views` 表、`tags.category`、`viewerContext`、TagManageView 骨架）。

---

## 二、现状盘点（代码事实）

### 2.1 后端能力（已具备）

`GET /api/v1/images` 查询参数 → `ImageFilter`（backend/crates/core/src/models.rs）：

| 维度 | 参数 | 说明 |
|---|---|---|
| 关键字 | `q` | 文件名 LIKE |
| 标签 | `tags`（**AND**）/ `exclude_tags` | EXISTS 子查询 |
| 日期 | `date_from/to` | EXIF 时间回退 mtime |
| 美学 | `aesthetic_min/max` + `aesthetic_include_unscored` | 1-5 分 |
| 清晰度 | `clarity_min/max` | |
| 来源/格式 | `source` / `format` | danbooru/gelbooru/local |
| 尺寸 | `min_width/height` | |
| 状态 | `is_redundant` / `is_ai` / `sauce_status` | |
| 排序 | `sort`（imported/date/aesthetic/clarity/size/random）+ `order` + 游标分页 | |

标签数据模型（V1__init.sql）：
- `tags(id, name UNIQUE, name_cn, category, is_custom, is_blacklisted)`
- `image_tags(image_id, tag_id, source, confidence)`，source 含 auto_*/manual/ai
- `GET /api/v1/tags` → `TagWithCount{id,name,name_cn,category,image_count}`（含 q 搜索、按图数倒序）——**缺 category 过滤参数**

**关键发现**：`smart_views(id, name UNIQUE, filter_json, sort, created_at)` 表 **V1 已建但从未使用**——"保存命名筛选方案"的后端存储现成。

### 2.2 前端现状

- **图库页筛选控件被隐藏**（`SHOW_LIBRARY_FILTERS = false`，代码注释"筛选功能未完整实装前隐藏"）；实际筛选能力集中在搜索页（关键字/美学范围/来源/溯源状态/标签输入）
- `LibraryFilter` 类型已含大部分字段；未暴露：日期范围、尺寸、clarity_max、格式等（后端有、前端没接）
- 标签选择为逗号分隔文本输入（AND），无 OR、无可视化选择器
- TagManageView：标签 CRUD / 分类修改 / 拉黑 已可用
- 增强 2 已落地：`viewerContext` 有序上下文 + 详情页位置指示

### 2.3 缺口 → 方案映射

| 用户诉求 | 现状 | 方案 |
|---|---|---|
| 保存命名筛选 | 无 | 3.1-C（启用 smart_views） |
| 标签 OR | 仅 AND | 3.1-B（后端 tag_mode） |
| 空结果引导 | 无 | 3.1-E |
| URL 分享筛选 | 无 | 3.1-D |
| 分类导航 | 无 | 3.2 |

---

## 三、方案设计

### 3.1 筛选增强

#### A. 统一筛选栏组件 `FilterBar.vue`（前端）

- 图库页启用（替换临时隐藏），搜索页复用同一组件；分类浏览结果页也复用
- 分区：
  - **关键字**输入
  - **标签选择器**：输入搜索 → 下拉候选（显示中文名/图数）→ 多选 chips；旁挂 **全部(AND)/任一(OR)** 切换
  - **分类区**：来源 / 格式 / AI / 溯源状态 / 冗余 下拉+多选
  - **评分区**：美学双滑块（含"包含未评分"开关）、清晰度
  - **日期范围**、**尺寸**（min 宽/高）
- 已选条件以可删除 chips 汇总，一键清空
- 状态模型直接对应 `ImageFilter`，提交给 store → `fetchImages()`；**与增强 2 联动**：图库/搜索/浏览的 `setViewerContext` 继续生效

#### B. 标签 OR 模式（后端 1 处 + 前端选择器）

- `ImageFilter` 增加 `tag_mode: Option<TagMode>`（`all` 默认 / `any`）
- `build_filter_conds`：
  - all：现状（每标签一个 EXISTS）
  - any：单个 `EXISTS (… WHERE tg.name IN (…))`
- API：`/images?tags=a,b&tag_mode=any`
- 前端：标签选择器旁「全部命中 / 任一命中」单选；chips 显示当前模式；**与 exclude_tags 组合语义**：排除仍是"命中任一即排除"（文档写明）

#### C. 保存的命名筛选方案（启用 `smart_views` 表）

- 后端新增（routes/smart_views.rs）：
  - `GET /api/v1/smart-views` → 列表（含方案名）
  - `POST /api/v1/smart-views` `{name, filter, sort}` → 校验 `filter` 反序列化为 `ImageFilter` 合法子集；重名返回 409
  - `PUT /api/v1/smart-views/{id}`、`DELETE /api/v1/smart-views/{id}`
- 前端：筛选栏「保存当前方案」→ 命名对话框；侧栏新增「我的方案」分组或筛选栏下拉，点击即套用（含排序）
- 说明：方案只存**筛选参数 + 排序**，不存图片集合（动态视图语义）

#### D. URL 可分享/可恢复

- 图库/搜索/浏览视图将筛选状态序列化到 query（`?q=&tags=&tag_mode=&amin=&…`），路由 `replace` 同步（避免历史污染）
- 进入视图时从 query 反序列化恢复并应用
- 与 smart_views 互补：URL = 临时分享；方案 = 长期收藏

#### E. 空结果引导

- 抽取 `EmptyResult.vue`：显示当前筛选 chips + 建议文案（按场景）：
  - 有标签 AND → "试试切换为『任一命中』"
  - 有排除标签 → "尝试移除排除项"
  - 有美学下限 → "降低美学分要求或勾选包含未评分"
  - 有关键字 → "检查关键字拼写"
  - 无任何条件 → "导入图片后自动出现（支持拖拽）"
- 一键按钮直接执行对应调整

### 3.2 分类导航（标签分类浏览）

#### A. 后端

- `list_tags_filtered` 增加可选 `category` 参数；`GET /api/v1/tags` 暴露 `category`、`min_count` 过滤（复用现有 q/分页）
- 新增 `GET /api/v1/tags/browse?category=&q=&limit=&offset=`：
  ```
  { items: [{ id, name, name_cn, category, image_count, cover_thumb }], total }
  ```
- **封面规则可配置**（评审决策 4），设置项 `tag_cover_rule`（后端 settings 表 + 白名单）：
  - `aesthetic`：该标签下美学分最高 1 张（NULL 排后）
  - `size`：文件大小最大 1 张
  - `newest`：最新导入 1 张
  - `random`：随机 1 张
  - `manual`：读 `tag_covers` 表（手动指定），未指定回退 aesthetic
  - 缺省 `aesthetic`；`cover_thumb` 取该图 `thumb_rel`
- **手动封面**（新迁移 V10）：`tag_covers(tag_id PK REFERENCES tags, image_id REFERENCES images)`；`PUT /api/v1/tags/{id}/cover {image_id|null}` 设/清

#### B. 前端（图库内嵌面板，评审决策 2——不新增路由）

- 图库工具栏新增「分类浏览」开关 → `el-drawer` 右侧抽屉面板，内含：
  - 封面规则下拉（美学最高/文件最大/最新/随机/手动）
  - 分类 Tab：全部 / 画师 / 系列 / 角色 / 常规
  - 标签名/中文名搜索框
  - **标签卡片网格**（封面缩略图 + 名称(中文名) + 图数量；hover 操作：手动设封面 / 清除手动封面）
  - 手动设封面：弹图格（`/images?tags=<name>&limit=30` 缩略图）→ 点选 → `PUT /tags/{id}/cover`
- 点标签卡片 → 关闭面板 + 设置 `library.filter.tags=[name]` → `fetchImages()` → 图库网格显示结果 + 顶部活跃标签 chips（可清除）
- 点图进详情：`setViewerContext(结果 ids, '标签：<name>')` → 详情页在该标签集合内导航（增强 2 机制）
- 封面规则变更 → `PUT /api/v1/settings` 持久化 → 重新拉取 browse

#### C. 与增强 2 的联动

浏览/筛选/搜索产生的任何有序结果列表 → `setViewerContext(ids, label)` → 详情页导航天然正确（已就绪，只需各视图调用）。

### 3.3 数据与迁移

- 新增 **V10__tag_covers**：`tag_covers(tag_id INTEGER PRIMARY KEY REFERENCES tags(id) ON DELETE CASCADE, image_id INTEGER NOT NULL REFERENCES images(id) ON DELETE CASCADE)`（手动封面映射；其余无新表，`smart_views` 已存在）
- 无破坏性变更：`tag_mode` 缺省 = 现状语义；`/tags/browse`、`/tags/{id}/cover` 为新增端点；设置项 `tag_cover_rule` 走现有 settings 白名单
- 性能：封面 top-1 相关子查询走 image_tags(tag_id)/images 主键索引；OR 的 `IN` 查询优于 N 个 EXISTS；大数据量时后续可加物化统计（本期不做）

### 3.4 实施分期（每期独立可交付）

| 期 | 内容 | 依赖 |
|---|---|---|
| **P3（本次）** | 分类浏览：browse API（category + 可配置封面 5 规则）+ V10 tag_covers + 图库内嵌面板 + 点标签筛选 + 详情标签集内导航 | 无 |
| **P1** | B 标签 OR（后端+选择器）；A FilterBar 统一（图库启用+搜索复用）；E 空结果引导 | 无 |
| **P2** | C smart_views CRUD + 保存方案 UI；D URL query 同步 | P1 的 FilterBar |

### 3.5 风险与边界

- URL 长度：筛选参数较多时 query 可能很长（标签多选）——上限 20 个标签/方案，超出提示用"保存方案"
- OR+排除组合：语义为「(任一命中 tags) 且 (不命中 exclude_tags)」，文档与 UI 明示
- smart_views 重名覆盖需确认（见开放问题）
- 大库性能：本期不引入标签计数缓存，依赖索引；观察后再优化

---

## 五、搜索式筛选（当前实施方向）

- **图库工具栏左侧搜索框**（与视图模式同高）：danbooru 风格标签/状态联想
- `GET /api/v1/search/suggest?q=&limit=`：标签前缀匹配（名称/中文名，LIKE 转义）按词频倒序；
  状态匹配（ai / 溯源 / sauce / 冗余 / 来源）带各自图片数量
- 选中联想项 → 搜索框内出现**色块 chips**（标签=primary、状态=warning）→ 即时筛选刷新
- 标签 AND 语义；状态 chips 映射 is_ai / sauce_status / is_redundant / source
- **/search 页面已删除**（路由、侧栏入口、Dashboard 快捷入口均已改指图库）

## 六、分类浏览（废案，暂隐藏）

- 图库内嵌面板已移除；后端 `GET /api/v1/tags/browse`、`PUT /tags/{id}/cover`、V10 `tag_covers` 表保留备用
- 待「图集」（真人写真/漫画等独立集合）设计敲定后，以独立路由（英文名待定）重新登场

---

## 四、评审决策（2026-08-28 已确认）

| # | 决策项 | 结论 |
|---|---|---|
| 1 | 保存方案存储 | **后端 `smart_views` 表** |
| 2 | 分类导航入口 | **图库页内嵌面板**（`el-drawer`，不新增路由） |
| 3 | 标签多选默认模式 | **全部命中 AND**（显式切换任一） |
| 4 | 封面图规则 | **可配置**：美学最高 / 文件最大 / 最新 / 随机 / 手动选择（设置项 `tag_cover_rule`；manual 用 `tag_covers` 表） |
| 5 | 实施顺序 | **P3 分类浏览先行**（本次实施） |
