# 更新日志

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/) 风格。版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。

## [未发布]

### 修复

- **图库翻页会漏图/重图**（BUG-A）。列表的 ORDER BY 主键是排序字段
  （`imported_at` / `aesthetic_score` / `size_bytes` …），而翻页条件写的是
  `i.id > 游标` —— id 的先后与排序字段无关，于是第 2 页起就跳过了大量本该
  出现的图，降序时甚至一页都翻不出来。
  - 后端改为 **keyset 双列游标**：新增 `ImageCursor { sort_val, id }`
    （`backend/crates/core/src/models.rs`），编码成 `v=<值|~>;i=<id>`
    （`~` 表示该行排序列为 NULL），对前端完全不透明、只做字符串透传。
    续接条件按 ASC/DESC × 排序值是否 NULL 分四支，与
    `ORDER BY <列> <方向>, i.id ASC` 严格对齐。
  - 随机排序没有「可续接的位置」，明确返回 `next_cursor = null`，
    前端把翻页器换成一句说明，而不是给个坏掉的翻页器让人反复翻同一页。
  - 前端 `LibraryView.vue` 的 `fetchPage` 原来还有一个独立缺陷：取游标用的是
    `pageCursors[page]` 而非 `pageCursors[page-1]`，第 2 页永远拿不到游标、
    反复显示第 1 页数据。改为**链式取页**：跳到第 N 页时顺序走完中间页，
    把每页结束游标记进 `pageCursors`；数据中途到底就停在实际最后一页。
  - 排序/筛选一旦变化，旧游标全部失效（它们绑的是上一套结果集）。新增
    `resetPaging()`，`onSortChange` 与所有筛选入口统一走
    `applyFilterAndReset()`，回到第 1 页再拉；升降序按钮改为先翻转再请求
    （原顺序相反，点一下拿到的是上一次的方向）。`onRecycle` 由裸
    `fetchImages()` 改走 `fetchPage()`，避免分页开启时退化成一次性 200 张。
  - 回归测试 `keyset_cursor_pagination_has_no_gaps_or_dups`：12 张图、id 与
    排序值刻意错开、美学分含并列与 NULL，对 5 种「排序 × 方向」组合断言
    「逐页拼接结果 == 一次取全的顺序」，并断言 Random 不返回游标。

- **查重页「保留最优 → 回收站」点了没反应**（显示「已回收 0 张冗余图」，组还在）。
  这不是显示问题，是**整条回收链路在最常见的状态下会失败**：
  - 直接原因：图 2066 的原图**已经在 `data/recycle/` 里**，但 `images.status` 仍是
    `active`、`recycle_bin` 也没有记录。`recycle_image` 的 `move_file` 发现源文件
    不存在 → `std::fs::copy` 报 `IO 错误: 系统找不到指定的文件。(os error 2)` → 整个函数 Err。
  - 这个「文件在回收站、状态还是正常」的半完成态，来源是 `replace_from_url` 与
    `replace.rs` 的**原图替换**：它们把旧文件搬进 `data/recycle/` 却既不写
    `recycle_bin` 也不改状态。而且「旧图可恢复」这个承诺从未成立——
    `recycle_bin.image_id` 是 UNIQUE、id 已被**新内容**占用，旧文件根本没法登记成
    回收站条目。所以每次替换只是在给 `data/recycle/` 扔孤儿文件。
  - 放大成「0 张」的原因：`resolve_group` 逐张 `match recycle_image { Err => warn! }`
    **静默吞掉失败**，随后无条件 `set_group_state(id, "resolved")`，接口只回
    `{"recycled": 0}`；成员筛选条件是 `status='active'`，故组成员看起来毫无变化。

  修复：
  - `recycle_image` 改为**幂等且容忍半完成态**：状态已是 `recycled` 时补齐缺失的
    bin 记录后直接成功；搬运时「源缺失但回收站已有同名文件」按已完成处理，
    「两处都没有」报明确错误（`无法回收：<路径>`）而不是裸 NotFound。
    `restore_image` 同样容忍半完成搬运，`purge_image` 删文件改为 best-effort
    （原先是 `?`，删失败会留下「行还在、文件已删」的更糟残局）。
  - `resolve_group` 不再吞错：收集 `failed` 明细，**只有全部成功才置 resolved**，
    否则组保持 `open` 可重试；响应扩为 `{recycled, failed, errors, group_state}`。
    前端按三种情况分别提示（成功 / 无需处理 / 部分失败并列失败原因）。
  - 原图替换改为「**先落库、后删旧文件**」，并删掉「旧文件移入回收站」那段——
    既然不可恢复，就别再制造孤儿。读原内容的真正途径是 `source_url` 重新下载。

