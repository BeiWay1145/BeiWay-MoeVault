# 图库/主目录操作逻辑改版方案（资源管理器风格）

> 状态：**构想阶段，未实施**。待确认后实装。
> 本文档同时作为后续功能扩展的架构参考。

## 一、可行性结论

**总体可行，改动幅度中等偏上（约 900–1300 行，集中在 6 个文件）。**

| 需求 | 可行性 | 难度 | 说明 |
|------|--------|------|------|
| 去掉多选/全选按钮，改快捷键 | ✅ 完全可行 | 低 | 现有 `library.multiSelect` 状态可直接删除 |
| 单击图片下方信息区单选 | ✅ 完全可行 | 低 | 只需拆分点击区域（缩略图 vs 信息区） |
| Ctrl+左键叠加选择 | ✅ 完全可行 | 低 | 纯前端事件处理 |
| Ctrl+A 全选 | ✅ 完全可行 | 低 | 需处理输入框聚焦时不拦截 |
| Shift+左键范围选择 | ✅ 完全可行 | 中 | 需要"最后点击锚点"状态（现有代码无此概念） |
| 框选（拖拽橡皮筋） | ✅ 可行 | **中高** | 需自绘选择框 + 命中检测 |
| 拖出到资源管理器（复制） | ⚠️ **需验证** | **高** | Web 端 `DataTransfer` 无法携带真实路径；见 §2.7 |
| **右键菜单（新增追加需求）** | ✅ 完全可行 | 中 | 见 §2.8 |

---

## 二、逐项实施方案

### 2.1 去掉多选模式，改快捷键

**现状**：`stores/library.ts` 有 `multiSelect`；`LibraryView` 有多选/全选复选框；
`onCardClick` 在其为 true 时走 `toggleSelect`，否则进详情页。

**改法**：
- 删除 `multiSelect` 状态与相关 UI
- 新增 `lastAnchor: number | null`（Shift 范围锚点）
- 统一收敛到**一个**选择动作函数（见 §3.1 的 `applySelection`）

### 2.2 点击"信息区"单选

`ImageCard.vue` 拆分两个命中区：
- **缩略图区**（`.thumb`）→ 打开详情（保持现有行为）
- **信息区**（`.meta`：名称/分辨率/大小/清晰度）→ 单选该图

新增 `@select` 事件；`.meta` 绑定点击并 `@click.stop`。

> 按你的描述"点 A 再点 B 会取消 A"→ 信息区点击是**替换式单选**（不叠加）。

### 2.3 Ctrl+左键（叠加）

按住 Ctrl 点击 → 切换该图选中态；重复点击同一张反复切换（满足"重复单选达到多选"）。

### 2.4 Ctrl+A 全选

- 全局 `keydown`；焦点在 `input`/`textarea`/`contenteditable` 时**不拦截**
- 范围 = 当前显示列表（`library.images`），不含跨页未加载项

### 2.5 Shift+左键（范围选择）

- 记录 `lastAnchor`（上次单击的图片 id）
- Shift+点击 C → 选中 anchor→C 在当前显示顺序中的**连续区间**
- 幂等；无 anchor 时退化为单选

### 2.6 框选（橡皮筋）

- 容器上 `mousedown`（左键、非 Ctrl/Shift）→ 记起点 → 进入框选态
- `mousemove` → 绘制 `position: fixed` 半透明选择框
- 命中检测：遍历卡片 `getBoundingClientRect()` 与选择框求交集
- `mouseup` → 应用（默认替换；Ctrl 叠加）
- 拖动 < 4px 视为普通点击；框选期间 `user-select: none`

> **为何用 rect 而非布局数据**：瀑布流用 `transform` 显式定位，
> `getBoundingClientRect()` 仍返回真实视口坐标，天然兼容三种视图模式。

### 2.7 拖出到资源管理器 ⭐ 关键风险

**问题**：Web 的 `DataTransfer` 出于安全限制**无法设置真实文件路径**。

| 方案 | 可行性 | 代价 |
|------|--------|------|
| **A. 壳层原生拖出**（推荐） | ✅ | 用 `DoDragDrop` + `IDataObject`(CF_HDROP)；与已实现的拖入模块同源，已有 `windows` crate |
| B. `webview.start_drag()` | ⚠️ API 面窄、受限 | 需试验 |
| C. 复制到临时目录 + 提示 | ⚠️ 体验差 | 低 |

