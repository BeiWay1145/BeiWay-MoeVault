# Skill 时效性审计与能力选型报告

- 审计日期：2026-11-14
- 审计对象：`~/.dsh/skills`（实为 51 个真实目录 + `tavernweave` 符号链接）
- 审计人：skill-auditor（团队 task-3）
- 结论先行：**9 项可能过时、32 项仍然有效、10 项与本项目无关**；另推荐 5 个外部插件、排除 5 个看似相关但不合适的。

---

## §1 环境事实

### 1.1 宿主与内核版本

| 项目 | 实测值 | 证据来源 |
|---|---|---|
| 宿主 | `dsh-desktop-next` **2.0.22-next** | `D:/Program Files/DSH-Next/DSH NEXT/resources/app.asar` 内 `package.json`（asar 头解析） |
| 内核 | `@deepseek-ai/dsh` **0.2.2-rc.1** | 同上 dependencies，整套 `@deepseek-ai/dsh-*` 均为 0.2.2-rc.1 |
| Cordis | `@deepseek-ai/cordis` 4.1.0 | 同上 |
| asar 时间 | 2026-11-05 03:12，237 MB | `ls -la` |
| 生效 profile | `desktop` | `~/.dsh/desktop-next.json` → `{"version":1,"active":"desktop"}` |
| profile 依赖 | 23 个插件，26 个 bundle | `~/.dsh/profiles/desktop/package.json` |
| patchReload | `live` | 同上 |

> 注意：`~/.dsh/profiles/desktop/node_modules/@deepseek-ai` 里只有 `cosmokit`、`schemastery` 两个包 —— 这**不是**内核位置，内核在宿主 asar 内。这一点 `dsh-desktop-upgrade` 自己就写对了（铁律 2），但版本号仍停在 0.1.x。

### 1.2 skill 实际规模

- `ls -1 ~/.dsh/skills | wc -l` = **52**
- 其中 `tavernweave` 是一个**符号链接**：
  `tavernweave -> C:\Users\BeiWay1145\.dsh\tavernweave\TavernWeave\skills`
  展开后含 **20 个** SillyTavern / TavernWeave 子 skill（`activate-tavernweave-soul`、`code-quality-workflow`、`consult-tavernweave-library`、`orchestrate-project-blueprint`、`reflect-on-vibe-code-growth`、`rolecard-workshop-ops`、`shadcn-tailwind-ui`、`sillytavern-api-reference`、`sillytavern-card-components`、`sillytavern-card-pipeline`、`sillytavern-component-update`、`sillytavern-database-rolecards`、`sillytavern-embedded-ui`、`sillytavern-extension-dev`、`sillytavern-media-live2d-runtime`、`sillytavern-render-regex-pipeline`、`sillytavern-rolecard-performance`、`sillytavern-rolecard-security`、`sillytavern-runtime-debug`、`tavern-card-builder`）
- 因此**真实 skill 面 = 51 + 20 = 71 个**，而面板/统计里通常只看到 52。

### 1.3 本报告采用的判据

按任务书「关键判据」，只把「要求调用当前环境不存在的**具体工具名/API/路径**」算作硬证据。以下情况**不计入**过时：

- 三个反引号 + `bash` 的代码围栏（全文 40+ 处）—— 那是代码块语言标记，不是工具调用
- 「用 bash 跑个脚本」「this is a Bash script (not POSIX sh)」—— 泛指 shell 语义
- 参数名（`train_batch_size`、`fund_code`、`image_path` 等）

---

## §2 可能过时（9 项，含 file:line 证据）

