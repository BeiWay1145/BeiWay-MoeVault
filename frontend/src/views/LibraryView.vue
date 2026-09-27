<script setup lang="ts">
import { computed, nextTick, onActivated, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Grid, List, Close, Refresh } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useLibraryStore, type ImageItem, type ViewMode } from '@/stores/library'
import { useTaskStore } from '@/stores/tasks'
import { useSettingsStore } from '@/stores/settings'
import { post } from '@/api/client'
import { invoke } from '@tauri-apps/api/core'
import { reportLog } from '@/api/log'
import ImageWall from '@/components/ImageWall.vue'
import ImagePreview from '@/components/ImagePreview.vue'
import SearchFilter from '@/components/SearchFilter.vue'
import ExportDialog from '@/components/ExportDialog.vue'
import ContextMenu from '@/components/ContextMenu.vue'
import MarqueeSelect from '@/composables/useMarqueeSelect.vue'
import { useSelection } from '@/composables/useSelection'
import { actionsFor, defaultBatchOptions, type BatchAction } from '@/constants/batchActions'

// keep-alive 缓存名（与路由 name 一致）
defineOptions({ name: 'library' })

// 暂时方案：筛选功能未完整实装前，隐藏图库页筛选控件（AI生成显示/美学筛选/溯源下拉）。
// 恢复时改为 true 即可。
const SHOW_LIBRARY_FILTERS = false

const router = useRouter()
const library = useLibraryStore()
const taskStore = useTaskStore()
const settingsStore = useSettingsStore()

// ---- 架构重构：统一选择模型（资源管理器风格）----
// 所有选择入口（信息区点击、Ctrl 叠加、Shift 范围、Ctrl+A、框选、右键）都收敛到 sel。
// 注意：范围选择/Ctrl+A 都基于"当前可见"的图片——见下方 visibleImageIds 的说明
const sel = useSelection(() => visibleImageIds())
/** 批量参数（注册表声明，模板自动渲染或右键子菜单使用）。 */
const batchOptions = ref(defaultBatchOptions())
/** 右键菜单组件引用。 */
const ctxMenuRef = ref<InstanceType<typeof ContextMenu> | null>(null)
/** 工具栏使用的批量动作。 */
const toolbarActions = computed<BatchAction[]>(() => actionsFor('toolbar'))

// E6: 分页模式（图库每页数独立 localStorage，增强4）
const page = ref(1)
const pageCursors = ref<Record<number, string>>({})
const paginationOn = computed(() => settingsStore.settings.pagination_enabled)
const pageSize = ref(Number(localStorage.getItem('moevault-library-page-size') || '50'))

/** 增强4：图库每页数修改立即生效持久化（BUG2：接收 size 参数并更新 ref，watch 负责重置+刷新）。 */
function onLibraryPageSizeChange(s: number) {
  pageSize.value = s
  localStorage.setItem('moevault-library-page-size', String(s))
}

/**
 * 是否播放入场动画（从上到下、从左到右渐变显示）。
 *
 * 触发场景（视觉改进1）：
 *  - 首次打开图库（onMounted）
 *  - 重置筛选/切换搜索词条后重新拉取（见 watchFilters）
 *  - 切换视图模式（网格/瀑布流/列表）
 * 排除场景：分页翻页、滚动追加、导入完成刷新、窗口尺寸变化、从其它页面切回——
 * 这些情况下保持即时显示，避免每次操作都"重放一遍"动画。
 */
const appearAnim = ref(false)
/** 动画批次号：每次播放递增，作为列表项 key 前缀强制重播动画。 */
const appearEpoch = ref(0)
let appearTimer: number | undefined

/**
 * 播放入场动画一批。
 *
 * 实现：递增 appearEpoch（列表项 key 随之变化 → 元素重建 → CSS 动画必然重播），
 * 同时打开 appearAnim 以计算延迟；动画播完后关闭，后续追加项不再动画。
 *
 * 注意：必须在**数据已渲染之后**调用——否则重建的是旧内容（调用方在 fetchPage 后 await）。
 */
async function playAppearAnimation() {
  // 等一帧确保新数据已进入 DOM（Vue 的 nextTick 只保证虚拟 DOM 更新，
  // 这里再让出一次以覆盖瀑布流的测量时机）
  await nextTick()
  appearAnim.value = true
  appearEpoch.value += 1
  if (appearTimer !== undefined) window.clearTimeout(appearTimer)
  // 最长延迟(620ms) + 动画时长(320ms) + 余量
  appearTimer = window.setTimeout(() => {
    appearAnim.value = false
  }, 1000)
}