**方案 A 要点**：前端 `invoke('start_file_drag', { ids })` → 壳层构造 `CF_HDROP`（指向 `data/library` 真实文件）
→ `DoDragDrop(hwnd, dataObj, DROPEFFECT_COPY)`（默认复制，无确认）。
⚠️ `DoDragDrop` **阻塞调用线程**直到拖放结束，需注意消息循环。

### 2.8 右键菜单（本次追加需求）

**现状**：项目**目前没有任何右键菜单**，批量行为全依赖工具栏下拉框。

**行为定义**（按你的要求）：
- 右键**未选中**的图片 → 菜单只作用于**这一张**（临时单选，不改变现有选择集）
- 右键**已选中**的图片 → 菜单作用于**整个选择集**（单选或多选都适用）

**菜单项**（复用现有批量能力，避免重复实现）：
```
复制路径          ← 新增（复制文件绝对路径到剪贴板）
复制图片          ← 新增（复制文件本身到剪贴板，CF_HDROP，可粘贴到资源管理器）
──────────
打标 / 美学评分 / 溯源 / AI 检测   ← 复用 onExecuteBatch
──────────
导出…            ← 复用 ExportDialog
原图替换（溯源）
──────────
移入回收站        ← 复用 onRecycleSelected
```

**实现方式**：
- 复用 Element Plus 的 `el-dropdown` 手动定位，或自绘一个轻量 `ContextMenu.vue`
- 在 `ImageWall` 的卡片容器上监听 `contextmenu`，`preventDefault` 后定位弹出
- 菜单组件接收 `targetIds: number[]`（根据右键时该图是否已在选中集决定）
- **批量行为的参数选项**（如"强制重评""强制溯源"）→ 菜单项展开为子菜单或直接使用当前默认

**这一项应与 §2.1 的选择模型同步实施**——因为"右键未选中图"需要临时选择语义。

---

## 三、长远架构建议（重点）

你提到"考虑后续维护/增加功能"，这是本次最值得先定的事。当前痛点很明显：

### 3.1 建议一：抽出统一的选择模型 composable

**问题**：当前选择逻辑分散在 `LibraryView.vue`（`onCardClick`、`toggleSelectAll`、选中集收缩 watch）
与 `ImportDirectoryView.vue`（`selected`、`toggleSelect`、`toggleDirSelect`）**两套独立实现**，
行为已经不一致（主目录多一层"按目录全选"）。

**建议**：新建 `composables/useSelection.ts`：

```ts
export function useSelection(getOrderedIds: () => number[]) {
  const selected = ref<Set<number>>(new Set())
  const anchor = ref<number | null>(null)

  /** 唯一的选择入口：所有点击/快捷键/框选都收敛到这里。 */
  function applySelection(id: number, mods: { ctrl?: boolean; shift?: boolean }) { ... }
  function selectAll() { ... }
  function clear() { ... }
  function selectRange(from: number, to: number) { ... }
  function selectByRect(rect: DOMRect) { ... }
  function setSelection(ids: number[]) { ... }

  return { selected, anchor, applySelection, selectAll, clear, selectRange, selectByRect, setSelection }
}
```

**收益**：两个页面共用一套语义；右键菜单、框选、拖出全部基于同一个 `selected`；
未来新增视图（如标签页、搜索结果页）零成本接入。

### 3.2 建议二：抽出「批量操作」注册表

**问题**：批量行为目前硬编码在 `onExecuteBatch` 的 `if (actions.includes('tag')) ...` 长串分支里，
同时散落在模板的下拉选项中。**每加一个批量功能要改 3–4 处**。

**建议**：新建 `constants/batchActions.ts`，把"能力"声明式注册：

```ts
export interface BatchAction {
  key: string                 // 'tag' | 'aesthetic' | 'sauce' | 'ai-detect' | 'export'
  label: string               // '打标'
  icon?: Component
  /** 执行（ids 为选中的图片 id） */
  run: (ids: number[], opts: BatchOptions) => Promise<void>
  /** 可选参数（渲染为勾选框/子菜单） */
  options?: { key: string; label: string; default: boolean }[]
  /** 是否可用于右键菜单 */
  inContextMenu?: boolean
  /** 前置条件（如"未选中任何图时禁用"） */
  enabled?: (ctx: { ids: number[] }) => boolean
}
```

**收益**：新增批量功能 = **只加一个对象**，工具栏下拉、右键菜单、快捷键三处自动出现。
这是投入产出比最高的一项。

