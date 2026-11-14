# BeiWay-MoeVault 架构审查报告

> 审查日期：2026-11-14
> 审查对象：`D:\Code\Reasonix_Projects\image`
> git HEAD：`dc805ebc28c225cffb8770d0c4147db0854dec9f`（工作区仅 `?? test/` 未跟踪）
> 审查模式：**只读 / architecture-hotspot-scan + focus:architecture**。本次未修改任何源码文件。
> 规模：backend 44 文件 13397 行 · frontend 37 文件 11347 行 · src-tauri 5 文件 1814 行，合计 26533 行。

## 0. 说明与已知重构基线

本报告只讨论**近期重构之后仍然存在**的结构问题。以下已完成的改动不作为问题提出：

- `frontend/src/composables/useSelection.ts` 统一选择模型
- `frontend/src/constants/batchActions.ts` 批量动作注册表
- `frontend/src/composables/useMarqueeSelect.vue` 框选
- `frontend/src/components/ContextMenu.vue`
- `frontend/src/utils/scrollTarget.ts` 的 `findScrollContainer()`
- `backend/crates/src-tauri/file_ops.rs` 集中 CF_HDROP 逻辑
- `backend/crates/tagger/src/exitpool.rs` 多出口 IP 池

严重度定义：

| 级别 | 含义 |
|---|---|
| **P0 / 必须改** | 已经在产生可观测的功能错误、数据不一致或平台稳定性风险；下一轮迭代前必须处理 |
| **P1 / 应该改** | 不改会持续放大维护成本，且每次新增功能都在同一批点上重复付费 |
| **P2 / 建议改** | 结构不理想但风险可控，适合在有相关需求时顺带处理 |
| **P3 / 可以不动** | 已识别但不值得投入，写明理由与触发条件 |

---

## 1. 模块边界

### 1.1 依赖图：无循环，方向正确

实测 `backend/Cargo.toml` 的 7 个 workspace member 与各 `Cargo.toml`：

| crate | 依赖 |
|---|---|
| `core` | 仅 `serde` |
| `db` | `core` + `rusqlite` + `thiserror` + `tracing` |
| `ingest` | `core`, `db` + `serde_json` |
| `dedup` | `core`, `db` |
| `tagger` | `core`, `db`, `ingest` + `reqwest` |
| `api` | `core`, `db`, `ingest`, `dedup`, `tagger` + `reqwest` + `axum` + `tokio` |
| `app` | `core`, `db`, `api` |

**无循环依赖，分层自下而上干净。** 这一点做得对，不必改。

> 注：`src-tauri` 是**独立的 workspace**（`src-tauri/Cargo.toml` package name = `app`，lib name = `app_lib`），不在 `backend/Cargo.toml` 的 members 里，与前 7 个 crate 无 Cargo 依赖。之前的审查笔记把它误记为 `backend/crates/src-tauri`，实际路径是仓库根 `src-tauri/`。

### 1.2 正面结论：api 层无 SQL 直写（这条不用再管）

`grep SELECT/INSERT/rusqlite` 于 `backend/crates/api/src` —— **全空**。api 层没有绕过 db 层直接操作 SQLite，db 层边界守住了。这是本次审查最重要的正面结论之一。

### 1.3 问题一：`app_error_response` 零调用，`DbError` 却泄漏到 api

`api/src/routes/mod.rs:39` 直接 `use moevault_db::DbError`，即 **api 层知道 db 层的错误类型**。而 `core` 里已经准备了 `impl From<DbError> for AppError`（`db/src/lib.rs:31`）和 `app_error_response`（`mod.rs:34-36`），本可以让 db 层只暴露 `AppError`、api 层完全不需要 `use moevault_db::DbError`。

现状：这条转换链**建好了但没接通**，api 层绕过它直接抓 `DbError`。详见 §4.1。

- 严重度：**P2 / 建议改**，与 §4.2 一起做。代价：`api/src/routes/mod.rs` 删 1 行 use + `db_error_response` 改为 `|e: DbError| app_error_response(e.into())`，88 处调用点**不用改**（签名不变）。风险低。

### 1.4 问题二：api 层直接依赖 tagger 的领域类型

`api/src/routes/tagging.rs` 与 `aesthetic.rs` 直接 `use moevault_tagger::{ApiKeyPool, ExitPool, ...}`，`api/src/state.rs:8` 也 `use moevault_tagger::{ApiKeyPool, ExitPool}`。

这导致 **`AppState` 的形状由 tagger 的内部实现决定**：`state.rs:24-31` 的 `sauce_pool: Arc<RwLock<Option<Arc<ApiKeyPool>>>>`、`exit_pool: Arc<RwLock<Option<Arc<ExitPool>>>>` 把 tagger 的调度器类型直接暴露在 API 共享状态上。

判断：**这条不建议改**。理由：tagger 是 api 的下游依赖（api → tagger，方向合法），api 作为编排层需要拿到调度器来启动任务，硬加一层抽象（如定义 api 自己的 trait）只会增加 3 个文件的间接层，换来零收益。这不是「职责泄漏」，是编排层的正常耦合。

### 1.5 问题三：db 层混入了业务语义

db 层应该只做「表操作」，实测有 4 处越界：

| 位置 | 越界内容 |
|---|---|
| `db/src/lib.rs:1785` | `import_cn_dict` —— 解析词典文件格式（切分、编码、缺省填充规则） |
| `db/src/lib.rs:1442` | `browse_tags` —— 标签封面选取规则（业务规则，非表操作） |
| `db/src/lib.rs:2308` | `month_start_secs` —— 自写 civil-from-days 日期算法做时区月份边界计算 |
| `db/src/lib.rs:324-491` | `import_tree`/`import_dir_images` —— 内嵌整套筛选 SQL（详见 §2.1） |

其中 `month_start_secs` 最典型：一个纯日期计算函数塞在 db 层，且是手写算法（约 40 行），本可用现成 crate 或放到 `core`。

- 严重度：**P2 / 建议改**（`import_cn_dict`）/ **P3 / 可以不动**（其余三项）。理由：这三处虽越界，但都是「读一次、写一次」的自包含逻辑，抽出去只是搬家不改行为；`browse_tags` 的封面规则与它的 SQL 强耦合，硬拆反而要跨层传参。触发条件：等这些函数需要第二次被调用时再动。

### 1.6 问题四：config 解析有两份真源（出口池）

同一份配置被两处独立解析，实现逐字相同：

- `api/src/state.rs:38-64` `AppState::new` 内直接 `db.get_setting("sauce_proxy_ports")` + `split([',', ';', ' ', '\n'])` + `parse::<u16>()` + `filter(|p| *p > 0)`；`sauce_proxy_enabled` 用 `v == "true" || v == "1"`
- `api/src/routes/settings.rs:445-457` `fn read_exit_ports(db) -> Vec<u16>` + `:459-462` `fn exits_enabled(db) -> bool` —— **同样的代码**

风险：改一处忘记另一处，启动时的出口池与设置页显示的出口状态就会不一致。

- 严重度：**P2 / 建议改**。代价：把 `read_exit_ports`/`exits_enabled` 提到 `api/src/state.rs`（或新建 `api/src/config_ext.rs`）并 `pub(crate)`，`AppState::new` 调它们。改 2 个文件约 30 行，风险低。

### 1.7 过时注释 3 处（不影响运行，但会误导下一个改代码的人）

| 位置 | 注释内容 | 实际情况 |
|---|---|---|
| `api/src/state.rs:38-39` | 「放在这里（而非 app 的 main）是因为 app crate 不直接依赖 tagger」 | app → api → tagger，app **间接依赖** tagger；且 `AppState::new` 本来就在 api 里，这段解释的理由不成立 |
| `api/src/routes/settings.rs:69` | 「api crate 无 reqwest 依赖」 | 同 crate 的 `images.rs:337`/`:380`、`replace.rs:94`、`dict.rs:129` 都在用 `reqwest` |
| `api/src/routes/tagging.rs:674-701` | `run_tag_pipeline_async` 注释称「把同步 DB 操作包进 spawn_blocking」 | 函数体（`:686-699`）只是 `moevault_tagger::run_tag_pipeline(...).await`，**没有任何 spawn_blocking** |

- 严重度：**P3 / 可以不动**（纯注释）。但 `settings.rs:69` 那条建议顺手改掉 —— 因为它正在为一个**手写裸 TCP HTTP 客户端**（`:71-97`，`TcpStream` + `read_to_end` + `split("\r\n\r\n").nth(1)`）提供「不得不这样」的假理由，会误导后续维护者继续维护这段代码。

---

## 2. 重复代码

### 2.1 导入/查询的筛选 SQL 手写两遍，且都不复用现成的 `build_filter_conds`（**P1**）

`db/src/lib.rs` 里已经有一个覆盖 16 个筛选维度的动态 SQL 构造器：

- `build_filter_conds`（`db/src/lib.rs:2158-2298`，141 行），支持 tags/exclude_tags/q/date_from/date_to/aesthetic_min/aesthetic_max/aesthetic_include_unscored/clarity_min/clarity_max/source/sauce_status/format/min_width/min_height/is_redundant/is_ai/tagged
- 但**只有 2 个调用点**：`list_images_filtered`（`:125`）与 `count_images_filtered`（`:174`）

而两个导入相关的方法各自手写了一遍类似的筛选条件：

- `import_tree`（`:324-396`）：`:335-367` 手写 sauce(3 分支)/tag(3 分支)/ai(2 分支) 的字符串条件拼接
- `import_dir_images`（`:401-491`）：`:424-456` **与上面逐字相同**

第三份同样的表达式藏在 `build_filter_conds` 内部：`:2254-2263` 的 `sauce_status` 分支又是一次 `is_sauced`/`is_ai` 条件构造。

**合计 3 处相同的 sauce/tag/ai 条件逻辑。**

- 严重度：**P1 / 应该改**。代价：把 `:335-367` 与 `:424-456` 的公共部分抽成 `fn sauce_tag_ai_conds(...) -> (String, Vec<Value>)`，3 处共性调用。改 1 个文件约 90 行，风险中（SQL 字符串拼接出错是运行期错误，编译期发现不了，**必须有测试或手工对拍**）。

### 2.2 任务启动模板 6 份（**P0 附带真实缺陷**）

6 个任务启动点结构逐段相同：

1. `api/routes/aesthetic.rs:35-107` `run_aesthetic`
2. `api/routes/aesthetic.rs:128-187` `rescore_image`
3. `api/routes/tagging.rs:459-547` `run_tagging`
4. `api/routes/tagging.rs:550-672` `run_sauce`
5. `api/routes/tagging.rs:763-837` `retag_image`
6. `api/routes/tasks.rs:163-278` `resume_task`

每份都重复：读配置 → `create_job`（spawn_blocking 包 `db.create_job(ty, payload)`）→ 取 `sauce_pool`/`exit_pool` → `tokio::spawn` → `start_job` + `add_log` → 跑 pipeline → `match &result` → `update_job` → 构造 task 事件 JSON → `broadcast`。