- **回收站里看不见的数据残局（BUG-B / BUG-C）**。库里有 12 张 `status=recycled`
  却没有任何 `recycle_bin` 记录的图（回收站页用 `recycle_bin JOIN images` 查询，
  所以它们**在回收站页完全不显示**），且原图早已不在磁盘——既恢复不了也删不掉；
  同时 `data/recycle/` 堆了 228 个没有任何引用的孤儿文件（46.9 MB）。
  新增 `backend/crates/dedup/src/audit.rs`：`audit_recycle` 按三类明确规则判定
  （半完成 / 僵尸行 / 文件丢失 + 孤儿文件），`repair_recycle` 按规则修复，
  支持 `dry_run` 只报数不落盘。对外提供 `GET /api/v1/trash/audit` 与
  `POST /api/v1/trash/repair {dry_run, remove_orphans}`，回收站页新增
  「一致性检查 / 修复」区块。实机执行结果：识别 13 项问题 + 228 个孤儿，
  修复后 `bin_records=1 / issues=0 / orphans=0`，释放 46,952,974 字节。
  顺带补一条：孤儿文件删完后会留下 256 个空的哈希前缀目录（看着像没清干净），
  `remove_orphans` 时会自底向上收掉空目录（`removed_empty_dirs` 计数）。空目录本身无害，
  故只在显式要求删孤儿时才清，不做隐式清理。
  判定规则写死在代码里、报告逐项列出，**不做任何猜测性删除**。

- **任务中心显示 total=0 而实际处理了 18 张（BUG-D）**。`start_job(job_id, total)`
  在三条管线（打标 / 溯源 / 美学）里都写成
  `force_ids.as_ref().map_or(0, |v| v.len())` —— 走「自动筛选待处理图片」时
  `force_ids` 是 `None`，total 被写死 0，而管线内部实际取的是
  `untagged_active_images(10000)` / `unscored_active_images(10000)`。
  `jobs` 新增 `set_job_total()`，三条管线在算出真实 ids 后（`filter_eligible` 过滤之后）
  立刻回填；启动日志也不再写死「（0 张）」，改为「（指定 N 张）」或「（自动筛选待…图片）」。
  顺手给 `start_job` 加了终态保护（`WHERE status NOT IN ('done','failed','cancelled')`），
  避免刚点中断、管线刚起来时把 `cancelled` 拉回 `running`。
  实测：删掉 3 张图的自动标签制造 3 个候选 → 发起任务 → `total=3`（修复前恒为 0）。

### 开发环境

- **落装 4 个 DSH 插件并完成运行时验证**（详见 `docs/SKILL_AUDIT_2026-11.md` §7）：
  `dsh-sql@0.5.11`（连本项目 `app.db`，只读）、`deepseek-computer-use@0.2.9`
  （Windows 桌面黑盒验收）、`brooks-lint@1.9.1`（6 个代码质量 skill）、
  `aegis`（GitHub 源，23 个工程方法 skill）。
  `dsh-system-monitor-plugin` 经核实只面向用户面板、监控数据不暴露给 Agent，故未装。
  两个本机坑：`pnpm add` 不写 `dsh.profile.bundles`（需手工追加，26 → 30）；
  patch 行 `id` 必须用插件自报导出名（`dsh-sql` → `id: sql`）。

### 重构 / 架构

- **架构审查（`docs/ARCH_REVIEW_2026-11.md`）后的 P0 修复**：四个「有明确失效链条」的问题一次修完。

  - **任务中断后状态被覆盖成 done/failed**。根因比报告描述的更深一层：
    收尾处加保护还不够——worker 每处理几张图会写一次 `running` 进度，
    把 `cancel_job` 写下的 `cancelled` **覆盖回 running**，收尾时再读已读不到 cancelled。
    修复分两处：`Db::update_job` 的 SQL 增加「终态不可被 running 进度拉回」条件；
    各任务入口收尾统一走 `final_job_status()` 回读保护。
  - **选择态双真源**：`stores/library.ts` 的 `selected`/`toggleSelect`/`clearSelect`
    与 `useSelection` 并存且互相 watch 双写，导致批量面板显示过期选中数。
    已删除 store 侧副本，选择态唯一真源为各视图的 `useSelection`。
  - **离开任务中心后全局轮询永久停止**：`tasks.ts` 的 `stop()` 无条件清 timer，
    而 `AppLayout` 与 `TaskCenterView` 都会调 start/stop。改为**订阅者引用计数**，
    最后一个订阅者离开才真正停表。
  - **WS 事件被丢弃**：前端只认 `batch.*`，`task.done`/`task.failed` 全丢；
    后端 `broadcast` 的 `Lagged` 分支静默 `continue`。现在前端消费任务事件并派发
    `moevault:task-event`，后端溢出时下发 `resync` 让各视图重拉权威状态。

- **任务事件统一构造**：新增 `routes::task_end_event()` + `ProgressLike` trait，
  替换此前散落在 5 处、各抄一份的 `json!` 模板。被中断的任务现在发 `task.cancelled`
  而不是 `task.done`，前端用警告色提示「已中断」，不再假装成功。

- **API 响应类型统一**：新增 `ApiResult<T>` / `ApiError` 别名，
  替换 77 处手写的 `Result<Json<Value>, (StatusCode, Json<Value>)>`。

- **出口池配置单一解析入口**：`state::exit_config()` 统一了此前 `state.rs` 与
  `routes/settings.rs` 两份重复的端口/开关解析。

- **推理服务代理改用 reqwest**：`settings.rs` 原有一段手写裸 TCP HTTP 客户端
  （自己拼请求行、按 `\r\n\r\n` 切 body，注释还写着"api crate 无 reqwest 依赖"——
  该依赖其实早已存在）。改后自动支持 chunked/gzip/非 200 状态，并复用连接池。