/** 按当前分页状态拉取（分页开启→cursor 翻页；关闭→一次拉取）。 */
async function fetchPage() {
  if (paginationOn.value) {
    const cursor = page.value === 1 ? null : pageCursors.value[page.value]
    await library.fetchImages(pageSize.value, { cursor })
    // 记录本页结束游标，供下一页使用
    if (library.nextCursor) pageCursors.value[page.value] = library.nextCursor
  } else {
    await library.fetchImages()
  }
}

async function onPageChange(p: number) {
  page.value = p
  await fetchPage()
  const scroller = document.querySelector('.app-main')
  if (scroller) scroller.scrollTop = 0
}

const totalPages = computed(() => Math.max(1, Math.ceil(library.total / pageSize.value)))

// 分页开关/页大小变化 → 回到第 1 页
watch([paginationOn, pageSize], async () => {
  page.value = 1
  pageCursors.value = {}
  await fetchPage().catch(() => {})
})

onMounted(async () => {
  window.addEventListener('keydown', onLibraryKeydown)
  await settingsStore.load()
  await fetchPage().catch((e: Error) => ElMessage.error(e.message))
  // 视觉改进1：首次加载 → 渐变显示
  await playAppearAnimation()
  // 增强1：从详情返回/重启后还原上次浏览位置
  await nextTick()
  restorePos()
  // 增强1：导入批次完成（WS 广播派发的窗口事件）→ 自动刷新当前列表
  window.addEventListener('moevault:import-done', onImportDone)
})

// keep-alive 激活（从其他板块切回 / 从详情页返回）：重新拉取数据
// （keep-alive 缓存组件时 onMounted 不会再次触发，需 onActivated 刷新）
onActivated(async () => {
  await fetchPage().catch((e: Error) => ElMessage.error(e.message))
  await nextTick()
  // 从详情页返回：恢复上次浏览位置；从其他板块切回：回到顶部
  const restored = restorePos()
  if (!restored) {
    const scroller = document.querySelector('.app-main')
    if (scroller) scroller.scrollTop = 0
    // 视觉改进1：从其它板块切回（非详情返回）→ 重新播放渐进入场。
    // 修复：此前只在 onMounted 播放，而 keep-alive 下切回不会重新挂载，
    // 导致"切换到主目录再切回来"动画消失。
    await playAppearAnimation()
  }
})

/** 增强1：导入完成 → 按当前筛选/排序重新拉取（保留浏览状态）。 */
function onImportDone() {
  fetchPage().catch(() => {})
}

onUnmounted(() => {
  window.removeEventListener('moevault:import-done', onImportDone)
  window.removeEventListener('keydown', onLibraryKeydown)
  if (appearTimer !== undefined) window.clearTimeout(appearTimer)
})

/** 恢复滚动位置：定位到上次查看详情的图片附近。返回是否成功恢复。 */
function restorePos() {
  const pos = library.restoreDetailPos('library')
  if (!pos) return false
  const el = document.querySelector<HTMLElement>(`.app-main [data-image-id="${pos.imageId}"]`)
  if (el) {
    el.scrollIntoView({ block: 'center' })
    return true
  }
  // 图片不在当前列表（可能已删除/筛选变化）：按比例恢复滚动
  const scroller = document.querySelector('.app-main')
  if (scroller && pos.scrollTop > 0) scroller.scrollTop = pos.scrollTop
  return true
}

const viewOptions: { key: ViewMode; icon: typeof Grid; label: string }[] = [
  { key: 'grid', icon: Grid, label: '网格' },
  { key: 'waterfall', icon: Grid, label: '瀑布流' },
  { key: 'list', icon: List, label: '列表' },
]

const sortOptions = [
  { key: 'imported', label: '导入时间' },
  { key: 'aesthetic', label: '美学分' },
  { key: 'size', label: '文件大小' },
  { key: 'random', label: '随机' },
]

// 选中计数
const selectedCount = computed(() => library.selected.size)

// 预览弹窗
const previewVisible = ref(false)
const previewImage = ref<ImageItem | null>(null)
function openPreview(img: ImageItem) {
  previewImage.value = img
  previewVisible.value = true
}