实测重复点：`create_job` 在 `aesthetic.rs:49`/`:139`、`tagging.rs:487`/`:577`/`:784`；`start_job` 在 `aesthetic.rs:72`/`:157`、`tagging.rs:503`/`:613`/`:804`、`tasks.rs:239`；`update_job` 在 `aesthetic.rs:87`/`:172`、`tagging.rs:527`/`:639`/`:822`、`tasks.rs:263`。

**task.done / task.failed 的 `json!` 模板 10 段**（5 处 × 2 分支）：`aesthetic.rs:95`/`:99`、`:175`/`:179`、`tagging.rs:535`/`:539`、`:660`/`:664`、`:825`/`:829`、`tasks.rs:266`/`:270` —— 逐字近似，只有 `kind` 与 `total` 字段不同。

#### ⚠️ 这里不只是重复，是一个**可观测的用户级缺陷**

「用户中断任务后，worker 收尾时把 `cancelled` 覆盖成 `done`/`failed`」的防护，**只有 2 处实现**：

- `tagging.rs:633-638`（run_sauce）：`let final_status = if db.get_job(job_id).ok().flatten().map(|j| j.status) == Some("cancelled".into()) { "cancelled" } else { status };`
- `tasks.rs:257-262`（resume_task）：同型判断

**缺失的 4 处**：

| 位置 | 收尾行号 | 后果 |
|---|---|---|
| `tagging.rs` run_tagging | `:523-543` | 打标任务被中断 → 状态被覆盖成 done/failed |
| `tagging.rs` retag_image | `:814-836` | 单图重打标被中断 → 同上 |
| `aesthetic.rs` run_aesthetic | `:83-104` | 美学评分被中断 → 同上 |
| `aesthetic.rs` rescore_image | `:166-184` | 单图重评分被中断 → 同上（且不写 add_log） |

用户视角：点了「中断」，任务中心里那条任务过几秒自己变成「已完成」。`cancel_task`（`tasks.rs:141-159`）只把 job 置 `cancelled`，靠 worker 轮询感知（`pipeline.rs:631-638`），worker 确实停了，但收尾写库把状态改回去了。

- 严重度：**P0 / 必须改**。
- 代价：最小修复 —— 把 `tagging.rs:633-638` 那 6 行抽成 `fn final_status(db, job_id, status) -> &'static str`，6 处收尾全部改调它。**改 3 个文件（tagging.rs / aesthetic.rs / tasks.rs）约 8 处**，风险低（纯替换，逻辑已验证过的）。
- 顺带收益：`final_status == "done"` 时触发 `run_auto_replace_for_images`（`tagging.rs:641-652`）这个行为也只有 run_sauce 有，顺手确认其他任务是否本该也有。

### 2.3 任务模板重复的根治方案（P2，做在 2.2 之后）

抽一个 `api/src/task_runner.rs`：

```rust
// 伪代码，示意形状
pub struct TaskSpec<'a> { kind: &'a str, payload: Value, ... }
pub async fn run_task<F, Fut>(state, spec: TaskSpec, body: F) -> ApiResult<Value>
where F: FnOnce(Arc<ApiKeyPool>, Option<ExitPool>) -> Fut, Fut: Future<Output = Result<Progress, AppError>>
```

6 个启动点各自只保留「自己的 body 闭包」+「自己的 payload 构造」。预计 `aesthetic.rs` 从 187 行降到约 90 行，`tagging.rs` 从 837 降到约 700。

- 严重度：**P2 / 建议改**。代价：新建 1 个文件约 80 行，改 3 个文件删约 250 行。风险中：`tokio::spawn` 内捕获的生命周期、`AppState` 的 `Clone` 语义容易写错，需要编译期反复调。**建议先做 2.2 的 6 行提取，观察一个迭代周期后再决定要不要做这一步。**

### 2.4 `thumbUrl` 4 份实现 —— 见 §5.5

2 定义（`stores/library.ts:39-42`、`stores/dedup.ts:33-36`）+ 2 内联（`LibraryView.vue:617`、`ImportDirectoryView.vue:489`）。**P2**。

### 2.5 `now_secs()` 两份同名同实现

- `db/src/migration.rs:28-33`
- `db/src/lib.rs:2300-2305`

两处都是 `SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)`。migration 是 db 的子模块，本可直接调父模块的。

- 严重度：**P3 / 可以不动**。理由：6 行函数，重复的维护成本近乎零，抽出去反而要处理 `pub(crate)` 可见性。触发条件：等有第三处需要时再抽。

### 2.6 手写裸 TCP HTTP 客户端（P2，与过时注释绑定）

`api/src/routes/settings.rs:70-97` `fn proxy_python_get(base: &str, path: &str) -> Result<String, String>`：

```rust
let mut stream = TcpStream::connect(...)?;      // 裸 TCP
stream.set_read_timeout(Some(Duration::from_secs(3)))?;
stream.write_all(request.as_bytes())?;
let mut buf = Vec::new();
stream.read_to_end(&mut buf)?;                   // 读到 EOF
let text = String::from_utf8_lossy(&buf);
text.split("\r\n\r\n").nth(1).unwrap_or("")   // 手切 header/body
```

在 http://127.0.0.1 的场景下能工作，但：不支持 chunked 编码、不处理 `Transfer-Encoding`、`set_read_timeout` 3 秒对慢推理服务偏短、HTTP/1.1 keep-alive 会让 `read_to_end` 挂到超时。

同 crate 已有 `reqwest` 在用（`images.rs:337`/`:380`、`replace.rs:94`、`dict.rs:129`），完全没有理由手写。

- 严重度：**P2 / 建议改**。代价：`proxy_python_get` 改为 `reqwest::Client` 的 `get().send().await?.text().await?`，约 30 行 → 8 行；`proxy_json`（`:114-128`）随之简化。改 1 个文件，风险低（但需要真实 Python 推理服务在跑来验证）。

### 2.7 重复代码排查中「可以不动」的部分

- **`api/src/routes/mod.rs` 的 4 个错误响应函数**（`error_response`/`app_error_response`/`db_error_response`/`join_error_response`）看起来像重复，实际职责不同（原始字符串 / AppError / DbError / JoinError 四种输入）。**不要合并**。真要动，是删掉零调用的 `app_error_response`（§4.1）。
- **`el-tab-pane` 6 个 tab 的模板结构相似**：那是 Element Plus 的常规用法，重复的是标签不是逻辑，**可以不动**。

## 3. 超长函数与超长文件

### 3.1 超长文件（>800 行）——实测 `wc -l`

| 文件 | 行数 | 内聚问题 |
|---|---|---|
| `backend/crates/db/src/lib.rs` | **2708** | 见 3.2，单个 `impl Db` 占 2064 行 |
| `frontend/src/views/ImageDetailView.vue` | **1895** | 见 3.3，8 类互不相关职责同处一文件 |
| `frontend/src/views/SettingsView.vue` | **1587** | 见 3.4，6 个独立功能页共享一份 ref 命名空间 |
| `src-tauri/src/lib.rs` | **1222** | Tauri 壳层：窗口/托盘/IPC/Python 推理环境管理/GPU 安装 |
| `backend/crates/api/routes/images.rs` | **973** | 列表/详情/原图/缩略/相似/代理/统计/导出 8 组 handler |
| `frontend/src/views/LibraryView.vue` | **889** | 列表+分页+框选+批量面板+视图切换 |
| `backend/crates/tagger/src/pipeline.rs` | **873** | 打标管线 + 溯源管线 + 自动替换编排三套流程 |
| `backend/crates/api/routes/tagging.rs` | **837** | 5 个任务启动点 + key 管理 + 代理 + 状态查询 |
| `frontend/src/views/ImportDirectoryView.vue` | **802** | 导入向导 + 目录树选择 + 第三套批量动作实现 |

**其余 >200 行文件**（供参考，不单独列为问题）：`tagger/keypool.rs` 685、`ingest/importer.rs` 664、`api/routes/settings.rs` 559、`TaskCenterView.vue` 489、`ImageWall.vue` 453、`ImageCard.vue` 440、`ingest/features.rs` 425、`tagger/saucenao.rs` 398、`SearchFilter.vue` 396、`api/routes/export.rs` 366、`DedupView.vue` 353、`core/models.rs` 344、`DragImport.vue` 337、`dedup/cluster.rs` 336、`tagger/exitpool.rs` 316、`src-tauri/file_ops.rs` 315、`api/routes/dedup.rs` 309、`stores/library.ts` 300、`api/routes/replace.rs` 300、`ExportDialog.vue` 286、`api/routes/tasks.rs` 278、`src-tauri/drag_drop.rs` 268、`api/routes/dict.rs` 257、`api/routes/import.rs` 236、`TopBar.vue` 227、`stores/tasks.ts` 226、`tagger/booru.rs` 214、`TrashView.vue` 213、`constants/batchActions.ts` 208、`DashboardView.vue` 207、`tagger/aesthetic.rs` 205。

### 3.2 `db/src/lib.rs`：单个 `impl Db` 块 = 2064 行

- `backend/crates/db/src/lib.rs:75` `impl Db {` 一直到 `:2138` 结束，**2064 行的单个 impl 块**，内含 **95 个 `pub fn`**（`grep -c "    pub fn "` = 95）。
- 95 个方法横跨 9 个业务域：图库查询、导入批次、去重分组、回收站、设置 KV、标签、AI 元数据、任务（job）、日志。**一个类型承担了整个持久层的全部表操作**。
- 自由函数只有 5 个（`:2140` row_to_job、`:2158` build_filter_conds、`:2300` now_secs、`:2308` month_start_secs、`:2331` row_to_item），说明**绝大部分逻辑都塞在 impl 里而不是拆成模块**。
- 直接后果（已有实证，见 §2）：`build_filter_conds`（`:2158-2298`，141 行）已实现完整动态筛选能力，但 `import_tree`（`:324-396`）与 `import_dir_images`（`:401-491`）**完全不知道它的存在**，各自手写一遍同样的 SQL 条件 —— 这是「文件太大导致作者找不到已有工具」的典型症状。
- 严重度：**P2 / 建议改**。代价：把 95 个方法按域拆成 `db/src/{images,dedup,recycle,settings,tags,jobs,logs}.rs` 各自的 `impl Db` 块（Rust 允许跨模块 `impl`），**只移动代码不改签名**，风险低。预计改 1 个文件拆成 8 个，diff 大但纯机械；可分批做（先拆 jobs+logs，约 500 行，收益最直接）。

### 3.3 `ImageDetailView.vue` 1895 行 = 8 类职责

文件切分：script `1-1047` / template `1048-1361` / style `1362-1895`。script 内实测 32 个 ref/computed/watch。8 类职责及行号：