- **文档整理**：README 的文档索引按「现行 / 历史」分组，补全三份新报告；
  当前状态一节重写为 7 crate 结构说明；五份 v0.1 期旧文档加时效性横幅
  （`PLAN` / `UI_DESIGN` / `TECH_DETAILS` / `ENHANCEMENTS` / `DESIGN_FILTER_NAVIGATION`）；
  `.gitignore` 里 GBK 误解码的中文注释修正为 UTF-8。

- **架构审查 P1 六项一次修完**（`docs/ARCH_REVIEW_2026-11.md` §9.1 记录落地结果）：

  - **筛选 SQL 三份 → 收敛为一份**（报告 §2.1）。`sauce` / `tag` / `ai` 三组条件此前在
    `import_tree`、`import_dir_images`、`build_filter_conds` 里**各手写一遍**（逐字相同的
    三分支 + 二分支），改一处必漏两处。现抽出唯一实现 `fn sauce_tag_ai_conds(...)`，
    三个入口共用；三组谓词提为 `IS_SAUCED` / `IS_AI` / `HAS_AUTO_TAG` 常量。
    新增回归测试 `sauce_tag_ai_filter_consistent_across_entrypoints`：
    插入「有溯源 / 只有 AI 元数据 / 都没有 / 标了无需溯源」四张图，对 8 种筛选组合
    交叉断言「图库计数 == 主目录树合计 == 来源组明细合计」，防止再次发散。
  - **`std::sync::Mutex` → `parking_lot::Mutex`**（报告 §4.3）。`db` 层 90 处
    `self.conn.lock().unwrap()` 的失效链条是：任一请求持锁 panic → Mutex 中毒 →
    此后**每次** db 调用都 panic → 后端事实上死亡但进程还活着（表现为「界面能点，数据全空」）。
    `parking_lot` 无中毒概念，`.unwrap()` 全部删除。
    （顺带更正报告：实测是 90 处，不是报告写的 141 处。）
  - **Tauri 壳层清零 unwrap/expect**（报告 §4.6）。7 处 `slot.lock().unwrap()`
    在子线程 panic 时会让主进程**永久不可用**，现统一走 `child_slot()` 辅助函数
    （中毒时 `into_inner()` 继续用）；4 处 `parse().unwrap()` 改为
    `SocketAddr::from(([127,0,0,1], BACKEND_PORT))`；
    `generate_context!()` 构建失败改为打印原因并 `exit(1)`，不再 panic 出栈。
    现在 `src-tauri/src/` 下 `unwrap`/`expect` **0 命中**。
  - **`ImageDetailView.vue` 1895 → 1326 行**（报告 §3.3）。抽出
    `components/detail/TagEditPanel.vue`（标签编辑状态机，445 行）、
    `components/detail/SauceCompareDialog.vue`（溯源对比，242 行）、
    `constants/tagGroups.ts`（分组规则，被标签栏与编辑弹窗共用）、`utils/format.ts`。
    保存后由子组件 `emit("saved")` 交父组件重拉详情，**保持单一数据源**。
  - **`SettingsView.vue` 1587 → 154 行**（报告 §3.4）。6 个 tab 各拆为独立组件
    （`views/settings/*.vue`），宿主只留 tab 状态、保存、脏检查与离开确认。
    关键坑：原 `.hint` 是 `scoped` 样式，拆出去的子组件内部**收不到**这份样式
    （模板里 37 处引用会全部失去样式），故提升为带 `.settings-page` 前缀的全局块。
    日志面板改为受控 `active` prop 驱动定时器，避免拆出后后台继续轮询。

- **P2 八项经复查只剩 5 项**（记录在 `docs/ARCH_REVIEW_2026-11.md` §9.2）。
  报告里的 §1.6（出口池双源）与 §2.6（手写裸 TCP 客户端）**在 P0 阶段已被修掉**，
  §2.4 的 `thumbUrl` 也只剩 2 份而非 4 份。剩余项中只有「内部错误原文回传前端」
  有实际的用户可见影响，其余属可选优化。

### 性能 / 打标流程