| # | skill | 证据（file:line） | 原因 | 处理建议 |
|---|---|---|---|---|
| 1 | `stock-analyzer` | `stock-analyzer/SKILL.md:871` 使用 `write_to_file` 工具保存 HTML 文件；`:872` 使用 `preview_url` 工具在浏览器中预览 | `write_to_file` / `preview_url` 在当前环境**均不存在**（现为 `write`，且无对应预览工具）。属于「要求调用具体工具」的硬证据 | 改用 `write` + `browser_*`（或 `ego_*`）预览；不改也可用，只是第 3、4 步会卡住 |
| 2 | `everything-cli` | `everything-cli/SKILL.md:4` `allowed-tools: Bash(everything-cli:*)` | Claude Code 专有 frontmatter 键，DSH 不识别。讽刺的是 `dsh-skill-mcp-ops/SKILL.md:169` 自己就写了「Claude Code 专属 …… 有则 DSH 不认，需删改」这条判据 | 删除该行即可（skill 正文本身有效，全文仅此一处问题） |
| 3 | `plugin-upgrade` | 全文 **44 处** `0.1.x` 版本号 | 内核已是 **0.2.2-rc.1**，跨了一个 minor 大版本。该 skill 的核心资产是「版本走廊 + 破坏性变更表」，走廊全部落在 0.1 系 | 需按 0.2.x 重新校准走廊；方法论本身（只读检查、版本卡）仍可复用 |
| 4 | `dsh-desktop-upgrade` | `:284` 内核 `0.1.2-alpha.1`；`:322` 内核是 0.1.1-rc.1；全文 **35 处** `0.1.x` | 同上。它的「铁律 2：内核权威位置」写对了，但所有版本号、prerelease 分线判断都基于 0.1 系 | **最高优先级更新项**：升级评估是高频任务，版本号错会直接给出错的 GO/NO-GO |
| 5 | `dsh-upgrade-audit` | 全文 **11 处** `0.1.x` | 版本对比工具，默认对比区间落在 0.1 系 | 补 0.2.x 版本对 |
| 6 | `dsh-benchmark-case` | `:88` `@deepseek-ai/dsh@0.1.2-alpha.2` | 基准题固定在内核 0.1.2-alpha.2 上 | 基准题需重新锚定到 0.2.2-rc.1 |
| 7 | `plugin-release` | 全文 **2 处** `0.1.x`（提及 `0.1.2-alpha.*` 未发布队列） | 「未上 npm 的 alpha 队列」这个说法可能已随 0.2 变化 | 核实 npm 现状后更新 |
| 8 | `dsh-plugin-batch-update` | `:135` `wcu 0.5.3 的 launch_app、cost-meter 1.9.49 的搜索补记逻辑` | 引用的插件版本已过期：实测已装 `dsh-cost-meter@1.9.64`（profile package.json） | 版本号刷新；方法论与五条自检陷阱仍然有效 |
| 9 | `kaggle-sdxl-lora-training` | `:1063` 下载目标 `D:/Game/AI/HUISHI_ComfyUI/models/loras/` | 实测 `test -d` = **MISSING**，该目录已不存在 | 若还要用，改为参数化路径；与当前项目无关，可低优先级 |

### 2.1 查过但**不**判定为过时的（避免误伤）

| skill | 疑点 | 核实结论 |
|---|---|---|
| `wizard` | `SKILL.md:13` 单独一行 `- bash` | 位于 `metadata.tags` 内，是**标签**不是工具声明。正文 `bash -n <script>` 只是校验脚本语法。**不过时** |
| `anysearch-skill-3.1.0` | `:185` "Run it with `bash`, not `sh`" | 说的是 `anysearch_cli.sh` 的解释器选择，不是工具调用。**不过时** |
| `plugin-write` | `:44` Implement and register a `ctx.bash` backend | 讲的是插件运行时后端注册（`ctx.bash`/`ctx.pty`/`ctx.fs`/`ctx.tools`），不是模型可调用工具名。宿主确有 shell 执行能力，说明该后端存在；但 0.2.2-rc.1 下是否改名**未核实**（见 §6） |
| `manga-image-translation` | `:34` `mcp_manager_restart` 也不会真正重启进程 | 指的是 mcp-manager 面板动作，非模型工具；且该 skill 与本项目无关 |
| `personality-palette` | `:41` `get_sub_domains` | `mcp__anysearch__get_sub_domains` 在当前环境**存在**。不过时 |
| `reverse-engineer-anything` | `:35` 提到 `grok_bot`、`grok_build`、`omp`、`commandcode`、`vscode` | 这些是 REA 工具枚举的「其他 Agent 运行时」名称，不是本环境工具。不过时 |
| `dsh-skill-mcp-ops` | `:45` 举 `tavernweave_status` 为例 | 那是「举例说明某插件注册的工具」，非必须工具。不过时 |
| `fund-analysis` | `:811` `save_report` | 是 skill 自己脚本里的函数名。不过时 |

### 2.2 全文扫描的负结果（重要）