1. **图片导航**：`:20` image、`:158` orderedIds、`:166` indexInList、`:168` gotoImage、`:177` onKeydown
2. **全屏模式**：`:92` openFullscreen、`:99` fsGoto、`:113` preloadAround、`:188` enterFullscreen、`:27` fullscreen、`:30` prevSrc、`:86-90` fsSrc/fsImgReady/fsPrevSrc
3. **标签分组展示**：`:131` tagGroups、`:154` hasAnyTags
4. **AI 元信息**：`:22-25` aiInfo/aiTags/aiChecked、`:224` readAiInfo、`:251` toggleAiMark
5. **回收**：`:268` recycle
6. **标签编辑器（`:784-991`，208 行）**：`:805` enterEditMode、`:811` exitEditMode、`:818` toggleTagDelete、`:823` startEditTag、`:833` commitEditTag、`:845` applyTagEdit、`:854` stopEditTag、`:858` addNewTag、`:867` startNewTagEdit、`:873` commitNewTagEdit、`:878` removeNewTag、`:885` applyTagChanges；另有 `:933-1000` 的 rawTags 文本编辑（`:937` buildRawTagsText、`:947` openRawTagsDialog、`:953` parseRawTags、`:974` applyRawTags）
7. **溯源与联网替换**：`:518` trySauce、`:532` checkSauceReplace、`:567` doReplaceFromNet、`:699-783` 对比弹窗（`:708` openCompare、`:738` proxyUrl、`:744` cmpColor、`:750-755` localPixels/netPixels/sizeColor/pxColor、`:757` keepNetwork）
8. **文件操作与动画定位**：`:583` exportImage、`:624` openExplorer、`:633` editSourceUrl、`:653` renameImage、`:673` displaySourceUrl、`:680` openSourceUrl、`:293` zoomingOut、`:304` lookupTargetRect、`:331` goBack（内部 167 行）

**关键点**：第 6 类（标签编辑器，208 行 + 4 个 ref 组）与第 7 类（溯源对比弹窗，84 行）与「详情页」本身**没有任何耦合**，是纯粹的独立功能被顺手塞进了当前唯一打开的页面。它们各自完全可以抽成 `TagEditorPanel.vue` / `SauceCompareDialog.vue`。

- 严重度：**P1 / 应该改**。代价：抽 2 个组件（`TagEditorPanel.vue` `:784-1000` 约 217 行 script + 对应 template 段；`SauceCompareDialog.vue` `:699-783` 84 行），主文件降到约 1500 行。改动 1 改 3，风险中（标签编辑涉及 applyTagChanges 的状态回收，需人工回归）。

### 3.4 `SettingsView.vue` 1587 行 = 6 个功能页共享一份 ref 命名空间

文件切分：script `1-997` / template `998-1483` / style `1484-`。实测 44 个 ref/computed/watch —— **全前端密度最高**。

template 侧 `el-tab-pane` 6 个（已分 tab，这一步做对了）：`:1001` 通用(library)、`:1053` SauceNAO、`:1079` 本地推理(inference，**单页 177 行**)、`:1258` 回收站/sidecar(misc)、`:1271` 标签(tags)、`:1302` BUG追踪器(logs)。

但 script 侧**没有对应拆分**，5 个互不相关领域各成一块、共用同一个顶层作用域：

| 领域 | 行范围 | 关键符号 |
|---|---|---|
| Sauce 密钥管理 | `:33-38`、`:57`、`:381-460` | newKey/newKeyName/newKeyTier/keys/manageVisible、onKindSelect、loadKeys、editQuota、removeKey、loadKeyStatuses、addKey、openManage |
| 本地推理服务 | `:64-89`、`:115-228` | loadInferHealth、gpu* 一组、showGpuTutorial、copyGpuTutorial、refreshGpuStatus、installGpuEnv、onInferStart、onInferStop、shellDeps、loadShellDiagnostics、onInstallDeps |
| 出口池 | `:260-374` | loadExits、saveExits、testExits、clashTemplate、showClashTemplate |
| 字典导入 | `:533-567` | loadDictStatus、fmtDictBytes、fmtDictTime、importCnDict |
| 日志查看器 | `:618-948` | 定时器、loadLogs、tracker、dumpLogsBackend、clearLogs、exportLogs、formatLogMsg、过滤态、搜索、onModelSelect、设备加载 |

- 严重度：**P1 / 应该改**。代价：按 tab 拆 6 个 `settings/*.vue` 子组件（SauceKeysPanel / InferPanel / ExitsPanel / DictPanel / LogsPanel / GeneralPanel），1 改 7。风险中：日志查看器的定时器（`:646-696`）在 `onUnmounted` 里必须清理，拆分时容易漏掉导致定时器泄漏 —— 拆完要在 6 个 tab 之间来回切换验证。

### 3.5 `pipeline.rs:537` `run_sauce_pipeline` = 267 行，全项目唯一 >200 行函数

已用括号配平脚本全量实测，**>200 行的函数只有 1 个**：`backend/crates/tagger/src/pipeline.rs:537` 的 `run_sauce_pipeline`，267 行（`:537-803`）。

>100 行的共 16 个：`saucenao.rs:83` search_file_with 184、`src-tauri/lib.rs:41` run 152、`db/lib.rs:2158` build_filter_conds 141、`api/routes/dict.rs:120` import_dict 139、`api/routes/tagging.rs:550` run_sauce 123、`pipeline.rs:405` sauce_one 122、`api/routes/export.rs:247` run_export 120、`src-tauri/lib.rs:697` ensure_infer_venv 117、`api/routes/tasks.rs:163` resume_task 116、`src-tauri/lib.rs:820` start_infer 115、`api/routes/search.rs:41` search_suggest 110、`ingest/importer.rs:144` process_one 108、`db/lib.rs:1442` browse_tags 102、`db/lib.rs:2445` filter_by_aesthetic_range 101（测试内）、`src-tauri/lib.rs:1019` infer_install_gpu 101。

`run_sauce_pipeline` 之所以 267 行，是因为它同时承担 5 件事：① 不可溯源标记清理（`:577-579`）；② 队列 + 进度共享状态构造（`:583-590`）；③ worker 数量决策（`:585-604`，含 key×exit 的注释性推导）；④ worker 循环体内的 4 个结果分支各自维护进度（`:680-800`，其中「锁 progress → 自增 → drop → update_job」三连逐字重复 4 次）；⑤ 中断响应（`:631-638` 轮询 db.get_job，`:658-675` 用 `tokio::time::timeout(1s, pool.acquire())` + select）。

- 严重度：**P2 / 建议改**。代价：抽 3 个东西即可从 267 降到约 150——`fn spawn_workers(...)`（③④）、`fn bump_progress(&progress, &db, job_id)`（消除 4 份三连）、`async fn acquire_with_abort(&pool, &db, job_id)`（⑤）。改 1 个文件，风险低（纯提取，无逻辑变更）。
- **不建议**为了「降到 50 行」而硬拆 ④ 的分支 —— 4 个分支的差异（Ok(Some)/Ok(None)/RateLimited/Err）是业务语义差异，过度抽象反而更难读。这里的目标是消除**逐字重复的进度更新**，不是消灭分支。

## 4. 错误处理

### 4.1 结论速览

后端的错误类型**设计是好的**（`core/src/error.rs` 有 `ErrorKind` + `AppError` + `status_code()` 映射），但**没人用它**：

- `app_error_response`（`backend/crates/api/src/routes/mod.rs:34-36`）—— **零调用点**。全库 grep 只命中定义处这一行。统一错误映射函数写好了，恒温放着。
- 与之配套的 `impl From<DbError> for AppError`（`backend/crates/db/src/lib.rs:31`）—— **api 层从不使用**。db 方法返回 `DbError` 95 处、返回 `AppError` **0 处**；api 层用 `db_error_response` 手动把 `DbError` 转成 `ErrorKind::Db`（88 处）。

也就是说：**一套完整的错误抽象（AppError + From 转换 + app_error_response）建好了，实际走的却是「每个 handler 手写 (StatusCode, Json<Value>) 元组」的另一套**。

### 4.2 `Result<Json<Value>, (axum::http::StatusCode, Json<Value>)>` 手写 77 次

全量 grep 精确匹配 **77 次**，按文件分布：

| 文件 | 次数 |
|---|---|
| `api/routes/tagging.rs` | 18 |
| `api/routes/images.rs` | 13 |
| `api/routes/settings.rs` | 11 |
| `api/routes/dedup.rs` | 6 |
| `api/routes/import.rs` | 5 |
| `api/routes/tasks.rs` | 5 |
| `api/routes/logs.rs` | 4 |
| `api/routes/trash.rs` | 4 |
| `api/routes/aesthetic.rs` | 3 |
| `api/routes/replace.rs` | 3 |
| `api/routes/dict.rs` | 2 |
| `api/routes/export.rs` | 2 |
| `api/routes/search.rs` | 1 |

另有 12 处**同型但非 `Json<Value>`** 的返回签名，散落各处：`dedup.rs:54` parse_cursor、`trash.rs:30` parse_cursor、`images.rs:44`/`:362` Response、`:789` parse_sort、`:808`/`:861` parse_redundant/parse_ai、`:834` build_filter、`:876` parse_include_unscored、`:949` Json<Stats>、`tagging.rs:384` Arc<ApiKeyPool>。

- 问题：加一个 handler 就要再抄一遍这串 41 字符的类型名；想给错误响应加字段（如 `request_id`、`trace`）要改 77 处。
- 严重度：**P1 / 应该改**。代价：在 `api/src/routes/mod.rs` 加 `pub type ApiResult<T> = Result<T, ApiError>;` 并 `impl IntoResponse for ApiError`，然后 77 处签名替换为 `ApiResult<Json<Value>>`。**纯机械替换**，可 `sed` 批量；风险低。改 14 个文件，但每处只动一行签名。

### 4.3 `self.conn.lock().unwrap()` —— 141 处，Mutex 中毒即全库 panic

`Db` 定义（`backend/crates/db/src/lib.rs:71-73`）：

```rust
pub struct Db { conn: std::sync::Arc<Mutex<Connection>> }
```

**95 个方法，141 处 `.lock().unwrap()`**（`grep -c` 实测）。这是全库最大的 panic 面。

失效链条：任一 handler 在持锁期间 panic（例如 `row.get()` 越界、整数转换）→ Mutex 中毒 → **此后每一次 db 调用都 panic** → 整个后端事实上死亡，但进程还活着，用户看到的是「所有操作都报错」而非「服务挂了」。

- 严重度：**P1 / 应该改**（不是 P0，因为目前没有已观测到的触发点；但它把「一次局部错误」放大成「全局不可用」，风险杠杆极高）。
- 代价：三种改法，推荐第 2 种：
  1. `.lock().unwrap_or_else(|e| e.into_inner())` —— 忽略中毒继续用。一行改动 ×141，但可能用上已损坏的连接。
  2. **改 `parking_lot::Mutex`** —— 其 `lock()` 不返回 `Result`，**没有中毒概念**，141 处 `.unwrap()` 直接删掉即可。加 1 个依赖，改 141 行删除式改动，编译期就能全部验证。风险低。
  3. 换 `r2d2`/`deadpool` 连接池 —— 顺带解决 §6 的连接串行化，但改动大（Db 结构变、所有方法签名可能变）。不建议本轮做。