- **打标链路三项优化落地**（来自 `docs/RUST_TAGGER_REWRITE_EVAL_2026-11.md` 的 P0/P1）。
  **结论先说：打标任务的 90% 以上时间原先花在「等 SauceNAO 网络往返」，不是本地推理**；
  这三项改动后「标签可见」从 8–16 秒降到**亚秒级**，本地推理吞吐从 6.5 张/秒提到 **9.6 张/秒**。

  - **P0-2 Python 侧只锁 GPU 段**（`python/server/models/tagger_model.py`、`python/server/main.py`）。
    原先 `/infer/tags` 用一把全局 `_infer_lock` 把「解码 + 预处理 + 推理」整段包住，
    多请求只能排队。把 `infer()` 拆成 `prepare()`（解码 + 预处理，**不碰显存**）
    与 `run_prepared()`（只 `session.run` 时持锁），端点改为
    「锁内 load → 锁外 prepare → 锁内 run」。
    实测 12 张真实图：串行 154.7 ms/张（6.46 张/秒）→ **2 路并发 9.86 张/秒**；
    4 路无增益（9.2 张/秒），故并发上限取 2 附近。
    `run_prepared` 会在持锁后校验 kind 是否被切换，避免 prepare 与 run 之间换模型导致形状不匹配。
    **美学两个端点保持整段持锁**——torch 链路解码也参与构图，且调用频率低。
  - **P0-1 后端打标管线并发化**（`backend/crates/tagger/src/pipeline.rs`）。
    `run_tag_pipeline` 原是按序 `for` 逐张处理，照 `run_sauce_pipeline` 的 worker 池重写：
    共享 `VecDeque` 队列 + `Arc<Mutex<TagProgress>>`，
    **`worker_count = (key_count * exit_count).clamp(2, 8)`**——下限 2 是为了「只有 1 个 API key 时
    本地推理也能双路并行」，上限 8 防解码大图时内存爆。worker 内的
    `pool.acquire()` 用 1 秒 timeout 包住，以便中断检查能及时生效。
  - **P1 打标与溯源解耦**（本轮用户可见收益最大的一项）。
    原流程「先溯源、未命中才本地打标」意味着每张图都要等一次 8–16 秒的网络往返才看到任何标签。
    改为 **先本地打标落库（`auto_local`，亚秒可见）→ 再溯源 → 命中则用 booru 标签替换**。
    替换靠新方法 `Db::delete_image_tags_by_source(image_id, "auto_local")`，
    避免两套自动标签并存把筛选结果污染成「既有本地词又有 booru 词」。
    溯源失败或限流**不再把整张图算作失败**，只记一条 warn 并保留已写入的本地标签。
    `tag_one` 因此改为返回 `TagOutcome { local_written, sauce_source, cancelled }`。

  **实测佐证**：
  - 图 2162 先清空自动标签与缓存再强制打标：T+0.50s 出现 50 条 `auto_local`，
    T+3.03s 变为 38 条 `auto_danbooru`，`source` 同步为 `danbooru`，`auto_local` 完全消失。
  - 16 张混合批次：T+8.42s 完成 15 张（全部走本地），最后一张等 SauceNAO 到 T+37.96s。
    日志逐条区分「打标成功（本地 → 溯源 danbooru）」与「（本地，溯源未命中）」。
  - 纯本地批次：18 张 / 1.88 s = **9.58 张/秒**。

### 文档

- **`docs/RUST_INFERENCE_FEASIBILITY.md` 打标结论二次翻转**（625 行）。
  初稿判定「cl_tagger 预处理依赖 `transformers` 的 `AutoProcessor`，无法离线复现，不建议迁」。
  用户提供模型实机位置（`E:\Game\AI\cl_tagger\models`）后做了实测，结论推翻：

  - ONNX 签名只有 1 个输入 `pixel_values [N,3,384,384]`、1 个输出 `logits [N,108139]`，
    `model_metadata.json` 的 `is_naflex: false` —— 报告担忧的 NaFlex 三输入**根本不存在**。
  - `preprocessor_config.json` 是 slow 版 `SiglipImageProcessor`：`mean/std = 0.5`、
    `resample = 2`（BILINEAR）、384×384。手写预处理（`resize` → `/255` → `(x-0.5)/0.5`）
    与之逐图对拍：40 张图 **prob_maxdiff 0.0133**，2487 个标签中**阈值翻转仅 8 处**
    （全部落在 0.5±0.005 的模型边界带内）。差异来源是 u8 量化级，不是实现错误。
  - **resize 内核选错会静默改结果**：BILINEAR 翻转 2 处 / BICUBIC 41 处 / LANCZOS 55 处。
    这条已写进风险表（R9），因为 Rust 侧很容易随手用 lanczos3。
  - 迁移工作量从「400–700 行 / 高风险」下调为「**250–450 行 / 中风险**」，
    排序改为「先迁美学，打标排后」——唯一真障碍只剩 `ort` 的 CUDA 部署门槛。

  教训已写进文档：「把『仓库里没有』当成『系统里不存在』是错误推断」。