按任务书的假设，我专门扫了 `str_replace_editor`、`computer`、`apply_patch`、`read_file(`、`list_dir`、`search_files`、`browser_action`、`screenshot_tool`，以及 Claude 风格大写工具名（Read/Write/Edit/Glob/Grep/Bash/Task/WebFetch/WebSearch/TodoWrite/MultiEdit）作为工具引用：

**命中数为 0。** 整套 71 个 skill 里，只有 `stock-analyzer` 一处真正引用了不存在的工具（`write_to_file` / `preview_url`），以及 `everything-cli` 一处 Claude 专有 frontmatter 键。

结论：**这批 skill 的过时主因不是「旧工具名」，而是「内核版本漂移（0.1.x → 0.2.2-rc.1）」和「插件生态版本号过期」。**

### 2.3 本地绝对路径核验

| 路径 | 引用位置 | 实测 |
|---|---|---|
| `C:/Users/BeiWay1145/AppData/Local/Programs/Python/Python310/python.exe` | `dsh-skill-mcp-ops:76,307`、`chaoxing-work-automation:224,236` | **EXISTS** ✅ |
| `D:/Game/AI/HUISHI_ComfyUI/models/loras/` | `kaggle-sdxl-lora-training:1063` | **MISSING** ❌ |

---

## §3 仍然有效（32 项，按用途分组）

这些 skill 是纯方法论或只依赖当前仍然存在的工具（`ops_bash` / `read` / `write` / `edit` / `grep` / `glob` / `web_search` / `web_fetch` / `mcp__anysearch__*` / `subagent` / `spawn_teammate`）。

### 3.1 与当前项目直接相关（Rust / Vue / Tauri / Windows）

| skill | 为什么相关 |
|---|---|
| `diagnosing-bugs` | Rust/Python 双语言服务的疑难 bug 与性能回归诊断闭环 |
| `codebase-design` | 7 crate workspace 的模块边界与接口设计（深模块词汇） |
| `domain-modeling` | `backend/crates/{core,db,ingest,dedup,tagger,api,app}` 的领域词汇表 |
| `tdd` | 红绿重构；项目已有 `cargo test --workspace` 与 `test/` 目录 |
| `prototype` | 前端/交互方案先用一次性原型验证 |
| `resolving-merge-conflicts` | git 冲突处理 |
| `frontend-design` / `make-ui-not-ai` / `web-design-guidelines` / `react-best-practices` | UI 质量与可访问性审查。⚠️ `react-best-practices` 面向 React/Next.js，本项目是 **Vue3**，方法论可借鉴但结论不能直接套 |
| `webapp-testing` | Playwright 驱动本地前端（Vite dev server）做回归 |
| `loop-guard` | 长任务防退化循环，适合大重构与批量处理 |
| `humanizer-2.9.1` | 文档 / CHANGELOG 去 AI 味 |
| `docx` | 交付文档 |

### 3.2 DSH 自身运维（方法有效，仅需注意版本漂移）

`dsh-error-triage`、`dsh-sandbox-test`、`dsh-skill-mcp-ops`、`dsh-plugin-update`、`dsh-skill-collection`、`plugin-workflow`、`plugin-test`、`plugin-runtime-debug`、`plugin-heavy-dep`

> 注：plugin-workflow / plugin-test / plugin-runtime-debug / plugin-heavy-dep 的职责是「编排 / 测试 / 运行时排障 / 重依赖」，本身不含 0.1.x 硬编码，所以不受版本漂移影响；plugin-upgrade / plugin-release 则因版本走廊失效列入 §2。

### 3.3 通用工具与内容处理

`anysearch-skill-3.1.0`（v3.1.0，MCP 服务在册）、`aria2-download`、`research`、`reverse-engineer-anything`（REA v34，139 个工具，`mcp__rea__*` 已挂载）、`paddleocr-text-recognition`、`paddleocr-doc-parsing`、`qq-bot-notify`、`wizard`

### 3.4 tavernweave 符号链接内值得留意的 3 个

`code-quality-workflow`（全量审查 / 架构审查流程）、`orchestrate-project-blueprint`（总设计案与蓝图）、`shadcn-tailwind-ui`（React 向，仅参考）—— 这 20 个子 skill 里只有这三个可能复用到本项目。

---

## §4 与本项目无关（10 项，按主题分组）

这些**不是过时**，只是用不上。保留不影响运行，只是占 skill 目录的检索噪声。