- 另注：`db/lib.rs:2300` `now_secs()` 用 `.unwrap_or(0)` 是安全的；`migration.rs:28-33` 有一份**同名同实现的 `now_secs()`**，属重复（见 §2）。

### 4.4 同一表达式内的双重 `map_err`（spawn_blocking 惯例错误）

固定模式：`spawn_blocking(...).await.map_err(...)??` —— 外层 map_err 处理 JoinError，内层 `?` 处理业务错误。当作者把内层也写成一个 map_err 时，同一行出现两次错误转换：

- `backend/crates/api/src/routes/images.rs:307`：`.map_err(|e| error_response(ErrorKind::Internal, format!("任务失败: {e}")))??;`
- `backend/crates/api/src/routes/images.rs:738`：`.map_err(|e| error_response(ErrorKind::Internal, e.to_string()))?;`
- `settings.rs:122-123`、`trash.rs:109-110`、`export.rs:207-208`、`tagging.rs:758-759` 同型。

不是正确性错误，但说明这层惯例**没有被封装**：每个作者都要重新想一遍「哪层处理 JoinError、哪层处理 DbError」，于是写歪。这正是 §4.2 那个 `ApiResult` + 一个 `fn blocking<T>(f) -> ApiResult<T>` 助手该解决的问题。

- 严重度：**P2 / 建议改**，与 4.2 合并做。代价：加 1 个泛型助手函数，改约 30 处调用点。

### 4.5 内部错误原文回给前端 —— 信息泄漏

`error_response(ErrorKind::Internal, e.to_string())` 把底层错误原样序列化进 HTTP 响应体。`tagging.rs:326/333/336/355/360/365/368/372` 连续 8 处这样写。

SQLite 的错误信息包含**表名、列名、约束名**，部分包含 SQL 片段。虽然本应用是本地单机（`core/src/config.rs:10` host 默认 `127.0.0.1`），泄漏面小，但：

- 用户截图报 bug 时会把内部 schema 带出去；
- 违背 `ErrorKind` 的设计意图 —— `Internal` 本该是「对用户不可解释」的错误。

- 严重度：**P2 / 建议改**。代价：`Internal` 分支改为返回固定文案 + 完整错误写 `tracing::error!`（日志已在 `api/src/lib.rs:12-34` 的 `error_log_middleware` 落库），改 1 个函数 + 约 20 处调用点。

### 4.6 Tauri 壳层 16 处 unwrap/expect —— panic 即整个桌面应用崩溃

`src-tauri/src/lib.rs`（1222 行）实测 16 处，**全部在 GUI 主进程，无任何兜底**：

| 行号 | 代码 | 崩溃场景 |
|---|---|---|
| `:87` | `.expect("默认窗口图标缺失")` | 打包资源缺失 |
| `:129` | `.expect("托盘图标创建失败")` | 系统托盘不可用 |
| `:145`/`:169`/`:214`/`:224`/`:953`/`:963`/`:984` | `slot.lock().unwrap()` | 任一子线程持锁 panic 后，主线程全部 panic |
| `:152` | `BACKEND_URL.parse().unwrap()` | 常量错拼即启动崩溃 |
| `:182` | `.expect("error while building tauri application")` | 窗口创建失败 |
| `:236`/`:377`/`:403` | `&format!("127.0.0.1:{BACKEND_PORT}").parse().unwrap()` | 常量错拼 |
| `:825`/`:1008` | 裸 `.unwrap()` | 见对应上下文 |

对比：`src-tauri/src/drag_drop.rs`（268 行）、`file_ops.rs`（315 行）、`main.rs`（6 行）**unwrap/expect 计数均为 0** —— 说明这个代码库的作者是能写好的，问题集中在 `lib.rs` 这 1222 行的壳层里。

- 严重度：**P1 / 应该改**。理由：`slot.lock().unwrap()` 那 7 处是「一次子线程 panic → 主进程永久不可用」，与 §4.3 的 db Mutex 是同一类杠杆。`parse().unwrap()` 那 4 处是对**编译期常量**的解析，实际触发概率低。
- 代价：`parse().unwrap()` × 4 改为 `parse().expect("字面量合法")` 或直接 `const` 构造（几乎零风险）；`slot.lock()` × 7 改为 `match`/`if let` 并从异常分支返回错误给前端（需读懂每个 slot 的语义，风险中）。改 1 个文件，约 11 处。

### 4.7 正面结论（不要过度改造）

- `panic!`/`todo!`/`unimplemented!` 全库**仅 1 处**：`backend/crates/tagger/src/booru.rs:85` 的 `_ => unreachable!()`（match 已穷举其他分支，可保留）。
- `core/src/error.rs` 73 行的错误模型本身设计合理，`status_code()` 映射（NotFound→404、InvalidInput→422、Config|Internal→500、Db|External→502）语义正确。
- `ingest`/`dedup` 的 unwrap 几乎全在 `cfg(test)` 内（importer.rs 42 处、features.rs 15、cluster.rs 13、scan.rs 6），**不是生产风险**。
- `db_error_response`（88 处）、`join_error_response`（24 处）确实在承担实际的错误转换工作，不是死代码。

## 5. 前端状态管理

### 5.1 `library` store 还不是「上帝对象」，但已经是「共享可变草稿本」

`frontend/src/stores/library.ts` 300 行，职责：图库数据（images/total/cursor）、筛选态（filters）、选择态（selected）、视图态（viewMode/waterfallColumns）、详情页上下文（viewerContext）、导出。**规模上不至于失控**（对比 `SettingsView.vue` 单个文件的 44 个 ref，store 只有 300 行）。

真正的问题是**它维护了一份与 `useSelection` 重复的选择真源**，且同步方式错了。

### 5.2 选择态双真源 + 双写 + 清空错位（**P0**，已有可观测 bug）

三处实现同一件事：

| 位置 | 内容 |
|---|---|
| `stores/library.ts:74` | `const selected = ref<Set<number>>(new Set())` |
| `stores/library.ts:231-236` | `toggleSelect` |
| `stores/library.ts:238-240` | `clearSelect` |
| `stores/library.ts:121-123` | 注释：「selected 仍保留供视图读取（由 useSelection 同步写入）」 |
| `composables/useSelection.ts` | 179 行，`selected`/`anchor`/`count`/`isEmpty`/`applySelection`/`selectRange`/`selectAll`/`clear`/`setSelection`/`addToSelection`/`removeFromSelection`/`toggle`/`isAllSelected`/`isPartial`/`shrinkToVisible` |

**双写实证**（同一同步做两遍）：

- `views/LibraryView.vue:399-402` `onSelect`：`sel.applySelection(...)` 之后紧跟 `library.selected = sel.selected.value`
- `views/LibraryView.vue:428-434` 又有一个 `watch(() => sel.selected.value, s => { library.selected = new Set(s) })`

即：**一次点击 → 先同步赋值一次 → 再被 watch 赋值一次**。

**清空错位实证**（这是真 bug，不只是冗余）：

- `views/LibraryView.vue:623-628` `onExportDone` 只调 `library.clearSelect()`，**没有调 `sel.clear()`**
- 后果：导出/回收完成后，`library.selected` 空了，但 `sel.selected` 仍持有旧集合 → 批量操作面板仍显示「已选 N 张」，用户再点批量动作会作用在**已经不在列表里的 id** 上

**依赖这份副本的消费方**：`views/LibraryView.vue:311` `const selectedCount = computed(() => library.selected.size)` —— 即 UI 显示的选中数走的是副本，不是真源。

- 严重度：**P0 / 必须改**。这是用户可观测的状态不一致。
- 代价：**删除** `library.ts:74` 的 `selected`、`:231-236` `toggleSelect`、`:238-240` `clearSelect`，以及 `LibraryView.vue:399-402` 的同步赋值和 `:428-434` 的整个 watch；`LibraryView.vue:311` 改为 `computed(() => sel.count.value)`；`:623-628` 改为 `sel.clear()`。**改动 2 个文件约 6 处，全是删除**，风险低。这是本次审查里性价比最高的修复。

### 5.3 `useSelection` 只覆盖 1/3 的视图 —— 重构没走完

grep 实测：**`useSelection` 只有 1 个使用点** —— `views/LibraryView.vue:18` 引入、`:37` `const sel = useSelection(() => visibleImageIds())`。

另外两个视图各有一套平行实现：

- **`views/ImportDirectoryView.vue`**：`:61` `const selected = ref<Set<number>>(new Set())`、`:84` selectedCount、`:197-208` toggleDirSelect、`:210-214` isDirAllSelected、`:228-233` toggleSelect、`:257`/`:496` 手动 `selected.value = new Set()`、`:204-207`/`:534-547` 手动 new Set 复制。**且没有 anchor 概念（无 Shift 范围选择）**。
- **`views/TagManageView.vue:219`**：独立的 `toggleSelect`。

也就是说，抽出来的统一选择模型只服务了一个视图，其余视图仍在各自实现 —— **重构的收益没兑现，反而多了一份需要同步维护的实现**。

- 严重度：**P1 / 应该改**。代价：`ImportDirectoryView.vue` 换用 `useSelection`（它同时有目录树选择和图片选择两层，需要两个 `useSelection` 实例），改 1 个文件约 10 处。风险中：需确认目录树的「全选子项」语义能否映射到 `applySelection`。

### 5.4 `batchActions` 注册表只被右键菜单用，工具栏是第三套实现

`constants/batchActions.ts` 208 行，9 个动作 key：`tag`(:88)/`aesthetic`(:96)/`sauce`(:105)/`ai-detect`(:117)/`copy-image`(:139)/`copy-path`(:155)/`export`(:170)/`recycle`(:180)。

- **用上了**：`components/ContextMenu.vue:36` `const actions = computed(() => actionsFor('context'))`
- **没用上**：`views/ImportDirectoryView.vue` 的工具栏是第三套 —— `:428` `const batchActions = ref<string[]>([])`、`:443-475` `onExecuteBatch` 手写 `const order = ['ai-detect','sauce','tag','aesthetic']` + switch 分支、`:502-520` `onExecClick` 自建「两击确认」（注册表已有同样机制）、`:602-620` 模板手写 5 个 `el-option` + 条件 `el-checkbox`。执行体是 `:236-320` 的 onBatchDelete / onBatchTag / onBatchAesthetic / onBatchSauce / onBatchDetectAi —— 与注册表的 tag/aesthetic/sauce/ai-detect/recycle **5 个动作语义完全重叠**。