/** 单击缩略图：打开详情（记录位置）。
 *  选择改由 @select 事件承担（点击信息区 / Ctrl+点击 / Shift+点击）。
 *  增强2：把当前筛选/排序下的列表 id 设为浏览上下文（详情上/下一张在本列表内切换）。 */
function onCardClick(img: ImageItem) {
  const tags = library.filter.tags
  library.setViewerContext(
    library.images.map((i) => i.id),
    tags && tags.length > 0 ? `标签：${tags.join(' + ')}` : '图库',
  )
  library.saveDetailPos('library', img.id)
  router.push(`/library/${img.id}`)
}

/**
 * 资源管理器式选择（来自 ImageCard 的 @select）：
 * - 点击信息区、或 Ctrl/Shift+点击 → 语义由 useSelection 统一决定
 */
function onSelect(img: ImageItem, mods: { ctrl: boolean; shift: boolean }) {
  sel.applySelection(img.id, mods)
  library.selected = sel.selected.value
}

/** 右键：已在选中集里 → 作用于整个选择集；否则仅作用于该图（不改变现有选择）。 */
function onContextMenu(img: ImageItem, e: MouseEvent) {
  const ids = sel.selected.value.has(img.id) ? [...sel.selected.value] : [img.id]
  ctxMenuRef.value?.open(e, ids)
}

/**
 * 在已选图片上拖动 → 拖出到资源管理器（复制，无确认）。
 *
 * 实现要点：浏览器 HTML5 DnD 无法携带真实文件路径，
 * 因此调用壳层命令 file_drag_out（内部用原生 DoDragDrop + CF_HDROP）。
 * 拖动的是整个选择集（与右键菜单"作用于选择集"的语义一致）。
 */
async function onDragOut() {
  const ids = [...sel.selected.value]
  const paths = resolveImagePaths(ids)
  if (paths.length === 0) return
  try {
    await invoke('file_drag_out', { paths })
  } catch (e) {
    ElMessage.error(`拖出失败：${(e as Error).message ?? e}`)
  }
}

/** 同步选择集到 library store（保持既有读取 path 兼容：library.selected）。 */
watch(
  () => sel.selected.value,
  (s) => {
    library.selected = new Set(s)
  },
)

/**
 * 当前"可见"的图片 id（Ctrl+A 与范围选择的实际范围）。
 *
 * 为什么不能直接用 library.images：该数组是**累积**的——
 * 开启分页时翻页会 append，关闭分页时滚动加载也 append，
 * 因此它可能包含数百张而屏幕上只显示当前页。
 * 用 DOM 里实际渲染的卡片作为"可见范围"，语义与资源管理器一致。
 */
function visibleImageIds(): number[] {
  const el = wallContainerRef.value
  if (!el) return library.images.map((i) => i.id)
  const ids: number[] = []
  el.querySelectorAll<HTMLElement>('[data-image-id]').forEach((n) => {
    const id = Number(n.dataset.imageId)
    if (Number.isFinite(id)) ids.push(id)
  })
  // DOM 未就绪（首帧）时回退到数据源
  return ids.length > 0 ? ids : library.images.map((i) => i.id)
}

/** Ctrl+A 全选可见图 / Escape 清空（焦点在输入框时不拦截）。 */
function onLibraryKeydown(e: KeyboardEvent) {
  const t = e.target as HTMLElement | null
  const tag = t?.tagName?.toLowerCase()
  if (tag === 'input' || tag === 'textarea' || t?.isContentEditable) return
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a') {
    e.preventDefault()
    // 全选"当前可见"而非全部已加载（修复：一页 100 张却选中 200 张）
    sel.setSelection(visibleImageIds())
    return
  }
  if (e.key === 'Escape') {
    sel.clear()
  }
}

/** 打开导出弹窗（供注册表的 export 动作调用）。 */
function openExportFor(ids: number[]) {
  exportIds.value = ids
  exportDialogVisible.value = true
}

/** 框选容器（图片墙所在 div）。 */
const wallContainerRef = ref<HTMLElement | null>(null)

/** 框选命中回调：additive（Ctrl 按住）时叠加，否则替换。 */
function onMarqueeSelect(ids: number[], additive: boolean) {
  if (additive) sel.addToSelection(ids)
  else sel.setSelection(ids)
  if (ids.length > 0) sel.anchor.value = ids[0]
}