- **新增 `docs/RUST_TAGGER_REWRITE_EVAL_2026-11.md`：打标环节单独做 Rust 重写的可行性与收益实测**。
  上一份报告的主结论是「部分迁移」，但打标那一行的收益是跟美学合并算的，
  且把 `ort` 的 CUDA 门槛（R3）当成唯一真障碍。本次把打标单独拎出来做了完整实测：

  - **可行性：已跑通。** `scripts/ort_trial/` 用 `ort 2.0.0-rc.10` 直读 `model.onnx`，
    端到端（解码→resize→归一化→NCHW→ONNX→sigmoid→阈值→降序）**125–130 ms/张**，
    纯 `session.run` 76.8 ms/张。同图对拍 Python 47 标签 / Rust 47 标签，
    **Jaccard 1.0000**，置信度最大差 0.016855（平均 0.001490）。
  - **R3 更正（重要）**：那套「CUDA ≥13.2 + cuDNN ≥9.23」是 **ort rc.11+ 的要求**。
    rc.10 打的是 **ORT 1.22.0**，只需 **CUDA 12 + cuDNN 9**，与 torch 的 cu128 同源，
    可直接复用 `torch/lib` 里现成的 DLL，**零额外体积**。原文把两个 ORT 版本的要求混在了一起。
  - **最小 CUDA DLL 集合 = 10 个 / 1660.4 MB**。判据演进有教训：
    用「`InferenceSession` 能否构造」当判据会得出「12 个必需」的**错误结论**
    （构造只验证 DLL 加载，不验证 cuDNN 子库齐全）；换成「真实推理跑出 47 标签」后
    才测准——只留构造能过的 6 个会报 `CUDNN_STATUS_NOT_SUPPORTED_SUBLIBRARY_UNAVAILABLE`。
  - **收益远小于预期**（这是不建议做的真正原因）：
    - 体积：共存场景只省 **188 MB**（`onnxruntime` 包 391 MB），因为**只要美学还在 Python，
      torch 的 4228 MB 一个字节都删不掉**；3 GB 收益属于「美学也迁走」的合并账。
    - 吞吐：单路串行 Rust 8.0 张/秒 vs Python 2.8–5.5 张/秒；但**并发到 2 路后两者持平**
      （Rust 11.46 vs Python 细粒度锁 10.90 张/秒，差 5%）。瓶颈是单卡 GPU 串行，不是语言。
    - **真实任务级改善不到 2%**：job #152 38 张 290 s（7.6 s/张）、#153 29 张 387 s（13.3 s/张），
      本地推理只占 **1.7% / 0.9%**。`tag_one` 是「先 SauceNAO 溯源、未命中才走本地」，
      而 SauceNAO 单次往返 8–16 s。**打标慢是因为网络，不是因为 Python。**
  - **给出替代方案（按性价比）**：P0 后端打标管线并发化（`pipeline.rs:265` 的串行 `for`
    改成有界并发，理论上限 2×）；P0 Python 侧 `_infer_lock` 细化到只包 `session.run`
    （实测 2 线程可达 10.90 张/秒）；P1 打标与溯源解耦（用户看到标签从 8–16 s 降到 0.2 s）；
    P2 golden 基准集；Rust 重写排 P3。
  - 同步更正了主报告里 13 处 R3 相关表述（开头更正横幅、§0.2、§0.3.1、§2.1、§3.1、§4.1、
    §4.3、§5.3、§6.1、§8 风险表 R3、§9.1、§9.3、附录 A），均保留原文划线以便追溯。

### 工作区清理

- **`scripts/` 瘦身**：`scripts/` 从 111 文件 / 7.7 MB 降到 **22 文件 / 516 KB**。
  清掉的是 2026-08～09 期间的一次性调试脚本（`test_*` / `probe_*` / `diag_*` / `verify_*`）
  与本轮已消费的截图。保留的是有复用价值的验证脚本（含新增的 `scripts/verify_round.py`，
  安装后四项检查：后端 health / 推理 health / 真实打标 / 图库数据）。
  `scripts/` 本来就在 `.gitignore` 内，清理只影响本地。

### 新增

- **详情页返回时的"缩回缩略图"过渡**（视觉改进1）：退出详情页时，当前大图就地缩小并平移到
  它在图库中对应缩略图的位置，动画结束后才切换页面，返回过程视觉连续。

  实现要点（前两版失败的教训）：
  - **在详情页内就地播放**（对 `.stage-img` 施加 transform 缩放平移），动画结束再导航。
    早期版本"先导航、再由图库构建浮层"，期间真实缩略图已渲染，两者叠加反而加剧闪烁。
  - **目标坐标在进入详情时记录**（`detailPos.thumbRect`），而不是返回时查询——
    图库被 keep-alive 缓存时以 `display:none` 隐藏，返回时 `getBoundingClientRect()` 返回全 0，
    这正是此前"改进完全无效果"的原因。
  - 返回时按滚动差换算目标坐标（`y - (当前scrollTop - 进入时scrollTop)`），保证滚动后仍落到正确位置。
  - 详情页内翻页（上/下一张）会同步更新 `detailPos.imageId`，避免缩到最初那张的位置。
  - 主目录（分组展开结构）暂不播放该动画，直接返回。

### 修复

- **瀑布流卡片高度塌陷（细长条）的真正根因**：`ImageCard` 的瀑布流样式中写了
  `.waterfall-mode .thumb { aspect-ratio: auto; height: auto; }`，
  这两条**覆盖了由内联 style 提供的 aspect-ratio（原图宽高比）**，
  使缩略图容器高度塌陷为 0（`.thumb-img` 的 `height:100%` 在父高未定时亦为 0）→
  卡片变成细长条，且因 row span 与实际高度不符，整片布局看起来"堆叠"。
  现改为：瀑布流下 `.thumb` 只约束 `width: 100%`，高度完全交给内联 aspect-ratio；
  `.thumb-img` 显式 `width/height: 100%` 撑满容器。

  说明：此前多轮修复集中在 JS 的 row span 计算上（那部分现已改为按宽高比推算、逻辑正确），
  但 CSS 这处覆盖才是"卡片高度不对"的直接原因。

- **瀑布流布局塌陷（所有卡片挤成细条堆叠）**：根因是 `row span` 依赖"测量 DOM 高度"，
  而测量时机极易出错——缩略图 lazy 加载、`await` 让出线程、测量前清空 layout
  都会读到塌陷状态的高度，使 span 全为 1（卡片挤在一列）。
  **根治**：`row span` 改为**由图片宽高比直接推算**
  （卡片高 = 卡片宽 × 原图高宽比 + 信息区固定高度），完全不再依赖 DOM 测量与图片加载状态。
  同时给 `itemStyle` 增加**兜底定位**：layout 尚未算出时按索引估算位置，
  避免卡片因无 grid 定位而落入自动流堆叠（此为塌陷的直接表现）。