注意：`ImportDirectoryView.vue:701-708` 的右键菜单**确实注入了注册表所需的全部 props**（options/refresh/open-export/clear-selection/resolve-paths）—— 说明注册表在这个页面**已经可用**，只是工具栏没换过去。

- 严重度：**P1 / 应该改**。代价：删 `:236-320` 的 5 个方法 + `:443-475` 的 switch + `:602-620` 手写模板，改调 `actionsFor('toolbar')` + `action.run(ctx)`。改 1 个文件，删多于加，风险低。

### 5.5 `thumbUrl` 4 份实现（2 定义 + 2 内联）

函数体逐字相同：`/thumbs/${thumbRel.replace(/\\/g, '/')}`

| 位置 | 签名 |
|---|---|
| `stores/library.ts:39-42` | `thumbUrl(thumbRel: string \| null \| undefined)` |
| `stores/dedup.ts:33-36` | `thumbUrl(thumbRel: string \| null)` |
| `views/LibraryView.vue:617` | 内联 `i.thumbRel ? \`/thumbs/${i.thumbRel.replace(/\\\\/g, '/')}\` : ''` |
| `views/ImportDirectoryView.vue:489` | 同上内联 |

引用方也随之分裂：library 组（`DashboardView.vue:6`、`ImageCard.vue:4`、`ImagePreview.vue:3`）与 dedup 组（`DedupView.vue:5`、`TrashView.vue:6`）。

- 严重度：**P2 / 建议改**。代价：抽到 `utils/thumbUrl.ts`，改 6 个引用方 + 删 2 处定义 + 换 2 处内联。改 9 个文件但每处 1-2 行，风险低。

### 5.6 任务轮询生命周期 —— 离开任务中心后全局轮询永久停止（**P0**）

- `stores/tasks.ts:115-121`：`function start() { load(); if (timer !== undefined) return; timer = window.setInterval(load, 3000) }`；`:124-130` `stop()` 清 timer。
- **两个组件各自管理同一份全局 timer**：`AppLayout.vue:16-17`（onMounted start / onUnmounted stop）与 `TaskCenterView.vue:10`/`:16`（同样 start/stop）。

失效链条：进入任务中心 → TaskCenterView 调 `start()`（timer 已存在，直接 return，无害）→ 离开任务中心 → TaskCenterView 的 `onUnmounted` 调 `stop()` → **全局 timer 被清掉**，而 AppLayout 不会重新 start → 此后任务完成不再有任何通知，直到用户再次进入任务中心。

- 严重度：**P0 / 必须改**（用户级功能失效）。
- 代价：两种改法。（a）`start/stop` 加引用计数；（b）**更简单**：`TaskCenterView` 不再调 `stop()`，只调 `start()`（幂等），轮询生命周期完全交给 `AppLayout` 单点管理。推荐 (b)，改 1 个文件 2 行。

### 5.7 WS 事件被前端丢弃，后端也丢

- `frontend/src/api/ws.ts:43` 只判断 `batch.done`/`batch.failed`；后端已广播的 `task.done`/`task.failed`/`dedup.updated` **全部被丢弃** → 这才是 §5.6 用 3 秒轮询兜底的根因（有实时事件通道却不用，改用轮询）。
- `backend/crates/api/src/routes/ws.rs:64`：`Err(broadcast::error::RecvError::Lagged(_)) => continue` —— 慢客户端积压事件被**静默丢弃**，无补偿、无重同步。

- 严重度：**P1 / 应该改**（两处合起来才是完整问题：前端不用事件 + 后端丢事件）。代价：`ws.ts` 加一个通用的事件分发 switch（约 20 行），`task.done` 到达时刷新任务列表 → 可以**把 3 秒轮询降级为兜底**（保留但拉长到 30 秒）；`ws.rs:64` 在 Lagged 时向前端发一条 `resync` 事件要求全量刷新。改 2 个文件，风险低。

### 5.8 设置项的 4 处手工映射

`backend/crates/api/src/routes/settings.rs:24-56` 白名单 30 个 key（含 `pagination_enabled`、`page_size`、`sauce_proxy_enabled`、`sauce_proxy_ports`）。

前端 `stores/settings.ts` 里同一批字段手工出现 **4 次**：interface 定义 + `defaults` + `load()` 的 27 行转换（含 `log_clear_on_start: s.log_clear_on_start === true || s.log_clear_on_start !== 'false'` 这类三态兼容）+ `save()` 的 27 行 `String()` 序列化。

实测漏项：`page_size` 在**后端白名单里有、前端完全没有**。即：后端支持配置页大小，前端没有 UI，也没有读写 —— 死配置。

- 严重度：**P2 / 建议改**。代价：抽一个 `settingsSchema`（数组：key / 类型 / 默认值 / 序列化函数），`load`/`save` 遍历它。改 2 个文件约 100 行 → 30 行，风险中（三态布尔转换的语义要逐字段确认）。

### 5.9 正面结论

- `stores/dedup.ts`（93 行）结构干净：groupCount/redundantCount/involvedImages/groups/loading + refreshStats/fetchGroups/groupDetail/scan/resolve。**唯一问题就是多导出了那份 thumbUrl**。
- 路由缓存策略是对的：`router/index.ts` 给 library 与 imports 标 `meta.keepAlive: true`，`AppLayout.vue:81-85` `<keep-alive :include="['library','imports']">`；`ImageDetailView`/`SettingsView`/`TaskCenterView` 不缓存。重列表页缓存、重表单页重建，符合预期。

## 6. 性能隐患

### 6.1 结论速览

后端整体的 `spawn_blocking` 使用是**认真的**（api 路由层合计 87 处：tagging 23、images 16、settings 10、dedup 9、import 7、tasks 7、replace 6、aesthetic 5、trash 5、logs 4、export 2、search 2、dict 1）。但有一处系统性漏洞：**`tagger` crate 内部 0 处 `spawn_blocking`，却在 async 上下文里跑 51 处同步 SQLite 调用**（`pipeline.rs` 46 处 + `aesthetic.rs` 5 处）。

### 6.2 `tagger` 内整条流水线在 async 里跑阻塞 SQLite（**P1**）

- `tagger/src/pipeline.rs`：46 处 `db.` 同步调用，`spawn_blocking` **0 处**
- `tagger/src/aesthetic.rs`：5 处 `db.` 同步调用，`spawn_blocking` **0 处**

对比：`api/routes/tagging.rs` 里有 23 处 `spawn_blocking`，说明作者知道要包 —— 但包在了错误的层（路由层包一次，进到 pipeline 后又是裸同步调用）。

后果：tokio 的 worker 线程被 SQLite 的 `Mutex` 阻塞（见 `db/src/lib.rs:71-73` 单连接全局 Mutex）。溯源流水线最多 12 个 worker（`pipeline.rs:598-604`：`(key_count * exit_count).min(12)`），每个 worker 处理完一张图就要写 `db.update_job` + `db.add_log` —— 12 个 task 同时抢一把 SQLite 锁，**全部卡在 tokio worker 线程上**。

好在：worker 之间有网络请求（SauceNAO 上传+查询）作为天然的让出点，所以不会完全饿死 runtime；但 `db.update_job` 每张图都写一次（见 6.3），锁竞争频率很高。

- 严重度：**P1 / 应该改**。代价：两条路。（a）pipeline 内每个 `db.xxx` 调用包 `spawn_blocking`（46 处，机械但会让函数签名全变 async，扩散大）；（b）**推荐**：把 db 层的写操作批量化（见 6.3），天然把 46 次降到 2-3 次。改 1-2 个文件，风险中。
- 注：`api/routes/tagging.rs:674-701` 的 `run_tag_pipeline_async` 注释声称做了这件事，但函数体没有任何 `spawn_blocking`（见 §1.7）。

### 6.3 每张图写一次 job 行 —— 进度更新的写放大（**P1**）

`pipeline.rs` 的 4 个结果分支各自执行同一套三连（`:688-704` Ok(Some)、`:706-716` Ok(None)、`:746-752` RateLimited 耗尽、`:763-769` Err）：

```rust
let mut p = progress.lock().unwrap();
p.done += 1;
let (d, f) = (p.done, p.failed);
drop(p);
if let Some(jid) = job_id {
    db.update_job(jid, "running", d as i64, f as i64, None);  // 每张图一次 SQLite 写
}
```

理论上 1000 张图 = 1000 次 `UPDATE jobs`，每次都抢全局 Mutex。而进度数据本来就是给 UI 轮询看的（3 秒一次，见 §5.6），**完全不需要每张图都落库**。

同理 `pipeline.rs:233-299` 的 `run_tag_pipeline` 是**串行 for 循环逐张处理**（无并发），每张还调 `db.add_log` + `db.update_job` 两次写。

- 严重度：**P1 / 应该改**。代价：进度改为「内存计数 + 每 N 张（如 20）或每 500ms 落库一次」（节流）；`run_tag_pipeline` 的 `add_log` 改为批量。改 1 个文件（`pipeline.rs`）约 30 行，风险低。
- 顺带：这个改动也能缓解 6.2 的锁竞争。

### 6.4 N+1 查询 3 处

| 位置 | 模式 |
|---|---|
| `pipeline.rs:183-220` `filter_eligible` | 对每个 id 调 `db.get_image_by_id` + `db.image_tags` → **2N 次查询**，且被 `run_tag_pipeline:250` 与 `run_sauce_pipeline` **各调一次** |
| `pipeline.rs:313-322` / `:417-423` | `sauce_one` 与 `tag_one` 各自调 `db.get_image_by_id` + `db.image_tags`；`tag_one` 调 `sauce_one` 时**重复查同一张图** |
| `api/routes/images.rs:84+` `similar_images` | spawn_blocking 内先 `db.all_active_images()` 全量载入，再 `for iid in ids { db.get_image_by_id(iid) }` |
| `api/routes/export.rs:247-366` `run_export` | `with_tags` 时每张再查 `db.image_tags` |

`db.images_by_ids` 已存在（可见于方法清单），完全可以用它一次性取回，但上面这些都用了逐张 `get_image_by_id`。

- 严重度：**P1 / 应该改**（`filter_eligible` 与 `similar_images`）/ **P2**（`run_export` 的 with_tags）。代价：`filter_eligible` 改用 `images_by_ids` + 一次批量查 tags，改 1 个文件约 40 行；`similar_images` 同理。风险低（行为等价）。

### 6.5 全表载入内存

`db/src/lib.rs:632-644` `all_active_images()`：

```sql
SELECT id, phash, clarity_score FROM images WHERE status='active' ORDER BY id
```

无 LIMIT、无分页，把整个图库的 (id, phash, clarity) 读进 `Vec`。调用方：`similar_images`（`images.rs:84+`）与去重扫描。

1000 张图的 phash（16 字节）+ id + f64 ≈ 30KB/千张，**当前规模完全可接受**。但它是线性增长的，且去重扫描本来就是 O(N²) 的比对本性 —— 一起看，见 6.6。