| 主题 | skill |
|---|---|
| 金融投研 | `fund-analysis`、`portfolio-rebalance` |
| 教育学习 | `learning-skill-plus`、`zero-to-pass`、`chaoxing-work-automation` |
| 角色卡 / 二创 | `personality-palette`、`rolecard-fidelity-workflow` |
| 图像 / 视频 / 翻译 | `manga-image-translation`、`video-understanding` |
| MMD 动画 | `mmd-motion-search` |

另外 `stock-analyzer`、`kaggle-sdxl-lora-training` 是「过时 + 无关」双重属性（计在 §2）。

合计：9 过时 + 32 有效 + 10 无关 = **51** ✅（与 `ls` 实测一致；符号链接内的 20 个子 skill 未逐一分类，见 §6）

---

## §5 推荐的新能力

### 5.1 项目画像（用于匹配）

| 层 | 实测 |
|---|---|
| Rust 后端 | workspace **7 个 crate**（`core` / `db` / `ingest` / `dedup` / `tagger` / `api` / `app`；任务书说的 5 个已过时） |
| 关键依赖 | `axum` 0.8（ws）、`rusqlite` 0.32（bundled）、`tokio` full、`reqwest` 0.12（rustls-tls-native-roots）、`image` 0.25、`kamadak-exif` 0.6、`walkdir`、`md-5`、`zip` 2 |
| 桌面壳 | `tauri` 2.11.3（tray-icon + devtools），`rust-version` 1.77.2，自注册 OLE IDropTarget 绕开 wry 0.55.1 的 WebView2 拖放缺陷 |
| 前端 | Vue 3.5.13 + Element Plus 2.9.1 + Pinia 3.0.1 + Vite 6.0.7 + vue-tsc 2.2.0 |
| 推理服务 | Python：FastAPI ≥0.110、onnxruntime-gpu ≥1.15、torch ≥2.1、transformers ≥4.49 |
| 交付 | `cargo tauri build` → MSI：`src-tauri/target/release/bundle/msi/BeiWay-MoeVault_0.1.0_x64_en-US.msi` |

### 5.2 推荐（5 个，均来自 find_dsh_plugins 实测检索）

| # | 名称 | 解决什么具体问题 | 安装方式 | 是否需额外配置 | 兼容性 |
|---|---|---|---|---|---|
| 1 | **dsh-sql**（`STARDUSTLC666/dsh-sql`，npm v0.5.11，radar 实测过） | 直连 SQLite/MySQL/PostgreSQL，提供 `sql_list/query/exec/schema/stats/health`；**词法级只读保护 + 行数钳制 + 写审批门**。本项目用 `rusqlite 0.32 bundled`，调试 schema 迁移、查去重表、验证 `crates/db` 查询时不用再手写临时 Rust 二进制或 sqlite3 命令 | `dsh-sql` | 需给每库配连接串 / 文件路径；写操作走审批门（特性非障碍） | 未标 compat，stars 9，但 radar 标「实测过」 |
| 2 | **deepseek-computer-use**（`STARDUSTLC666/deepseek-computer-use`，v0.2.9） | 用 UI Automation + 窗口截图 + 鼠标键盘操作**真实 Windows 桌面软件**，带会话独立应用授权与操作审批。本项目是 Tauri 桌面应用，装完 MSI 后可做真正的「点开图标 → 拖入图片 → 看缩略图」黑盒验收，而不是只看前端 dev server | `deepseek-computer-use` | 需在设置页给目标 exe（BeiWay-MoeVault.exe）授权；首次会弹审批 | **compat 0.2.1-rc.2 @2026-10-19，紧邻宿主 0.2.2-rc.1** ✅ |
| 3 | **brooks-lint**（`hyhmrright/brooks-lint`，v1.9.1，1573★） | 六个代码质量技能：代码审查、架构审计、技术债、测试质量、健康看板、修复。可对 7 crate workspace 做一次架构级体检，找出 `dedup` / `tagger` / `ingest` 之间的耦合与重复 | `brooks-lint` | 无强制配置 | 未标 compat，需验证 |
| 4 | **dsh-system-monitor-plugin**（`JularDepick/dsh-system-monitor-plugin`，v0.3.3） | 监控 DSH 进程及其**派生进程**的 CPU/内存，面板图表展示整机与分会话占用。本项目常见场景：`cargo build` 7 crate + onnxruntime-gpu 推理 + Vite 同时跑，卡顿时能立刻定位是编译、推理还是前端 | `dsh-system-monitor-plugin` | 无 | 未标 compat |
| 5 | **Aegis**（`GanyuanRan/Aegis`，1438★） | 面向编码 Agent 的软件工程方法包：**基线优先规划 → 系统化调试 → 提示词卫生 → 完成前验证**，外加修复 / 退役双轨跟踪技能。补的正是本项目 `AGENTS.md` 里「每次改动都要 build + test + commit」之外的**完成前验证清单** | `github:GanyuanRan/Aegis` | 无 | 未标 compat |