- **详情页返回后图库位置向下错位一张**：`restorePos` 无条件用 `scrollIntoView({block:"center"})`，
  会把原本就在视口内的目标强行居中，表现为向下滚动。
  现改为：目标已在视口内则**不滚动**；不在视口内用 `nearest` 最小幅度滚动。

### 新增

- **详情页返回时的"缩回缩略图"过渡**：退出详情页时，当前大图会就地缩小并平移到它在图库中
  对应缩略图的位置，动画结束后再切换页面，返回过程视觉连续。
  实现要点：**在详情页内就地播放**（transform 缩放+平移），动画结束才导航——
  早期版本是"先导航、再由图库从零构建浮层"，期间真实缩略图已渲染，
  两者叠加反而造成闪烁且目标元素常未就绪（改进无效果），已改为当前实现。

### 修复

- **详情页也触发框选、且文字无法选中**：图库被 keep-alive 缓存，切到详情页时组件**不会卸载**，
  框选的全局 mousedown 监听继续生效；同时 `no-select` 加在 `body` 上波及了整页。
  现改为：新增 `enabled` 开关（由 `onActivated`/`onDeactivated` 控制，非激活时彻底失效），
  并把 `no-select` 限定在图片墙容器内（不再影响详情页等区域的文字选择）。
- **侧边栏收起/展开后瀑布流位置错乱（间隙变大或重叠）**：侧边栏是**宽度过渡动画**，
  过渡期间宽度连续变化，而 `layoutWaterfall` 只在列数变化时才等 `nextTick`，
  立即测量会读到过渡中间态的高度 → row span 算错。
  现改为：ResizeObserver 识别宽度变化后重排，并额外在过渡结束后（320ms）补一次收尾重排；
  `layoutWaterfall` 内部统一 `nextTick + rAF` 后再测量。
- **切换页面时闪烁（且详情页返回时更明显）**：根因是 `onActivated` 里**无条件 `fetchPage()`**——
  每次从其它页面切回图库都会重新拉取列表，缩略图被迫重新解码/懒加载。
  keep-alive 的意义本就是保住数据与 DOM，现改为**默认不重新拉取**，
  仅在列表为空（首次或上次加载失败）时才补拉；主目录只刷新树与分组计数，不重载图片。
  同时移除了此前"先导航再构建浮层"的飞回实现（它是闪烁加剧的直接原因）。

- **入场动画异常（卡片"飞过来"、明显卡顿、切回复现）**：三处根因——
  1. 卡片关键帧含 `transform: translateY(...)`，与卡片所在 grid/瀑布流布局及
     `TransitionGroup` 的位移动画互相干扰，表现为卡片从右下侧汇聚到左上角。
     现改为**纯 opacity 淡入**，不触碰 transform。
  2. `TransitionGroup` 的 `.flip-move { transition: all }` 会把 grid 定位、尺寸等
     一并做成动画（卡片从旧格子"飞"到新格子）。现只过渡透明度。
  3. 曾用 `appearEpoch` 拼进列表项 key 以强制重播动画 → **每次播放都整表 DOM 重建**，
     200+ 张图时卡顿明显，且元素重建期间瀑布流测量到 0 高度触发多轮重排。
     现改为 key 只用图片 id；卡片自身监听 `appearEpoch`，
     用「短暂关闭动画 → 读 offsetWidth 强制重算样式 → 重新开启」的方式重播，**不重建 DOM**。
  另：元素重建后等 `nextTick + rAF` 再测量瀑布流，避免测量到未渲染的高度。

- **「复制图片」导致应用闪退**（严重）：`OleSetClipboard` 要求数据对象在剪贴板使用期间保持存活，
  而此前该对象是局部变量，函数返回即释放 → 剪贴板持有悬空指针 → 访问违例崩溃。
  现改为直接 `SetClipboardData(CF_HDROP, hmem)`（内存块所有权转移给系统），
  不依赖任何 Rust 侧对象存活。
- **Shift 连选会选中文字**：框选路径已禁用文本选择，但 Shift/Ctrl 连选走的是**单击**路径，
  浏览器在 `mousedown` 时就开始文本选择。现三层防护：卡片 `mousedown` 阻止默认行为、
  信息区 `mousedown.prevent`、卡片样式加 `user-select: none`。
- **部分空白区域无法起框**：框选起框条件用了 `containerRef.contains(target)`，
  把图片墙容器的左右留白与上下间距也排除了。现改为「在 `.app-main` 内且不落在卡片/交互控件上」即可起框。
- **渐进入场动画效果异常**：上一版引入的「锁定式 playEnter」一旦为 true 便永不复位，
  导致翻页、追加加载的项也带延迟（表现为动画"变奇怪"）；且 `appearEpoch` 递增与数据渲染是异步分离的。
  现移除锁定逻辑，动画完全由 `appearDelay` 是否传入决定，并在动画前 `await nextTick()` 确保新数据已进入 DOM；
  同时补齐 `appearEpoch` 变化后的瀑布流重排。