- 严重度：**P3 / 可以不动**（当前规模下无问题）。触发条件：图库 > 5 万张时，改为分块载入（`LIMIT/OFFSET` 或按 id 区间滚动）。

### 6.6 去重是 O(N²) 内存比对

`dedup/src/cluster.rs`（336 行）对 `all_active_images()` 的结果两两比 phash 汉明距离。N=1000 时 50 万次比较（毫秒级）；N=50000 时 12.5 亿次（分钟级，且全在 spawn_blocking 的一个线程里，UI 卡死）。

- 严重度：**P3 / 可以不动**。触发条件：图库规模上到万级时，改用 BK-tree 或分桶（phash 前 8 位分桶）。现在改是过度设计。

### 6.7 每次请求新建 `reqwest::Client`（**P2**）

实测新建点：`images.rs:337`/`:380`、`replace.rs:94`、`dict.rs:129`、`exitpool.rs:97`/`:133`、`saucenao.rs:57`、`pipeline.rs:58`/`:442`（`:442` 是 `reqwest::Client::new()` 兜底）。

`Client` 内部持有连接池。每次新建 = 每次重新 TLS 握手（尤其 `rustls-tls-native-roots` 要读系统证书），对「逐张图代理下载」这种高频场景是明确浪费。

讽刺的是：`pipeline.rs:49` 有 `http: reqwest::Client` 字段、`:82` 有 `pub fn http(&self)` 暴露共享客户端，但 **api 层不复用它**。

- 严重度：**P2 / 建议改**。代价：在 `AppState` 里放一个 `http: reqwest::Client`（`state.rs` 加 1 字段 + `new` 里 build 一次），7 处新建点改为 `state.http.clone()`（`Client` 的 `clone` 是 Arc 语义，廉价）。改 8 个文件，风险低。

### 6.8 整文件读进内存（P2）

- `api/routes/export.rs:247-366` `run_export`：`:287-291` `File::open(&src).and_then(|mut f| { let mut buf = Vec::new(); f.read_to_end(&mut buf)?; Ok(buf) })` —— **逐张整文件进内存**，无流式。
- `api/routes/images.rs:359-415` `proxy_image`：`:405-411` `resp.bytes().await` 整图进内存，再 `Body::from(bytes)`（`:413`）。
- 反例（做对了的）：`images.rs:44-71` `get_original_file` `:64` 用 `Body::from_stream(tokio_util::io::ReaderStream::new(file))` —— **全项目唯一正确的流式下发**。

一张 50MB 的 PSD/PNG，导出时 50MB 常驻；并发导出多张会叠加。当前是单任务串行导出，风险有限。

- 严重度：**P2 / 建议改**。代价：`run_export` 改用 `tokio_util::io::ReaderStream`（`images.rs:64` 已有现成写法可抄），`proxy_image` 改 `Body::from_stream(resp.bytes_stream())`。改 2 个文件约 20 行，风险低。

### 6.9 前端性能

- **重复渲染**：`stores/library.ts` 的 `selected` 双写（§5.2）导致每次选择触发 2 次响应式更新链。`LibraryView.vue:311` 的 `selectedCount` 走副本，UI 更新依赖副本的替换时机，可能滞后一帧。
- **3 秒全量轮询**：`stores/tasks.ts:116-120` 每 3 秒一次 `GET /tasks`，不管有没有任务在跑。配合 §5.6 的生命周期 bug（离开任务中心后 timer 被清），实际表现是「有时完全不刷新」。
- **`watch` 深监听**：`LibraryView.vue:428-434` 的 `watch(() => sel.selected.value, ...)` 对 `Set` 做替换式赋值，尚可；但配合 :399-402 的同步赋值，每次点击产生 2 次 Set 全量复制（`new Set(s)`）。
- 严重度：**P1**（轮询 + WS 丢弃，见 §5.6/§5.7）。代价见对应小节。

### 6.10 性能排查中「可以不动」的部分

- `db/src/lib.rs` 单连接 + 全局 Mutex（`:71-73`）：本地单机应用、SQLite 本身也是单写者，**不是瓶颈**。真要动是 `§4.3` 的 parking_lot 迁移，收益主要在 panic 面而非吞吐。
- `api/src/routes/ws.rs` 的 `broadcast::channel(256)`（`state.rs:34`）：256 条缓冲对本地单用户足够。
- `LibraryView.vue:98-107` 的游标分页：`paginationOn` 时用 `pageCursors` 游标（正确的做法），关闭时 `fetchImages()` 默认 `limit=200`（一次性 200 张，可接受）。
- `ImageDetailView.vue:113` `preloadAround` 预加载相邻图片：合理的体验优化，不是问题。

---

## 7. 测试覆盖

### 7.1 实测数据

| 指标 | 数值 |
|---|---|
| `cfg(test)` 模块 | **11 个**（全部是 src 内联，无独立 tests 目录除 1 个） |
| `#[test]` + `#[tokio::test]` | **51 个** |
| 前端测试框架 | **0**（无 vitest/jest，`package.json` 无 `test` 脚本） |
| `api` crate 测试 | **0** |
| `app` crate 测试 | **0** |
| `src-tauri` 测试 | **0** |

51 个测试分布：`keypool.rs` 11、`db/lib.rs` 8、`importer.rs` 6、`features.rs` 5、`booru.rs` 4、`saucenao.rs` 3、`phash.rs` 3、`clarity.rs` 3、`cluster.rs` 3、`scan.rs` 2、`exif.rs` 2、`tagger/tests/exitpool_proxy.rs` 1。

`db` 的 8 个测试函数：`test_db`、`migrate_is_idempotent`、`stats_empty_library`、`insert_ai_source_tag`、`list_images_empty_and_filtered`、`filter_by_aesthetic_range`、`browse_tags_category_and_cover`、`suggest_tags_prefix_and_escape`、`import_cn_dict_fills_missing_only`（起点 `db/src/lib.rs:2354`）。

### 7.2 覆盖的形状：只覆盖「纯函数 + 单表 CRUD」

现有 51 个测试全部落在：phash 汉明距离、exif 解析、clarity 打分、booru URL 解析、keypool 状态机、dedup 聚类、migration 幂等、单表 insert/query。

**完全零覆盖的三条主干**：

1. **`api` 层全部 15 个 route 模块** —— 所有 handler、错误映射、参数校验、状态码。77 个手写 `Result<...>` 签名没有一个被验证过。
2. **`tagger/src/pipeline.rs`（873 行，全部业务编排）** —— 打标管线、溯源管线、中断机制、并发 worker 池、进度更新，**零测试**。这是项目最复杂的文件。
3. **前端全部** —— 选择模型、批量动作、任务轮询生命周期、WS 事件处理。

### 7.3 `db` 层 95 个 pub 方法里未覆盖的高风险部分

有测试的 8 个方法之外，未覆盖的包括：

- **`insert_images`**（批量插入，导入主路径）
- **去重全套**：`create_dedup_group`/`assign_to_group`/`clear_dedup`/`set_group_best`/`set_group_state`（`cluster.rs` 的 3 个测试测的是内存聚类算法，不测落库）
- **回收站全套**：`insert_recycle_bin`/`delete_recycle_bin`/`list_recycled`（**涉及文件移动，失败会丢用户图片**）
- **任务全套**：`create_job`/`start_job`/`update_job`/`cancel_job`/`resume_job`
- **日志全套**、**`sauce_cache`**、**`replace_pending`**

其中**回收站**最危险：它是唯一「DB 记录 + 磁盘文件」双写且需要一致性的地方，零测试。

### 7.4 现有测试的一个实际问题

`backend/crates/tagger/tests/exitpool_proxy.rs`（48 行，唯一独立测试文件）**需要真实 SauceNAO 可达**。在无网络的 CI 里会失败或被 `#[ignore]`。即：唯一的集成测试是一个依赖外部网络的用例。

### 7.5 建议的补测顺序（按投入产出比）

| 顺序 | 目标 | 理由 | 代价 |
|---|---|---|---|
| 1 | `db` 回收站 3 个方法 | 唯一会丢用户数据的地方 | ~80 行测试 |
| 2 | `db` 任务全套 5 个方法 | 支撑 §2.2 的中断修复 | ~100 行 |
| 3 | `api` 冒烟测试（axum `oneshot`） | 覆盖健康检查 + 列表 + 设置读写 3 条最常用路径 | ~120 行，需加 `tower::ServiceExt` |
| 4 | 前端加 vitest，测 `useSelection` | 它是 §5.2/§5.3 修复的基础 | ~60 行 + 配置 |
| 5 | `pipeline` 的 `final_status` 逻辑 | §2.2 的 P0 修复需要防回归 | ~40 行 |

前 3 项做完，就覆盖了「会丢数据」和「用户最常用」的两类风险。

- 严重度：**P1 / 应该改**。当前 51 个测试的质量不差（都是真断言，不是空壳），问题是**覆盖面偏科**，避开了所有跨层、有状态、涉及 IO 的路径。

### 7.6 关于 AGENTS.md 的构建门槛

`AGENTS.md` 要求「提交前跑通 `cargo test --workspace` 与 `npm run build`」。前者能跑（51 个测试通过），后者能跑。但 `cargo test` 的通过**不能证明**上面三条主干是好的 —— 这正是「绿灯但高风险」的典型。

- 建议：在 `AGENTS.md` 的提交门槛里加一句「改了 `db` 回收站/任务、`api` 路由签名、前端选择模型时，需手工回归对应流程」。这是 P3（流程建议），不改代码。

## 8. 结论与建议执行顺序

### 8.1 Routing Decision（本次审查的定性）

- **审查类型**：architecture-hotspot-scan + focus:architecture，**只读**。未修改任何源码文件，唯一写入是本文档。
- **覆盖范围**：backend 44 文件 / 13397 行全量静态阅读 + 定向取证；frontend 37 文件 / 11347 行（重点 3 个大文件全读，其余按职责取证）；src-tauri 5 文件 / 1814 行。
- **审计结论**：**架构骨架健康，问题集中在「重构半途」与「并发收尾」两类。** 依赖图无循环、api 层无 SQL 直写、错误模型设计合理 —— 这些是真实优点，不要推倒重来。需要修的是 4 个 P0（都是小而确定的改动）和 5 个 P1（都是「重构已经起了个头但没走完」）。

### 8.2 Findings 汇总（按严重度）

#### P0 / 必须改（4 项，全部是小改动）