### 5.3 查过但**不推荐**的（避免重复踩坑）

| 名称 | 为什么不合适 |
|---|---|
| `dsh-log-viewer`（v0.1.2，0★） | 明确写「选择本机 **Java 服务**文本日志」，本项目是 Rust `tracing` + Python uvicorn，用不上；且成熟度太低 |
| `dsh-fast`（v0.2.28） | 剖析的是 **DSH 自身**的上下文 / 缓存 / 延迟，不是项目代码的性能。想剖析 Rust 应直接用 `cargo flamegraph` / perf，不属于 DSH 插件范畴 |
| `dsh-context` / `dsh-mobile` | 目录审核标「当前仓库无法完成检查，安装命令已暂时撤回」，装不上 |
| `Tencent/BrowserSkill` / `omdsh-dev/dsh-browser` | 本项目**已装** `@beiway1145/dsh-browser-control` + `dsh-ego-browser@0.9.14`，浏览器能力重复 |
| `dsh-ops` | **已装** 0.5.4，无需重复 |

### 5.4 通用安装注意

1. 生效 profile 是 `desktop`，由 Electron **独占**，必须走宿主自带 pnpm，不能用 dsh CLI 直接改 —— 这条 `dsh-plugin-batch-update` 已写死为本机硬约束。
2. 装之前先按 `dsh-plugin-update` 的 `ERR_PNPM_UNEXPECTED_STORE` 与 WAL 恢复事务章节处理已知坑。
3. 上表 5 个插件 `compat` 字段大多缺失或指向 `0.1.0-rc.8`，**装完必须重启验证**（以启动日志为主，HTTP 路由判据在 asar 宿主上已作废）。

---

## §6 局限（未验证到的地方）

1. **符号链接内的 20 个 TavernWeave 子 skill 未逐一分类**。frontmatter 未全部读取，只抽查了 3 个。若需完整结论要单独再跑一轮。
2. **`plugin-write` 的 `ctx.bash` / `ctx.pty` / `ctx.fs` 等运行时后端名未在 0.2.2-rc.1 内核里逐个核实**。宿主体是 asar 打包，未解包 .d.ts。
3. **`manga-image-translation` 的 13 处本地绝对路径未全量 test -f**（该 skill 与本项目无关，未投入时间）。
4. **§2 的版本号统计是「出现次数」而非「每个版本号都失效」**。`plugin-upgrade` 的 44 处里可能有一部分是历史版本卡引用（本就该保留），需在真正改写时逐条判断，不能一刀切删除。
5. **§5 推荐的兼容性均为「目录标注值」，未在 0.2.2-rc.1 上实装验证**。dsh-sql、brooks-lint、dsh-system-monitor-plugin、Aegis 都没有 compat 字段，属于「看起来能用，未证明」。
6. **未审计 skill 的 references/、scripts/ 子文件内部**（如 `plugin-upgrade` 有 58 个文件、`react-best-practices` 有 76 个、`docx` 有 61 个）。本报告结论只基于 `SKILL.md` 主文件。
7. **未修改任何文件**。`~/.dsh` 全程只读，本报告是唯一写入产物。

---

## 附录 A：全量清单与判定（51 项）