- **选择交互三处问题**：
  - **拖动会选中文字/图片（变蓝）**：框选/Shift 连选期间未禁用文本选择。
    现框选按下时给 `body` 加 `no-select`（`user-select: none`），松开或卸载时移除。
  - **Ctrl+A 选中数量超过一页**：全选取的是 `library.images`——该数组在分页/滚动加载时**累积**，
    可能包含数百张而屏幕只显示当前页。现改为以 DOM 中实际渲染的卡片为"可见范围"，
    范围选择（Shift）同样基于可见范围，语义与资源管理器一致。
  - **新增**：选中状态下点击空白处取消选择（空白区普通单击，与框选共用同一入口）。
- **「复制图片」无效**：`OleSetClipboard` / `DoDragDrop` 都要求线程已 `OleInitialize`，
  而此前完全未初始化 COM → 调用静默失败。现新增 `ensure_ole()` 在拖出与剪贴板入口统一初始化
  （正确识别 S_FALSE 与 RPC_E_CHANGED_MODE，不误判为错误）。
- **渐进入场动画仍不生效**：此前靠切换 class 触发 CSS 动画，浏览器常因优化而不重放；
  且 `appearDelay` 依赖瀑布流列数，列数未测量时行号全为 0。现改为：
  父级递增 `appearEpoch` 作为列表项 key 前缀 → 元素重建 → 动画可靠重播；
  列数未就绪时即时推算。另为**懒加载的缩略图**单独加淡入（卡片动画很短，
  图片常在动画结束后才解码完成，会"啪"地出现破坏观感）。

- **图库渐进入场动画在切回时失效**：动画此前只在 `onMounted` 播放，而图库被 keep-alive 缓存，
  从主目录切回不会重新挂载，动画因而消失。现改为在 `onActivated` 中判断——
  从详情页返回（恢复浏览位置）时**不播放**，从其它板块切回时**重新播放**。

### 新增

- **导入时创建批量任务**：拖入导入确认框新增「导入后」选项（自动打标 / 自动美学评分 / 自动溯源），
  导入完成后自动对本次新增的图片提交对应批量任务
- **美学批量强制重评**：图库与主目录的批量操作新增「强制重评（覆盖已有分数）」勾选，
  忽略"已有美学分"过滤，重新评分并覆盖旧分数

### 修复

- **拖入导入失效（拖入窗口显示 STOP 拒绝光标）**：wry 0.55.1 在 WebView2 上先调用
  `SetAllowExternalDrop(false)` 关闭内置拖放，再以 `RevokeDragDrop(hwnd) != DRAGDROP_E_INVALIDHWND`
  为前提注册自己的 OLE `IDropTarget`——关闭内置拖放后子窗口一般没有既有 target，
  该条件短路导致注册被跳过，系统层面没有任何可接受拖放的目标窗口：
  拖入显示拒绝光标，`DragDropEvent` 也永不触发。
  修复：壳层新增 `src-tauri/src/drag_drop.rs`，在窗口及其子窗口上**无条件**注册 `IDropTarget`，
  通过 Tauri 事件 `moevault://drag-drop` 把路径转发给前端；前端同时监听原生通道与官方通道。
  顺带修复拖入导入忽略「默认导入方式」设置（此前硬编码为 move）。

