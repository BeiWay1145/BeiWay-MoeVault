<script setup lang="ts">
import { computed, nextTick, onActivated, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Grid, List, Close, Refresh } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useLibraryStore, type ImageItem, type ViewMode } from '@/stores/library'
import { useTaskStore } from '@/stores/tasks'
import { useSettingsStore } from '@/stores/settings'
import { post } from '@/api/client'
import { reportLog } from '@/api/log'
import ImageWall from '@/components/ImageWall.vue'
import ImagePreview from '@/components/ImagePreview.vue'
import SearchFilter from '@/components/SearchFilter.vue'

// keep-alive 缓存名（与路由 name 一致）
defineOptions({ name: 'library' })

// 暂时方案：筛选功能未完整实装前，隐藏图库页筛选控件（AI生成显示/美学筛选/溯源下拉）。
// 恢复时改为 true 即可。
const SHOW_LIBRARY_FILTERS = false

const router = useRouter()
const library = useLibraryStore()
const taskStore = useTaskStore()
const settingsStore = useSettingsStore()

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
  await settingsStore.load()
  await fetchPage().catch((e: Error) => ElMessage.error(e.message))
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
  }
})

/** 增强1：导入完成 → 按当前筛选/排序重新拉取（保留浏览状态）。 */
function onImportDone() {
  fetchPage().catch(() => {})
}