| # | skill | 目录 mtime | 判定 |
|---|---|---|---|
| 1 | anysearch-skill-3.1.0 | 2026-08-24 | 有效 |
| 2 | aria2-download | 2026-09-13 | 有效 |
| 3 | chaoxing-work-automation | 2026-10-29 | 无关 |
| 4 | codebase-design | 2026-08-22 | 有效 |
| 5 | diagnosing-bugs | 2026-08-22 | 有效 |
| 6 | docx | 2026-10-09 | 有效 |
| 7 | domain-modeling | 2026-08-22 | 有效 |
| 8 | dsh-benchmark-case | 2026-09-10 | **过时** |
| 9 | dsh-desktop-upgrade | 2026-09-14 | **过时** |
| 10 | dsh-error-triage | 2026-09-12 | 有效 |
| 11 | dsh-plugin-batch-update | 2026-11-02 | **过时**（版本号） |
| 12 | dsh-plugin-update | 2026-10-27 | 有效 |
| 13 | dsh-sandbox-test | 2026-09-27 | 有效 |
| 14 | dsh-skill-collection | 2026-09-15 | 有效 |
| 15 | dsh-skill-mcp-ops | 2026-09-07 | 有效 |
| 16 | dsh-upgrade-audit | 2026-09-10 | **过时** |
| 17 | everything-cli | 2026-10-12 | **过时**（frontmatter） |
| 18 | frontend-design | 2026-08-25 | 有效 |
| 19 | fund-analysis | 2026-08-26 | 无关 |
| 20 | humanizer-2.9.1 | 2026-08-22 | 有效 |
| 21 | kaggle-sdxl-lora-training | 2026-10-01 | **过时** + 无关 |
| 22 | learning-skill-plus | 2026-09-03 | 无关 |
| 23 | loop-guard | 2026-09-07 | 有效 |
| 24 | make-ui-not-ai | 2026-08-22 | 有效 |
| 25 | manga-image-translation | 2026-10-09 | 无关 |
| 26 | mmd-motion-search | 2026-10-12 | 无关 |
| 27 | paddleocr-doc-parsing | 2026-08-22 | 有效 |
| 28 | paddleocr-text-recognition | 2026-08-22 | 有效 |
| 29 | personality-palette | 2026-08-15 | 无关 |
| 30 | plugin-heavy-dep | 2026-09-10 | 有效 |
| 31 | plugin-release | 2026-09-10 | **过时** |
| 32 | plugin-runtime-debug | 2026-09-10 | 有效 |
| 33 | plugin-test | 2026-09-10 | 有效 |
| 34 | plugin-upgrade | 2026-09-10 | **过时** |
| 35 | plugin-workflow | 2026-09-10 | 有效 |
| 36 | plugin-write | 2026-09-10 | 有效（1 处待核实） |
| 37 | portfolio-rebalance | 2026-11-07 | 无关 |
| 38 | prototype | 2026-08-22 | 有效 |
| 39 | qq-bot-notify | 2026-10-06 | 有效 |
| 40 | react-best-practices | 2026-08-25 | 有效（React 向，Vue 需转译） |
| 41 | research | 2026-08-22 | 有效 |
| 42 | resolving-merge-conflicts | 2026-08-22 | 有效 |
| 43 | reverse-engineer-anything | 2026-10-11 | 有效 |
| 44 | rolecard-fidelity-workflow | 2026-11-01 | 无关 |
| 45 | stock-analyzer | 2026-08-27 | **过时** + 无关 |
| 46 | tdd | 2026-08-22 | 有效 |
| 47 | video-understanding | 2026-11-01 | 无关 |
| 48 | web-design-guidelines | 2026-08-25 | 有效 |
| 49 | webapp-testing | 2026-08-25 | 有效 |
| 50 | wizard | 2026-08-22 | 有效 |
| 51 | zero-to-pass | 2026-09-03 | 无关 |
| — | `tavernweave`（符号链接 → 20 子 skill） | 2026-09-07 | 部分相关（§3.4、§6.1） |

## 附录 B：本次审计执行的取证命令（可复现）

```bash
# 1. 规模与时间
cd ~/.dsh/skills && ls -1 | wc -l
ls -la --time-style=long-iso

# 2. 旧工具名硬证据扫描（负结果）
grep -rniE 'str_replace|computer_use|apply_patch|read_file\(|list_dir|search_files' --include=SKILL.md .

# 3. 内核版本漂移统计
grep -rnoE '0\.[01]\.[0-9]+(-[a-z]+\.[0-9]+)?' --include=SKILL.md . | awk -F: '{print $1}' | sort | uniq -c | sort -rn

# 4. 宿主真实版本（asar 头解析）
node -e "const fs=require('fs');const b=fs.readFileSync('D:/Program Files/DSH-Next/DSH NEXT/resources/app.asar');const hs=b.readUInt32LE(12);const h=JSON.parse(b.slice(16,16+hs).toString());const e=h.files['package.json'];const o=16+hs+parseInt(e.offset);console.log(b.slice(o,o+e.size).toString())"
```