| # | 问题 | 证据 | 代价 |
|---|---|---|---|
| **P0-1** | 任务中断后状态被覆盖成 done/failed | `tagging.rs:523-543`、`:814-836`、`aesthetic.rs:83-104`、`:166-184` 缺 `final_status` 判定；只有 `tagging.rs:633-638` 与 `tasks.rs:257-262` 有 | 抽 6 行函数，改 3 文件 8 处 |
| **P0-2** | 选择态双真源 + 清空错位，批量面板显示过期选中数 | `library.ts:74`/`:231-240` 与 `useSelection` 并存；`LibraryView.vue:399-402` 同步赋值 + `:428-434` watch 双写；`:623-628` 只清副本不调 `sel.clear()` | 删 6 处，改 2 文件 |
| **P0-3** | 离开任务中心后全局任务轮询永久停止 | `stores/tasks.ts:115-130` + `AppLayout.vue:16-17` 与 `TaskCenterView.vue:10/16` 双向 start/stop | 改 1 文件 2 行 |
| **P0-4** | 前端丢弃 `task.done`/`task.failed` WS 事件，后端 `Lagged` 时静默丢事件 | `api/ws.ts:43` 只认 `batch.*`；`ws.rs:64` `Lagged(_) => continue` | 改 2 文件约 25 行 |

> P0-1 到 P0-4 全部可以在**一次会话内**完成，总改动约 6 个文件、不到 60 行。这是本次审查的最高优先级建议。

#### P1 / 应该改（5 项）

| # | 问题 | 证据 | 代价 |
|---|---|---|---|
| **P1-1** | `tagger` 内 51 处同步 SQLite 调用无 `spawn_blocking` | `pipeline.rs` 46 处、`aesthetic.rs` 5 处，两文件 `spawn_blocking` 计数 0 | 与 P1-2 合并做 |
| **P1-2** | 每张图写一次 job 行（写放大 + 锁竞争） | `pipeline.rs:688-704`/`:706-716`/`:746-752`/`:763-769` 四份三连 | 节流落库，改 1 文件 30 行 |
| **P1-3** | 任务启动模板 6 份重复，10 段 `json!` 模板 | 见 §2.2 | 先做 P0-1，再做模板抽取 |
| **P1-4** | `Result<Json<Value>, (StatusCode, Json<Value>)>` 手写 77 次；`app_error_response` 零调用 | §4.2、§4.1 | 加 `ApiResult` alias，机械替换 77 行 |
| **P1-5** | 重构只做了一半：`useSelection` 只服务 1 个视图，`batchActions` 只被右键菜单用，工具栏是第三套 | `LibraryView.vue:37` 唯一使用点；`ImportDirectoryView.vue:236-320`/`:443-475`/`:602-620` 平行实现 | 改 1 文件（ImportDirectoryView），删多于加 |

#### P2 / 建议改（9 项）

| # | 问题 | 证据 |
|---|---|---|
| P2-1 | `db` 筛选 SQL 手写 3 处，不复用 `build_filter_conds` | `db/lib.rs:335-367`、`:424-456`、`:2254-2263` |
| P2-2 | `self.conn.lock().unwrap()` 141 处，中毒即全库 panic | `db/lib.rs` 全文件 → 改 `parking_lot::Mutex` |
| P2-3 | `db/src/lib.rs` 单 impl 2064 行 / 95 个 pub 方法 | `:75-2138` |
| P2-4 | `ImageDetailView.vue` 1895 行含 8 类职责，标签编辑器 208 行可独立 | `:784-1000`、`:699-783` |
| P2-5 | `SettingsView.vue` 1587 行，5 个领域共享 ref 命名空间 | `:33-38`/`:64-89`/`:260-374`/`:533-567`/`:618-948` |
| P2-6 | 每次请求新建 `reqwest::Client`（7 处），已有共享 client 不用 | `images.rs:337/380` 等 |
| P2-7 | 整文件读进内存（`run_export`、`proxy_image`），而 `get_original_file` 有正确流式写法 | `export.rs:287-291`、`images.rs:405-413` vs `:64` |
| P2-8 | 出口池配置双解析源 | `state.rs:38-64` vs `settings.rs:445-462` |
| P2-9 | 手写裸 TCP HTTP 客户端 + 过时注释 | `settings.rs:69-97` |

#### P3 / 可以不动（附理由）

| 问题 | 为什么不动 |
|---|---|
| `thumbUrl` 4 份 | 4 行函数，抽取要动 9 个文件，收益低（若做 §5 其他改动时顺带） |
| `now_secs()` 两份 | 6 行，重复成本近零 |
| `all_active_images()` 全表载入 | 千张约 30KB，当前规模无问题；>5 万张再说 |
| 去重 O(N²) 比对 | 万级以下毫秒级；现在改是过度设计 |
| `build_filter_conds` 141 行 | 它就是**被正确复用的那个**（2 个调用点），长但不该动 |
| `run_sauce_pipeline` 267 行 | 抽 3 个助手降到 150 即可，不要为行数硬拆 4 个业务分支 |
| `db` 的 `browse_tags`/`month_start_secs` 越界 | 自包含逻辑，搬家不改行为；等第二次被调用再动 |
| `api` 依赖 `tagger` 领域类型 | 下游依赖，方向合法；加抽象层是负收益 |
| `routes/mod.rs` 4 个错误响应函数 | 职责不同，不要合并 |
| 过时注释 3 处 | 纯注释；但 `settings.rs:69` 那条建议随 P2-9 一起改 |

### 8.3 Refactor or Slimming Signals（该动 / 不该动）

**该动（信号明确）**：

- 一个函数/文件被改时**总是要同时改另外 N 处**（任务模板 6 份、设置项 4 处映射、thumbUrl 4 份）→ 说明缺少接缝，抽取收益确定。
- 同一 bug 在不同副本里**修了 2 处漏了 4 处**（P0-1）→ 重复代码已经产生过真实缺陷，这是最强的重构信号。
- 已存在的工具**不被使用**（`build_filter_conds` 只 2 个调用点、`images_by_ids` 不用、`pipeline.rs:82` 的共享 `http()` 不用、`app_error_response` 零调用）→ 说明模块的接口没被发现，属「深模块表面太浅」。
- 重构**只覆盖 1/3 使用点**（`useSelection` / `batchActions`）→ 半成品比没做更贵（两份实现要同步维护）。

**不该动**：

- 所有「只有 1 个调用点、逻辑自包含、6-40 行」的东西（`now_secs`、`month_start_secs`、4 个错误响应函数、`browse_tags` 的封面规则）。
- 已经**正确复用**的长函数（`build_filter_conds` 141 行，2 个调用点）。
- 依赖方向合法的地方（api → tagger）。
- 当前规模下不成问题的地方（全表载入、O(N²) 去重、256 缓冲）。

### 8.4 Do Not Touch Yet（有理由暂缓）

1. **`db` 改连接池（r2d2/deadpool）**：会改 `Db` 结构和所有 95 个方法签名，风险远超收益。单机 + SQLite 单写者，连接池解决不了实际问题。**§4.3 只做 `parking_lot` 迁移就好。**
2. **把 `db/lib.rs` 拆成 8 个文件**（P2-3）：纯机械但 diff 巨大，会与任何并行的 db 改动冲突。**建议只在需要大改某个域时顺手拆该域**（如要做 §7.5 的回收站测试，先拆 `recycle.rs`）。
3. **`SettingsView.vue` 拆 6 个组件**（P2-5）：日志查看器的定时器在 `onUnmounted` 清理（`:646-696`），拆分时极易漏掉导致泄漏。**需要一次完整的人工回归（6 个 tab 来回切换）**，不要在没有验证条件时动。
4. **`pipeline.rs` 的 4 个结果分支抽象**：它们是业务语义差异（Ok(Some)/Ok(None)/RateLimited/Err），过度抽象会降低可读性。**只消除逐字重复的进度更新即可。**
5. **前端引入状态管理重构（如 Pinia 分模块重划）**：当前 store 规模（library 300 行、tasks 226、settings 165、dedup 93）远未到需要重划的程度。**只需修 P0-2/P0-3 的具体缺陷。**

### 8.5 Verification Gaps（本次审查未覆盖 / 无法证明的部分）

必须诚实标注，以下均为**未验证**，不能当作结论：

1. **无运行时证据**。所有结论基于静态阅读，没有启动应用、没有跑测试、没有抓真实请求。P0-1（中断覆盖状态）与 P0-3（轮询停止）的失效链条是**从代码推出的**，未在真实 GUI 上复现。
2. **前端渲染性能未测**。§6.9 的「重复渲染」是读代码推断，没有 profiler 数据。`selectedCount` 的「可能滞后一帧」尤其未验证。
3. **SQL 相关改动（P2-1）无测试保护**。`import_tree`/`import_dir_images` 的条件拼接如果改错，编译期发现不了，现有 51 个测试也覆盖不到。**改这项前必须先补测试或准备手工对拍。**
4. **`tagger` 的锁竞争是推断**。§6.2 说「12 个 worker 抢一把 SQLite 锁」，但没有实测 tokio worker 线程的阻塞时长。实际影响取决于 SauceNAO 的网络延迟（天然让出点），可能远小于推断。
5. **未审查的代码**：`ingest/features.rs`（425 行）、`tagger/saucenao.rs`（398 行）、`tagger/keypool.rs`（685 行）、`dedup/cluster.rs`（336 行）只做了针对性 grep，未逐行阅读。这几处有测试覆盖（共 22 个测试），风险相对低但**不等于已审查**。
6. **`src-tauri` 的 IPC 契约未逐一核对**：`lib.rs` 1222 行里的 `#[tauri::command]` 与前端 `invoke` 调用点是否一一对应，本次未做交叉验证。
7. **数据库 schema 演进未评估**：`db/src/migration.rs`（66 行）的迁移策略、是否有回滚路径，不在本次范围内。
8. **未验证 `AGENTS.md` 的构建门槛实际执行情况**：「提交前跑 `cargo test --workspace`」是约定，历史提交是否真的都跑了，未查 git log。

### 8.6 Gate Recommendation（建议的放行条件）

**当前状态**：不建议在 P0 未修的情况下继续做新功能。理由：P0-1/P0-2/P0-3 都是**用户会直接看到的状态错误**，新功能会叠加在这些不可靠的状态之上（尤其是任务中心和选择态，几乎所有批量功能都依赖它们）。

**建议的执行顺序**（每步都可独立交付）：

| 阶段 | 内容 | 改动量 | 验证方式 |
|---|---|---|---|
| **第 1 步** | P0-1 ~ P0-4 全修 | 6 文件 / <60 行 | 手工回归：① 启动打标任务→中断→看任务中心状态；② 导出后看批量面板选中数；③ 进任务中心再离开→跑任务看是否收到通知 |
| **第 2 步** | P1-4（`ApiResult` alias）+ P2-8（出口池解析去重）+ §1.7 的三条过时注释 | 14 文件签名替换 + 2 文件 | `cargo build` 通过即证明替换无遗漏（签名不匹配会编译失败） |
| **第 3 步** | §7.5 的前 2 项补测（回收站 + 任务），然后做 P1-1/P1-2（pipeline 落库节流） | 新增约 180 行测试 + 改 1 文件 | `cargo test` + 手工跑一次 100 张以上的溯源任务，对比 DB 写入次数 |
| **第 4 步** | P1-5（`ImportDirectoryView` 换用 `useSelection` + `batchActions`） | 改 1 文件，删多于加 | 手工回归：目录树全选/反选/Shift 范围选择、5 个批量动作 |
| **第 5 步** | 视需求做 §3 的拆分（`ImageDetailView` 抽 2 组件、`SettingsView` 拆 6 组件） | 各 1 改 3 / 1 改 7 | 必须有完整人工回归；`SettingsView` 要在 6 个 tab 间来回切换验证定时器 |