### 3.3 建议三：右键菜单与批量行为解耦

右键菜单**不应自己实现业务**，而是渲染 §3.2 注册表中 `inContextMenu: true` 的项。
这样"右键能做什么"与"工具栏能做什么"永远一致，不会出现两套逻辑漂移。

### 3.4 建议四：剪贴板能力走壳层命令（而非前端 API）

"复制路径""复制图片"这类涉及**真实文件系统**的操作，浏览器 API 能力有限
（`navigator.clipboard` 只能放文本；放文件需 `CF_HDROP`）。
建议与拖出一起，在壳层实现一个 `clipboard.rs`：

```rust
#[tauri::command]
fn copy_files_to_clipboard(paths: Vec<String>) -> Result<(), String>  // CF_HDROP
#[tauri::command]
fn copy_text(text: String) -> Result<(), String>                       // 复用，简单
```

**收益**：与拖出共用同一套路径解析/校验逻辑；后续做"粘贴导入"也能复用。

### 3.5 建议五：把「批量任务」的参数收敛为配置对象

当前批量参数（`forceSauce`、`autoReplaceSauce`、`forceAesthetic`）是**散落的独立 ref**，
每加一个参数就要在模板里加勾选框、在调用处加传参。建议改为：

```ts
const batchOptions = reactive<Record<string, boolean>>({
  forceSauce: false, autoReplaceSauce: false, forceAesthetic: false,
})
```

配合 §3.2 的 `options` 声明，模板可**自动渲染**所有参数，无需逐项手写。

### 3.6 建议六：为「文件操作」建立统一的壳层命令层

拖入（已实现）、拖出（§2.7）、复制到剪贴板（§3.4）本质都是**文件系统交互**。
建议统一到 `src-tauri/src/file_ops/` 目录：
```
file_ops/
  drag_in.rs      # 拖入（从现有 drag_drop.rs 迁移）
  drag_out.rs     # 拖出 DoDragDrop
  clipboard.rs    # CF_HDROP 剪贴板
  common.rs       # 路径校验、CF_HDROP 构造/解析（三者共用）
```
**收益**：CF_HDROP 的构造与解析逻辑只写一次；新平台（如未来 macOS）替换该目录即可。

---

## 四、实施顺序建议

| 阶段 | 内容 | 说明 |
|------|------|------|
| **0** | §3.1 选择模型 + §3.2 批量注册表 | **先做基础设施**，后续全部受益 |
| 1 | §2.1–2.5 选择与快捷键 | 基于阶段 0，改动很小 |
| 2 | §2.8 右键菜单 | 自动获得全部批量能力 |
| 3 | §2.6 框选 | 独立功能 |
| 4 | §3.6 + §2.7 + §3.4 壳层文件操作 | 拖出 + 复制（同源实现） |
| 5 | 主目录页同步 + 打磨 | 复用阶段 0 的 composable |

> **强烈建议先做阶段 0**：虽然它是"看不见的重构"，但能让你后续每加一个功能
> 从"改 4 处"变成"加 1 处"。若跳过它直接做右键菜单，会再复制出一套并行逻辑，
> 后续维护成本翻倍。

---

## 五、需要你确认的决策点

1. **是否采用阶段 0 的架构重构**（推荐）？还是先做可见功能、架构以后再补？
2. **拖出方案**：接受壳层原生实现（方案 A）？
3. **单击缩略图**：保持"打开详情"？还是也改为选择？
4. **框选起点**：只在空白区按下才框选，还是图片上也能起框？
5. **拖动冲突**：建议「已选图片→拖出；未选中图片→从该图起框选」，认可吗？
6. **右键菜单**：术语用"复制图片"（复制文件本身）还是"复制"？是否需要"复制路径"？
7. **主目录页**：是否同样改造（它当前是"卡片+复选框"结构）？
8. **右键的批量参数**（强制重评等）放在菜单子项，还是沿用当前工具栏设置？

---

## 六、预估工作量

| 阶段 | 预估 |
|------|------|
| 0（架构基础） | 1 轮 |
| 1（选择+快捷键） | 1 轮 |
| 2（右键菜单） | 1 轮 |
| 3（框选） | 1 轮 |
| 4（拖出+剪贴板） | 1–2 轮 |
| 5（主目录同步） | 1 轮 |

合计 **6–7 轮**（含阶段 0 架构重构）。