onUnmounted(() => {
  window.removeEventListener('moevault:import-done', onImportDone)
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

/** 点击卡片：多选模式→切换选择；否则进入详情（记录位置）。
 *  增强2：把当前筛选/排序下的列表 id 设为浏览上下文（详情上/下一张在本列表内切换）。 */
function onCardClick(img: ImageItem) {
  if (library.multiSelect) {
    library.toggleSelect(img.id)
    return
  }
  const tags = library.filter.tags
  library.setViewerContext(
    library.images.map((i) => i.id),
    tags && tags.length > 0 ? `标签：${tags.join(' + ')}` : '图库',
  )
  library.saveDetailPos('library', img.id)
  router.push(`/library/${img.id}`)
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

/** 批量入回收站。 */
async function onRecycleSelected() {
  const ids = [...library.selected]
  if (ids.length === 0) return
  try {
    await ElMessageBox.confirm(`将所选 ${ids.length} 张图片移入回收站？可随时恢复。`, '批量删除', {
      type: 'warning',
      confirmButtonText: '移入回收站',
    })
  } catch {
    return
  }
  let ok = 0
  for (const id of ids) {
    try {
      await post(`/images/${id}/recycle`, { reason: 'manual' })
      ok++
    } catch {
      /* 单张失败继续 */
    }
  }
  ElMessage.success(`已回收 ${ok} 张`)
  library.clearSelect()
  await library.fetchImages()
}

/** 批量打标（后端管线自动跳过带 AI 生成标签/已溯源已打标/不可溯源的图）。 */
async function onBatchTag() {
  const ids = [...library.selected]
  if (ids.length === 0) return
  const skip = ids.filter((id) => {
    const it = library.images.find((i) => i.id === id)
    return it ? it.isAi || (it.sourceUrl != null && it.sourceUrl !== '') : false
  }).length
  if (skip > 0) ElMessage.info(`其中 ${skip} 张（AI 图/已溯源）将自动跳过`)
  try {
    await taskStore.enqueueTag(ids)
    library.clearSelect()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 批量美学评分。 */
async function onBatchAesthetic() {
  const ids = [...library.selected]
  if (ids.length === 0) return
  try {
    await taskStore.enqueueAesthetic(ids)
    library.clearSelect()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 批量 SauceNAO 溯源（自动跳过 AI 生成图）。 */
async function onBatchSauce() {
  const ids = [...library.selected]
  if (ids.length === 0) return
  const skip = ids.filter((id) => {
    const it = library.images.find((i) => i.id === id)
    return it ? it.isAi : false
  }).length
  if (skip > 0) ElMessage.info(`其中 ${skip} 张 AI 生成图将自动跳过溯源`)
  try {
    await taskStore.enqueueSauce(ids, forceSauce.value)
    library.clearSelect()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 批量溯源是否强制重试不可溯源图。 */
const forceSauce = ref(false)

// ---- 增强3：下拉框多选批量执行（与主目录一致）+ 全选 + 选中集随筛选自动收缩 ----
const batchActions = ref<string[]>([])
const execArmed = ref(false)
let execTimer: number | undefined
/** 全选当前显示的图。 */
const selectAllCurrent = ref(false)
const selectingAll = ref(false)

/** 全选/取消全选当前筛选结果（library.images 即当前显示的图）。
 *  BUG2 修复：全选时自动进入多选模式。 */
async function toggleSelectAll(on: boolean) {
  selectingAll.value = true
  try {
    if (on) {
      library.multiSelect = true
      const s = new Set<number>()
      for (const img of library.images) s.add(img.id)
      selectAllCurrent.value = true
      library.selected = s
    } else {
      selectAllCurrent.value = false
      library.selected = new Set()
    }
  } finally {
    selectingAll.value = false
  }
}

/** 选中集随筛选自动收缩：library.images 变化时，selected 只保留仍在当前显示里的图。 */
watch(
  () => library.images.map((i) => i.id).join(','),
  () => {
    const cur = new Set(library.images.map((i) => i.id))
    const shrunk = new Set([...library.selected].filter((id) => cur.has(id)))
    // 全选状态：当前显示的图是否全被选中
    selectAllCurrent.value = library.images.length > 0 && library.images.every((i) => shrunk.has(i.id))
    if (shrunk.size !== library.selected.size) {
      library.selected = shrunk
    }
  },
)

/** BUG1：关闭多选模式时同步重置全选状态（勾选图标回到未选）。 */
watch(
  () => library.multiSelect,
  (on) => {
    if (!on) {
      selectAllCurrent.value = false
    }
  },
)

/** 按优先级执行批量行为：AI检测 → 溯源 → 打标 → 美学。 */
async function onExecuteBatch() {
  const ids = [...library.selected]
  if (ids.length === 0) {
    ElMessage.warning('没有可执行的图片（请先多选或全选）')
    return
  }
  const order = ['ai-detect', 'sauce', 'tag', 'aesthetic']
  for (const act of order) {
    if (!batchActions.value.includes(act)) continue
    switch (act) {
      case 'ai-detect':
        await onBatchDetectAi()
        break
      case 'sauce':
        await onBatchSauce()
        break
      case 'tag':
        await onBatchTag()
        break
      case 'aesthetic':
        await onBatchAesthetic()
        break
    }
  }
  if (batchActions.value.length > 0) ElMessage.success('批量任务已全部提交')
  batchActions.value = []
}

/** 执行按钮两击确认：第一下变红显示「确认执行」，再点执行；Shift 直接执行。 */
function onExecClick(e: MouseEvent) {
  if (batchActions.value.length === 0) {
    ElMessage.warning('请先选择批量行为')
    return
  }
  if (e.shiftKey) {
    execArmed.value = false
    onExecuteBatch()
    return
  }
  if (execArmed.value) {
    execArmed.value = false
    if (execTimer !== undefined) window.clearTimeout(execTimer)
    onExecuteBatch()
  } else {
    execArmed.value = true
    if (execTimer !== undefined) window.clearTimeout(execTimer)
    execTimer = window.setTimeout(() => (execArmed.value = false), 3000)
  }
}

/** 批量检测 AI（逐张读 PNG tEXt；自动跳过已标记 AI 的图）。 */
async function onBatchDetectAi() {
  const ids = [...library.selected]
  if (ids.length === 0) return
  const todo = ids.filter((id) => {
    const it = library.images.find((i) => i.id === id)
    return it ? !it.isAi : true
  })
  if (todo.length === 0) {
    ElMessage.info('所选图片均已标记为 AI 生成')
    library.clearSelect()
    return
  }
  ElMessage.info(`正在检测 ${todo.length} 张图片的 AI 元信息…`)
  let ok = 0
  for (const id of todo) {
    try {
      await post(`/images/${id}/ai-info`)
      ok++
    } catch {
      /* 单张失败继续 */
    }
  }
  ElMessage.success(`AI 检测完成：${ok} 张已处理`)
  library.clearSelect()
  await library.fetchImages()
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

      <el-checkbox v-model="library.multiSelect">
        多选模式
      </el-checkbox>

      <el-checkbox
        :model-value="selectAllCurrent"
        :indeterminate="selectedCount > 0 && !selectAllCurrent"
        :disabled="library.images.length === 0"
        @change="(v: boolean) => toggleSelectAll(v)"
      >
        全选({{ selectedCount }})
      </el-checkbox>

      <div class="spacer" />

      <!-- 增强3：下拉框多选批量行为 + 两击确认执行（与主目录一致） -->
      <template v-if="selectedCount > 0 || batchActions.length > 0">
        <el-button type="danger" plain @click="onRecycleSelected">删除所选 ({{ selectedCount }})</el-button>
        <el-select v-model="batchActions" multiple collapse-tags placeholder="选择批量行为" style="width: 220px" size="default">
          <el-option label="美学评分" value="aesthetic" />
          <el-option label="打标" value="tag" />
          <el-option label="溯源" value="sauce" />
          <el-option label="AI 检测" value="ai-detect" />
        </el-select>
        <el-checkbox v-if="batchActions.includes('sauce')" v-model="forceSauce" size="small">强制重试不可溯源</el-checkbox>
        <el-button :type="execArmed ? 'danger' : 'primary'" plain @click="onExecClick" :title="'Shift+点击直接执行'">
          {{ execArmed ? '确认执行' : '执行' }}
        </el-button>
        <el-button @click="library.clearSelect(); batchActions = []">取消</el-button>
      </template>
      <el-button :icon="Refresh" circle title="刷新" @click="onSortChange" />
    </div>

    <div class="wall-container">
      <ImageWall
        :images="library.images"
        :view-mode="library.viewMode"
        :selected="library.selected"
        :waterfall-columns="settingsStore.settings.waterfall_columns"
        @click="onCardClick"
        @toggle-select="library.toggleSelect($event.id)"
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