**放行判据**：第 1 步完成后，P0 全部关闭即可继续正常迭代；第 2-4 步可穿插在功能间隙做；第 5 步只在「这些文件正好要被改」时才做。

### 8.7 Residual Risk（剩余风险）

即使执行完上述建议，以下风险仍然存在：

1. **测试覆盖的结构性缺口不会很快补上**。51 个测试全部集中在纯函数层，即使补了 §7.5 的 5 项，`api` 层与前端仍是黑洞。**这个缺口会长期存在**，缓解方式是人工回归清单（§7.6 的建议），不是等测试补齐。
2. **`pipeline.rs` 的并发正确性没有测试能保护**。中断、worker 池、进度更新三者交互，任何改动都只能靠手工验证。**这是项目最脆弱的单点**，873 行、零测试、承载最多业务编排。
3. **单连接 + 全局 Mutex 的架构不变**。`parking_lot` 解决了 panic 传染，但「一个慢查询阻塞所有请求」的特性保留。当前单用户本地场景可接受，**多用户或高频并发场景下会是硬瓶颈**。
4. **前端选择模型的统一只完成 1/3**。即使做了 P1-5，`TagManageView.vue:219` 仍有独立实现；且 `ImportDirectoryView` 的目录树选择与图片选择是两层语义，能否完全映射到 `useSelection` 需实测确认。
5. **`build_filter_conds` 与手写 SQL 的共存不会消失**。P2-1 若不做，三份条件构造会继续分裂；若做，编译期无保护（字符串拼接），需要人工对拍。
6. **wx/Tauri 壳层的 panic 面仍在**（§4.6）。`slot.lock().unwrap()` 7 处的修复需要理解每个 slot 的语义，本次未逐一定位，改动风险高于收益不确定。
7. **所有结论基于 HEAD `dc805eb`**。工作区有 `?? test/` 未跟踪目录，若其中有未提交改动，本报告未覆盖。

---

## 附录：本次审查的方法与工具

- `wc -l` 全量行数统计（backend / frontend / src-tauri 逐文件）
- 自写括号配平脚本统计函数长度（识别 >100 行共 16 个、>200 行 1 个）
- `grep -c` 精确计数：`unwrap/expect`、`spawn_blocking`、`Result<Json<Value>, (axum::http::StatusCode, Json<Value>)>`、`pub fn`、`#[test]`
- `grep -n` 定位所有关键调用点，`sed -n` 读取指定行段
- 逐 `Cargo.toml` 核对依赖图
- 全文件精读：`api/src/state.rs`、`api/src/routes/mod.rs`、`core/src/error.rs`、`stores/dedup.ts`、`useSelection.ts`、`constants/batchActions.ts`、`api/ws.ts`、`pipeline.rs:183-803`
- **未执行**：`cargo build`/`cargo test`/`npm run build`（只读审查，未验证编译状态）

---

## 9. 后续跟进记录（2026-11-14）

> 本节由后续会话补写，**不属于原审查结论**，只记录「按 §8 建议动手之后实际发生了什么」，供下一次审查对账。原报告的 P0/P1/P2 定性保持原样，未回改。

### 9.1 P1 六项的落地结果

| 编号 | 原条目 | 实际状态 | 说明 |
|---|---|---|---|
| P1-1 | §2.1 筛选 SQL 手写三份 | ✅ 已完成 | 收敛为唯一 `fn sauce_tag_ai_conds(sauce, tag, ai) -> Vec<String>`，3 个调用点（`import_tree` / `import_dir_images` / `build_filter_conds`）全部改为调用。新增回归测试 `sauce_tag_ai_filter_consistent_across_entrypoints` 做三入口对拍 |
| P1-2 | §3.3 ImageDetailView 拆分 | ✅ 已完成 | 1895 → 1326 行；抽出 `constants/tagGroups.ts`、`utils/format.ts`、`components/detail/TagEditPanel.vue`（445 行）、`components/detail/SauceCompareDialog.vue`（242 行） |
| P1-3 | §3.4 SettingsView 拆分 | ✅ 已完成 | 1587 → 154 行；拆成 `views/settings/` 下 6 个面板。**关键坑**：原 `.hint` 是 scoped 样式，拆出去的子组件拿不到，必须改成带 `.settings-page` 前缀的非 scoped 块 |
| P1-4 | §4.2 手写 `Result<Json<Value>, …>` 77 次 | ✅ 早已完成 | 复核时 `routes/mod.rs:30` 已有 `pub type ApiResult<T> = Result<Json<T>, ApiError>;`，全 routes 命中 92 处。**本报告估的「77 次待改」在写作时就已经不是现状** |
| P1-5 | §4.3 `lock().unwrap()` 141 处 | ✅ 已完成 | 实测是 **90 处**（不是 141）。改 `parking_lot::Mutex` 后 90 处 `.unwrap()` 全部删除，锁中毒语义从根上消失 |
| P1-6 | §4.6 Tauri 壳层 16 处 unwrap/expect | ✅ 已完成 | `src-tauri/src/` 下 `unwrap()/expect(` 现已 **0 命中**。新增 `child_slot()` / `infer_start_guard()` 两个辅助函数收敛 7 处 `slot.lock()`；4 处 `parse().unwrap()` 改 `SocketAddr::from(([127,0,0,1], PORT))`；`tauri::Builder::build` 失败改为打印原因后 `exit(1)` |

### 9.2 P2 评估结论（只核实，未动手）

按「评估 P2 部分」的要求逐条核了现状，**其中两项已经不存在了**：

| 编号 | 原条目 | 现状核实 | 建议 |
|---|---|---|---|
| P2-§1.3 | `app_error_response` 零调用 | 仍零调用，仅定义在 `api/src/routes/mod.rs:43` | 保留观察。删它或接通它都只改 1–2 行，收益极小，**不值得单独开一轮** |
| P2-§2.4 | `thumbUrl` 4 份 | **只剩 2 份**（`stores/dedup.ts:33`、`stores/library.ts:39`）。另 2 份在 P0 清理时已删 | 可以合，但两个 store 域不同（图库 vs 查重），合并要处理 props 传递。**收益小，暂缓** |
| P2-§2.6 | 手写裸 TCP 的 `proxy_python_get` | **已不存在** —— 已改用 `reqwest`（`api/src/routes/settings.rs:74`），且 doc 注释明确写了「这里曾是一段手写裸 TCP 客户端…那条注释早就过时了」 | **无需处理** |
| P2-§1.6 | 出口池配置两份真源 | **已不存在** —— 上一轮 P0 已收敛成 `state::exit_config(db) -> (Vec<u16>, bool)`（`api/src/state.rs:40`），`routes/settings.rs` 三处调用点（`:215` / `:433` / `:489`）全部走它，全 crate 已无第二份端口解析 | **无需处理** |
| P2-§3.2 | `db/src/lib.rs` 单 impl 2064 行 | 现 **2816 行**，单 impl 从 `:81` 起 | 拆分（`images/dedup/recycle/settings/tags/jobs/logs`）风险低但**收益也是纯组织性的**。建议等到下次真要改 db 时顺手做，不要专门开一轮 |
| P2-§3.5 | `run_sauce_pipeline` 267 行 | 仍在 `tagger/src/pipeline.rs:537` | **建议做**，但必须先在 §9.3 的「补测试」之后 —— 它零测试，直接改无法验证 |
| P2-§2.3 | 抽 `api/src/task_runner.rs` | 仍无该文件 | 依赖 §2.2（六个任务启动点），§2.2 已部分做了。**可做，排在 §3.5 之后** |
| P2-§4.5 | 内部错误原文回前端 | 仍在（`routes/tagging.rs` 多处 `error_response(ErrorKind::Internal, e.to_string())`） | **建议做**，且成本极低：加一个 `internal_error(e)` 只回「内部错误，详见日志」+ 服务端打日志。这是安全/体验双收益 |

**P2 总结**：8 项里 **3 项已消失**（§2.6 裸 TCP、§1.6 出口池双源，以及 §2.4 的 2 份副本），剩 5 项。按「收益/成本」排序，**只有 §4.5 内部错误脱敏值得单独做**（改 1 个辅助函数 + 替换调用点，安全与体验双收益）；其余 4 项（§1.3 / §2.4 / §3.2 / §3.5 / §2.3）都要等「正好改到那块代码」时顺手做，不专门开轮。

### 9.3 原报告结论的偏差修正

动手过程中发现原报告有 **3 处数字/事实与现状不符**，记录在此以免下次引用时被误导：

1. **§4.3 的「141 处 `lock().unwrap()`」实际是 90 处。** 差 51 处 —— 原报告可能把 `tests` 模块里的也算进去了，或统计口径包含了非 `self.conn` 的锁。
2. **§4.2 的「77 处待改签名」在报告写作时就已经改完了。** 引用这类「机械替换」类结论前应先 `grep` 复核一次。
3. **§4.6 的「16 处 unwrap/expect」分布描述有误。** 原文说 `:145/:169/:214/:224/:953/:963/:984` 共 7 处 `slot.lock().unwrap()`，实际还有 `.expect()` 与 `parse().unwrap()` 若干，逐一定位比按行号清单改更可靠。

**方法论教训**：审查报告里的**行号与计数是最易腐坏的部分**，因为它们是「瞬间快照」。后续会话引用前必须重新 `grep` 验证，否则会照着已经不存在的行号改代码。

### 9.4 Residual Risk 的更新

原 §8.7 的 7 条剩余风险中，第 6 条**已关闭**：

> ~~6. wx/Tauri 壳层的 panic 面仍在（§4.6）。`slot.lock().unwrap()` 7 处的修复需要理解每个 slot 的语义，本次未逐一定位，改动风险高于收益不确定。~~
> → **已解决。** `child_slot()` 把所有 slot 锁统一成「中毒也能取回」的语义，无需逐个理解 slot 内容即可安全收敛。`src-tauri/src/` 下现已零 unwrap/expect。

第 3 条**部分缓解**：

> ~~3. 单连接 + 全局 Mutex 的架构不变。`parking_lot` 解决了 panic 传染，但「一个慢查询阻塞所有请求」的特性保留。~~
> → panic 传染已彻底解决（`parking_lot` 无中毒概念）。「慢查询阻塞」的结构未变，仍是已知特性。

其余 5 条（测试缺口、pipeline 并发、选择模型统一 1/3、SQL 条件构造、结论基线）**全部仍然有效**，其中第 2 条（`pipeline.rs` 零测试承载最多业务编排）在完成 P2-§3.5 之前仍然是最高优先的补强对象。