- **运行时数据迁移到安装目录**：venv、模型、缓存等大体积文件默认放在 `<安装目录>\runtime\`，
  不再占用 C 盘（本机原占用 11.8 GB）。安装目录不可写时自动回退 `%LOCALAPPDATA%`；
  可用环境变量 `MOEVAULT_RUNTIME_DIR` 显式指定
- **打标 GPU 加速**：支持 onnxruntime-gpu（与 CPU 版共存），打标可真正跑在 CUDA 上；
  设置页「推理设备」新增能力提示（GPU 是否就绪 / 缺 onnxruntime-gpu 时给出安装引导）

### 修复

- **批量打标不调用 GPU**（三层原因）：
  1. venv 内为 CPU 版 onnxruntime，无 `CUDAExecutionProvider` → 装 `onnxruntime-gpu` 共存
  2. `tagger_device` 设置**从未下发**到推理服务（后端无该字段）→ 打通
     设置 → 后端 `InferClient.with_device` → `/infer/tagger/config`、`/infer/tags` 全链路
  3. `/devices` 用 `ort_cuda or torch_cuda` 判定打标能力，装 CUDA 版 torch（美学用）时
     会虚报打标 CUDA 选项，选中后静默回落 CPU → 改为**只依据 onnxruntime 自身能力**，
     并返回 `capabilities` 供前端提示
- `/infer/tagger/config` 支持 `model_dir` 为空（仅切换设备），修复“自动探测模型 + 选 GPU”无法生效
- 选显卡时支持 `cuda:N` 指定具体 GPU（onnxruntime `device_id` / torch `cuda:N`）
- 美学模型设备动态化：`set_device` 可在已加载模型上迁移设备，不再硬编码 `to("cuda")`

### 修复（续）

- **批量溯源限流**：SauceNAO 限流状态码 `-2`（Search Rate Too High）此前被当作普通错误——整批图被误判失败且冷却调度不合理。现改为：
  - `-2` / `3` 统一识别为限流，按 `retry_in` 等待
  - 新增**全局 IP 池窗口限流**（默认 4 次 / 30 秒，跨 key 共享）——免费账号共用 IP 池，此前多 key 各自 1 次/30 秒仍会整体超限
  - 限流后同一张图**等待重试**（最多 5 次）而非直接判失败；"无匹配"不再白白冷却 30 秒
  - 成功后的冷却改为按响应头 `short_remaining` 判定（配额未耗尽不冷却），普通失败只做 3 秒短冷却
  - 密钥管理弹窗新增「IP 池窗口」实时状态（已用/上限/冷却剩余）
- **超大图溯源**：超过 19MB 的图片此前会被 SauceNAO 拒收；现先做近无损压缩（JPEG 高质量优先，必要时降质/降分辨率）到限制内再上传
- **BUG 追踪器日志**：
  - 级别过滤下推到服务端（SQL 层），「只看 error」不再被海量 info 挤出结果窗口
  - 显示条数可设置（200/500/1000/2000/5000，localStorage 持久化），单次拉取上限从 500 提到 5000
  - DB 保留量 2000 → 20000 条；导出改为分页拉全量
  - 新增「只看错误 / 全级别」一键切换 + 已加载条数提示
- **任务中心**：修复后端 snake_case 字段（`type_label`/`created_at`）被当作 camelCase 读取导致**类型与时间全部为空**的问题；界面重设计为概览统计卡（点击即筛选）+ 进行中卡片（进度、已用时间、处理速度、预计剩余）+ 全部任务表格（进度条、耗时、错误摘要悬浮、详情/中断/继续）

## [0.1.0] - 2026-08-29

首个发行版（首次发布）。

### 新增

- **桌面壳拖入导入**：文件/多文件/文件夹/多文件夹从资源管理器拖入窗口任意位置 → 全屏覆盖提示 → 确认框（路径预览 + 移动/复制选择）→ 后台导入
- **搜索式筛选（danbooru 风格）**：图库工具栏搜索框，输入前缀联想标签（名称/中文名/别名，词频倒序）与状态（AI/溯源/冗余/打标），选中即色块 chips 即时筛选；支持逗号连续多标签；标签分类着色（画师红/系列橙/角色绿/常规蓝/状态灰）
- **标签归一化**：`looking at viewer` / `looking_at_viewer` 同义标签展示层归一化（显示空格、存储/词典/搜索下划线），空格输入自动匹配
- **多中文别名**：一个标签可有多条中文别名（`tag_aliases` 表），参与搜索联想；标签页多别名管理弹窗；主别名编辑
- **中文字典导入**：设置页一键下载 ffdfkj 的 Danbooru 中英对照表（tag.sqlite，317K+ 条）回填中文别名（仅填空缺）
- **标签页增强**：分类筛选 + 未设中文别名筛选 + 独立分页（50/100/200/500，localStorage 持久化）
- **详情页标签栏**：左侧 danbooru 风格标签栏（分类分组、词频、中文名、点击跳转该标签筛选、一键复制、修改标签二级窗口 + 修改原始标签文本三级窗口）
- **详情页栏收起**：左右栏整栏收起/展开（宽度过渡动画 + 状态持久化），图片区变大
- **导入完成自动刷新**：WebSocket 订阅 `batch.done` → 全局通知 + 主目录/图库/搜索自动刷新
- **浏览上下文导航**：详情页上/下一张在来源上下文内循环（主目录组/筛选结果/搜索结果），位置指示器
- **图库多选增强**：全选（自动进入多选）+ 选中集随筛选自动收缩 + 下拉框多选批量执行（两击确认）
- **已打标/未打标状态搜索**：图库搜索支持 tagged/untagged 状态
- **对比原图**：支持 danbooru/gelbooru/yande.re/konachan/lolibooru 解析 + 后端代理加载网络原图（绕 WebView2 网络栈差异与防盗链）
- **BUG 追踪器**：三态（关闭/本会话开启/开启）全量追踪——API 请求（脱敏/耗时/状态码/错误体）+ 前端错误堆栈 + 关键操作；退出自动转储 txt
- **设置页**：标签设置 tab（字典相关）+ 查重并入本地推理页 + 未保存离开提醒（保存/放弃/取消）

### 修复

- 推理服务：依赖自举（自动创建 venv + 清华镜像安装）、升级资源同步、run_server.bat 编码/语法修复
- 搜索框：候选缓存（失焦清空）、选中后输入残留、el-select 联想崩溃（改自绘组件）
- 标签页：分页 total 不真实（真实计数）、中文字典按钮移除
- 回收站：原因中文化、路径归一、测试杂物清理
- 图库：全选图标随多选关闭复位、每页条数修改生效
- 详情页：栏收起无跳变（宽度过渡）、网络图加载（原生 img + 代理）

### 移除

- `/search` 页面（搜索能力并入图库工具栏搜索框）
- 通用设置中的每页条数（图库/标签页各自独立）

### 架构

- 新增 `tag_aliases`（V11）、`tag_covers`（V10）表
- 新增 `/api/v1/search/suggest`、`/api/v1/dict/import`、`/api/v1/logs/export`、`/api/v1/proxy-image`、`/api/v1/tags/{id}/aliases` 等接口
- WebSocket 客户端（`frontend/src/api/ws.ts`）接入后端广播