/** 把图片 id 解析成磁盘路径（拖出/复制类动作需要）。 */
function resolveImagePaths(ids: number[]): string[] {
  const cur = new Set(ids)
  const out: string[] = []
  for (const img of library.images) {
    if (!cur.has(img.id)) continue
    // 用库内相对路径：壳层的 file_ops 会把它拼到 `<安装目录>/data/library` 下
    if (img.relPath) out.push(img.relPath)
  }
  return out
}

/** 批量执行上下文（注册表动作的统一入参）。 */
function batchCtx(ids: number[]) {
  return {
    ids,
    options: batchOptions.value,
    refresh: () => fetchPage().catch(() => {}),
    openExport: openExportFor,
    clearSelection: () => sel.clear(),
    resolvePaths: resolveImagePaths,
  }
}

/** 执行一个批量动作（工具栏与右键菜单共用）。 */
async function runBatchAction(action: BatchAction, ids: number[]) {
  if (ids.length === 0) {
    ElMessage.warning('没有可执行的图片')
    return
  }
  try {
    await action.run(batchCtx(ids))
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 移入回收站（卡片叉号触发，叉号两击/Shift 点击已是确认动作，不再弹框）。 */
async function onRecycle(img: ImageItem) {
  try {
    await post(`/images/${img.id}/recycle`, { reason: 'manual' })
    ElMessage.success('已移入回收站')
    reportLog(`回收图片 #${img.id} 到回收站`)
    await library.fetchImages()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}


// ---- 批量执行（架构重构后：统一走注册表，工具栏与右键菜单同源）----
/** 工具栏当前展开的批量面板是否可见（有选中即显示）。 */
const batchPanelVisible = computed(() => sel.count.value > 0)

/** 执行工具栏选中的动作（逐个执行，全部完成后清理选择）。 */
async function executeActions(actions: BatchAction[]) {
  const ids = [...sel.selected.value]
  if (ids.length === 0) return
  for (const a of actions) {
    await runBatchAction(a, ids)
  }
}

/** 视觉改进1：筛选条件变化（切换搜索词条/筛选）→ 重新播放渐进入场动画。
 *  这里监听 filter 的序列化值，避免 library.images 变化（含分页/导入刷新）误触发。 */
watch(
  () => JSON.stringify(library.filter),
  async () => {
    await playAppearAnimation()
  },
)

/** 视觉改进1：切换视图模式（网格/瀑布流/列表）→ 也重新播放一次。 */
watch(
  () => library.viewMode,
  async () => {
    await nextTick()
    await playAppearAnimation()
  },
)

/** 选中集随筛选自动收缩：library.images 变化时，selected 只保留仍在当前显示里的图。 */
watch(
  () => library.images.map((i) => i.id).join(','),
  () => {
    // 架构重构：收缩逻辑收敛到 useSelection，视图不再各自实现
    sel.shrinkToVisible()
  },
)


// ---- 功能增强1：批量导出（共用 ExportDialog 组件）----
const exportDialogVisible = ref(false)
const exportIds = ref<number[]>([])
const exportImages = ref<Array<{ id: number; name: string; thumb: string }>>([])

/** 打开导出弹窗：从选中集取图片信息（缩略图/名称）。 */
function openExportDialog(ids: number[]) {
  exportIds.value = ids
  const cur = new Set(ids)
  exportImages.value = library.images.filter((i) => cur.has(i.id)).map((i) => ({
    id: i.id,
    name: i.name,
    thumb: i.thumbRel ? `/thumbs/${i.thumbRel.replace(/\\/g, '/')}` : '',
  }))
  exportDialogVisible.value = true
}

/** 导出完成回调：回收则清空选择并刷新。 */
function onExportDone(info: { count: number; recycled: number }) {
  if (info.recycled > 0) {
    library.clearSelect()
    fetchPage().catch(() => {})
  }
}



/** 排序变化时重新拉取（后端排序）。 */
async function onSortChange() {
  await library.fetchImages().catch((e: Error) => ElMessage.error(e.message))
}

/** 切换"AI 生成显示"筛选：勾选=只显示 AI 图，不勾=排除 AI 图只显示正常图。 */
async function onToggleAiFilter(val: boolean | string | number) {
  await library
    .applyFilter({ isAi: val === true || val === 'true' ? true : false })
    .catch((e: Error) => ElMessage.error(e.message))
  reportLog(val === true || val === 'true' ? '切换筛选：仅显示 AI 生成图' : '切换筛选：排除 AI 生成图')
}

// ---- 美学分范围筛选（线段端点式 1-5，双端点控制上下限） ----
const aestheticRange = ref<[number, number]>([1, 5])
const aestheticActive = ref(false)
const aestheticIncludeUnscored = ref(false)

/** 滑块松手后自动查询（拖动中仅实时显示数值）。 */
function onAestheticChange() {
  if (aestheticActive.value) {
    library
      .applyFilter({
        aestheticMin: aestheticRange.value[0],
        aestheticMax: aestheticRange.value[1],
        aestheticIncludeUnscored: aestheticIncludeUnscored.value,
      })
      .catch((e: Error) => ElMessage.error(e.message))
  }
}

/** 开关变化：开启→立即按当前范围筛选；关闭→清除美学条件。 */
function onToggleAesthetic(val: boolean | string | number) {
  aestheticActive.value = val === true || val === 'true'
  if (aestheticActive.value) {
    onAestheticChange()
  } else {
    library
      .applyFilter({ aestheticMin: undefined, aestheticMax: undefined, aestheticIncludeUnscored: undefined })
      .catch((e: Error) => ElMessage.error(e.message))
  }
}

// ---- 搜索式筛选（danbooru 风格）：SearchFilter 组件 + chips 即时筛选 ----

/** 已选 chips：`t:<标签名>` 或 `s:<状态key>`。 */
const searchChips = ref<string[]>([])

/** 从 chips 重建筛选条件并刷新（选择/移除/清空都会触发）。 */
async function onSearchChange() {
  const tags: string[] = []
  let isAi: boolean | undefined
  let sauceStatus: string | undefined
  let isRedundant: boolean | undefined
  let source: string | undefined
  let tagged: boolean | undefined
  for (const v of searchChips.value) {
    if (v.startsWith('t:')) tags.push(v.slice(2))
    else if (v === 's:is_ai') isAi = true
    else if (v === 's:not_ai') isAi = false
    else if (v === 's:sauced') sauceStatus = 'sauced'
    else if (v === 's:unsauced') sauceStatus = 'unsauced'
    else if (v === 's:un-sauced') sauceStatus = 'un-sauced'
    else if (v === 's:redundant') isRedundant = true
    else if (v === 's:tagged') tagged = true
    else if (v === 's:untagged') tagged = false
    else if (v.startsWith('s:source_')) source = v.slice('s:source_'.length)
  }
  try {
    await library.applyFilter({
      tags: tags.length > 0 ? tags : undefined,
      isAi,
      sauceStatus,
      isRedundant,
      source,
      tagged,
    })
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 外部修改 filter.tags（如详情页点标签跳转）→ 同步到搜索框 chips。
 *  immediate：直接进详情页跳转时 LibraryView 后挂载，需在挂载时同步已有 filter。 */
watch(
  () => library.filter.tags,
  (tags) => {
    const next = (tags ?? []).map((t) => `t:${t}`).sort()
    const cur = [...searchChips.value].sort()
    if (JSON.stringify(cur) !== JSON.stringify(next)) {
      searchChips.value = next
    }
  },
  { immediate: true },
)
</script>

<template>
  <div class="library">
    <div class="toolbar">
      <!-- 搜索式筛选（danbooru 风格）：标签/状态联想，选中即筛选 -->
      <SearchFilter v-model="searchChips" @change="onSearchChange" />

      <el-radio-group v-model="library.viewMode" size="default">
        <el-radio-button v-for="v in viewOptions" :key="v.key" :value="v.key">
          <el-icon><component :is="v.icon" /></el-icon>
          {{ v.label }}
        </el-radio-button>
      </el-radio-group>

      <el-select v-model="library.sortKey" style="width: 140px" @change="onSortChange">
        <el-option v-for="s in sortOptions" :key="s.key" :value="s.key" :label="s.label" />
      </el-select>
      <el-button @click="onSortChange(); library.sortAsc = !library.sortAsc">
        {{ library.sortAsc ? '升序 ↑' : '降序 ↓' }}
      </el-button>

      <el-checkbox
        v-if="SHOW_LIBRARY_FILTERS"
        :model-value="library.filter.isAi === true"
        @change="onToggleAiFilter"
      >
        AI 生成显示
      </el-checkbox>

      <div v-if="SHOW_LIBRARY_FILTERS" class="aesthetic-filter">
        <el-switch v-model="aestheticActive" size="small" @change="onToggleAesthetic" />
        <el-slider
          v-model="aestheticRange"
          range
          :min="1"
          :max="5"
          :step="0.1"
          :disabled="!aestheticActive"
          :format-tooltip="(v: number) => v.toFixed(1)"
          style="width: 150px"
          @change="onAestheticChange"
        />
        <span class="aesthetic-val">{{ aestheticActive ? `${aestheticRange[0].toFixed(1)}~${aestheticRange[1].toFixed(1)}` : '美学不限' }}</span>
        <el-checkbox v-model="aestheticIncludeUnscored" size="small" :disabled="!aestheticActive">含未评分</el-checkbox>
      </div>

      <el-select
        v-if="SHOW_LIBRARY_FILTERS"
        :model-value="library.filter.sauceStatus ?? ''"
        style="width: 130px"
        @change="(v: string) => library.applyFilter({ sauceStatus: v || undefined })"
      >
        <el-option label="溯源：全部" value="" />
        <el-option label="已溯源" value="sauced" />
        <el-option label="不可溯源" value="un-sauced" />
        <el-option label="未溯源" value="unsauced" />
      </el-select>

      <span class="hint select-hint">
        {{ sel.count.value > 0 ? `已选 ${sel.count.value} 张 · 右键图片执行批量操作` : '单击信息区选中 · Ctrl 加选 · Shift 连选 · Ctrl+A 全选 · 空白处拖拽框选' }}
      </span>

      <div class="spacer" />

      <el-button :icon="Refresh" circle title="刷新" @click="onSortChange" />
    </div>

    <div ref="wallContainerRef" class="wall-container">
      <ImageWall
        :images="library.images"
        :view-mode="library.viewMode"
        :selected="sel.selected.value"
        :waterfall-columns="settingsStore.settings.waterfall_columns"
        :appear-anim="appearAnim"
        :appear-epoch="appearEpoch"
        @click="onCardClick"
        @select="onSelect"
        @contextmenu="onContextMenu"
        @drag-out="onDragOut"
        @preview="openPreview"
        @recycle="onRecycle"
      />
    </div>

    <!-- E6: 分页模式（通用设置开启时显示） -->
    <div v-if="paginationOn" class="pager">
      <el-pagination
        layout="total, prev, pager, next, sizes"
        :total="library.total"
        :page-size="pageSize"
        :current-page="page"
        :page-sizes="[25, 50, 75, 100]"
        @current-change="onPageChange"
        @size-change="onLibraryPageSizeChange"
      />
    </div>

    <!-- 大图预览 -->
    <ImagePreview v-model="previewVisible" :image="previewImage" />

    <!-- 功能增强1：批量导出二级界面（共用组件） -->
    <ExportDialog
      v-model="exportDialogVisible"
      :ids="exportIds"
      :images="exportImages"
      @exported="onExportDone"
    />

    <!-- 框选（空白区按下起框；Ctrl 叠加） -->
    <MarqueeSelect
      :container-ref="wallContainerRef"
      :on-select="onMarqueeSelect"
      :on-blank-click="() => sel.clear()"
    />

    <!-- 右键菜单（复用批量操作注册表，与工具栏同源；无需二次确认） -->
    <ContextMenu
      ref="ctxMenuRef"
      :options="batchOptions"
      :refresh="() => fetchPage().catch(() => {})"
      :open-export="openExportFor"
      :clear-selection="() => sel.clear()"
      :resolve-paths="resolveImagePaths"
    />
  </div>
</template>

<style scoped>
.library {
  display: flex;
  flex-direction: column;
  height: 100%;
}
.toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
  flex-wrap: wrap;
}
.aesthetic-filter {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: nowrap;
}
.aesthetic-val {
  font-size: 12px;
  color: var(--el-text-color-primary);
  min-width: 62px;
  font-variant-numeric: tabular-nums;
}
.spacer {
  flex: 1;
}
.pager {
  display: flex;
  justify-content: center;
  margin-top: 12px;
}
</style>
