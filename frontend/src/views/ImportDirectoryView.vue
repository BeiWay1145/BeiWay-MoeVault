<script setup lang="ts">
import { computed, onActivated, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { ArrowDown, ArrowRight, Refresh } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useLibraryStore, type ImageItem } from '@/stores/library'
import { useTaskStore } from '@/stores/tasks'
import { get, post } from '@/api/client'
import ImageCard from '@/components/ImageCard.vue'

const router = useRouter()
const library = useLibraryStore()
const taskStore = useTaskStore()

/** 主目录：按天 → 来源分组，状态筛选，组内分页，跨组多选+组内全选。 */

interface DirGroup {
  name: string
  source_dir: string | null
  count: number
}
interface DayGroup {
  date: string
  dirs: DirGroup[]
}

const days = ref<DayGroup[]>([])
const loading = ref(false)

// 筛选状态（sauce/tag/ai）
const sauceFilter = ref<'all' | 'sauced' | 'unsauced' | 'un-sauced'>('all')
const tagFilter = ref<'all' | 'tagged' | 'untagged'>('all')
const aiFilter = ref<'all' | 'ai' | 'not_ai'>('all')

// 来源组展开状态 + 组内图片缓存 + 分页游标
const expanded = ref<Set<string>>(new Set())
const dirImages = ref<Record<string, ImageItem[]>>({})
const dirNext = ref<Record<string, string | null>>({})
const dirLoading = ref<Record<string, boolean>>({})

// 选中（跨组）
const selected = ref<Set<number>>(new Set())
const selectedCount = computed(() => selected.value.size)
const forceSauce = ref(false)

/** 组唯一键：date + source_dir。 */
function dirKey(d: DayGroup, g: DirGroup) {
  return `${d.date}::${g.source_dir ?? ''}`
}

async function loadTree() {
  loading.value = true
  try {
    const params = new URLSearchParams()
    if (sauceFilter.value !== 'all') params.set('sauce', sauceFilter.value)
    if (tagFilter.value !== 'all') params.set('tag', tagFilter.value)
    if (aiFilter.value !== 'all') params.set('ai', aiFilter.value)
    const d = await get<{ days: DayGroup[] }>(`/imports/tree?${params.toString()}`)
    days.value = d.days
    // 清空组内缓存（筛选变化后失效）
    expanded.value = new Set()
    dirImages.value = {}
    dirNext.value = {}
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    loading.value = false
  }
}

/** 展开/折叠来源组 + 首次加载组内图片。 */
async function toggleDir(d: DayGroup, g: DirGroup) {
  const k = dirKey(d, g)
  if (expanded.value.has(k)) {
    expanded.value.delete(k)
    expanded.value = new Set(expanded.value)
    return
  }
  expanded.value.add(k)
  expanded.value = new Set(expanded.value)
  if (!dirImages.value[k]) {
    await loadDirImages(k, d.date, g.source_dir)
  }
}

async function loadDirImages(k: string, date: string, sourceDir: string | null, cursor?: string | null) {
  dirLoading.value[k] = true
  try {
    const params = new URLSearchParams()
    params.set('date', date)
    if (sourceDir) params.set('source_dir', sourceDir)
    if (cursor) params.set('cursor', cursor)
    if (sauceFilter.value !== 'all') params.set('sauce', sauceFilter.value)
    if (tagFilter.value !== 'all') params.set('tag', tagFilter.value)
    if (aiFilter.value !== 'all') params.set('ai', aiFilter.value)
    params.set('limit', '60')
    const d = await get<{ items: Array<Record<string, unknown>>; next_cursor: string | null }>(
      `/imports/dir?${params.toString()}`,
    )
    const mapped = d.items.map((it) => ({
      id: it.id as number,
      name: decodeURIComponent((it.rel_path as string).split(/[\\/]/).pop() ?? ''),
      width: it.width as number,
      height: it.height as number,
      sizeBytes: it.size_bytes as number,
      clarity: it.clarity_score as number,
      aesthetic: it.aesthetic_score as number | null,
      isRedundant: it.is_redundant as boolean,
      importedAt: it.imported_at as number,
      thumbRel: (it.thumb_rel as string) ?? '',
      isAi: it.is_ai as boolean,
      format: (it.format as string) ?? undefined,
      sourceUrl: (it.source_url as string) ?? undefined,
      source: (it.source as string) ?? undefined,
    }))
    dirImages.value[k] = [...(dirImages.value[k] ?? []), ...mapped]
    dirNext.value[k] = d.next_cursor
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    dirLoading.value[k] = false
  }
}

/** 加载更多（来源组内分页）。 */
async function loadMore(d: DayGroup, g: DirGroup) {
  const k = dirKey(d, g)
  await loadDirImages(k, d.date, g.source_dir, dirNext.value[k])
}

/** 加载某来源组的全部图片（循环到无下一页），供全选用。 */
async function loadAllDirImages(d: DayGroup, g: DirGroup) {
  const k = dirKey(d, g)
  // 循环加载直到无下一页或已加载数 >= 组 count
  // 首次加载用 null cursor（从头开始），后续用 dirNext
  let cursor: string | null = null
  let guard = 0
  while (guard < 100) {
    const loaded = dirImages.value[k]?.length ?? 0
    if (loaded >= g.count || (cursor === null && loaded > 0 && !dirNext.value[k])) break
    await loadDirImages(k, d.date, g.source_dir, cursor)
    cursor = dirNext.value[k] ?? null
    guard++
  }
}

/** 组内全选/取消全选（全选时自动加载完整个组，未展开也能选全部）。 */
async function toggleDirSelect(d: DayGroup, g: DirGroup, all: boolean) {
  const k = dirKey(d, g)
  if (all && (dirImages.value[k]?.length ?? 0) < g.count) {
    // 未加载完：先加载全部（含未展开的组）
    await loadAllDirImages(d, g)
  }
  const imgs = dirImages.value[k] ?? []
  const s = new Set(selected.value)
  if (all) imgs.forEach((i) => s.add(i.id))
  else imgs.forEach((i) => s.delete(i.id))
  selected.value = s
}

function isDirAllSelected(d: DayGroup, g: DirGroup) {
  const k = dirKey(d, g)
  const imgs = dirImages.value[k] ?? []
  return imgs.length > 0 && imgs.every((i) => selected.value.has(i.id))
}

/** 点击图片：多选模式切换选择，否则进详情。 */
function onCardClick(img: ImageItem) {
  if (library.multiSelect) {
    toggleSelect(img.id)
    return
  }
  library.saveDetailPos('imports', img.id)
  router.push(`/library/${img.id}`)
}

function toggleSelect(id: number) {
  const s = new Set(selected.value)
  if (s.has(id)) s.delete(id)
  else s.add(id)
  selected.value = s
}

/** 批量动作（沿用现有任务中心批量）。 */
async function onBatchDelete() {
  const ids = [...selected.value]
  if (ids.length === 0) return
  try {
    await ElMessageBox.confirm(`将所选 ${ids.length} 张图片移入回收站？`, '批量删除', {
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
  selected.value = new Set()
  await loadTree()
}

async function onBatchTag(ids?: number[]) {
  const list = ids ?? [...selected.value]
  if (list.length === 0) return
  try {
    await taskStore.enqueueTag(list)
    if (!ids) selected.value = new Set()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function onBatchAesthetic(ids?: number[]) {
  const list = ids ?? [...selected.value]
  if (list.length === 0) return
  try {
    await taskStore.enqueueAesthetic(list)
    if (!ids) selected.value = new Set()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function onBatchSauce(ids?: number[]) {
  const list = ids ?? [...selected.value]
  if (list.length === 0) return
  try {
    await taskStore.enqueueSauce(list, forceSauce.value)
    if (!ids) selected.value = new Set()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function onBatchDetectAi(ids?: number[]) {
  const list = ids ?? [...selected.value]
  if (list.length === 0) return
  const todo = list.filter((id) => {
    const all = Object.values(dirImages.value).flat()
    const it = all.find((i) => i.id === id)
    return it ? !it.isAi : true
  })
  if (todo.length === 0) {
    ElMessage.info('所选图片均已标记为 AI 生成')
    if (!ids) selected.value = new Set()
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
  if (!ids) selected.value = new Set()
  await loadTree()
}

function fmtDate(date: string) {
  // date 形如 2026-08-19
  const [y, m, d] = date.split('-').map(Number)
  return `${y}年${m}月${d}日`
}

watch([sauceFilter, tagFilter, aiFilter], loadTree)
// 导入完成后刷新主目录（新图自动分组出现）；keep-alive 切回时也刷新
onMounted(() => {
  library.fetchImages(50).catch(() => {})
  loadTree()
  window.addEventListener('moevault:import-done', loadTree)
})
onActivated(loadTree)
onUnmounted(() => {
  window.removeEventListener('moevault:import-done', loadTree)
})

/** 手动刷新主目录。 */
function refresh() {
  loadTree()
}

/** 重新解析解码失败的图片（width=0），修复基本信息/缩略图。 */
async function reprocessBroken() {
  try {
    const r = await post<{ reprocessed: number; failed: number }>('/images/reprocess')
    ElMessage.success(`重新解析完成：成功 ${r.reprocessed} 张${r.failed > 0 ? `，失败 ${r.failed} 张（多为文件缺失）` : ''}`)
    loadTree()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 查重按钮状态：多选时对选中图查重，否则对当前筛选集查重。 */
const dedupLoading = ref(false)

/** 获取当前筛选/选中范围的全部图片 id。 */
async function getScopeIds(): Promise<number[]> {
  // 多选优先：用选中图
  if (selected.value.size > 0) {
    return [...selected.value]
  }
  // 否则用已加载的组内图片（当前筛选下用户看到的全部图）
  const all = Object.values(dirImages.value).flat()
  const ids = [...new Set(all.map((i) => i.id))]
  if (ids.length === 0) {
    // 尚未展开任何组：提示先展开或先多选
    ElMessage.info('请先展开来源组或多选图片，再执行查重')
  }
  return ids
}

/** 查重：多选时查所选图，否则查当前筛选集。 */
async function onDedup() {
  const ids = await getScopeIds()
  if (ids.length === 0) {
    ElMessage.warning('当前范围没有可查重的图片')
    return
  }
  dedupLoading.value = true
  try {
    const r = await post<{ groups_created: number; images_clustered: number; redundant_marked: number }>(
      '/dedup/scan-scope',
      { image_ids: ids },
    )
    ElMessage.success(
      `查重完成：${r.groups_created} 组，${r.images_clustered} 张归入重复组，${r.redundant_marked} 张标记冗余`,
    )
    // 跳转到查重结果页
    router.push('/dedup')
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    dedupLoading.value = false
  }
}

// ---- 改进1：批量行为（多选下拉 + 执行两击确认）----
const batchActions = ref<string[]>([]) // 选中行为：aesthetic/tag/sauce/ai-detect
const execArmed = ref(false)
let execTimer: number | undefined
// 改进2：日期折叠（默认展开）
const collapsedDays = ref<Set<string>>(new Set())
// 改进3：全选全部
const selectingAll = ref(false)

/** 选中图或当前筛选集的 ids（批量执行用）。 */
function getBatchIds(): number[] {
  if (selected.value.size > 0) return [...selected.value]
  const all = Object.values(dirImages.value).flat()
  return [...new Set(all.map((i) => i.id))]
}

/** 按优先级执行批量行为：AI检测 → 溯源 → 打标 → 美学。 */
async function onExecuteBatch() {
  const ids = getBatchIds()
  if (ids.length === 0) {
    ElMessage.warning('没有可执行的图片（请先多选或展开加载）')
    return
  }
  const order = ['ai-detect', 'sauce', 'tag', 'aesthetic']
  for (const act of order) {
    if (!batchActions.value.includes(act)) continue
    switch (act) {
      case 'ai-detect':
        await onBatchDetectAi(ids)
        break
      case 'sauce':
        await onBatchSauce(ids)
        break
      case 'tag':
        await onBatchTag(ids)
        break
      case 'aesthetic':
        await onBatchAesthetic(ids)
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

/** 改进2：切换日期组折叠。 */
function toggleDay(d: DayGroup) {
  const s = new Set(collapsedDays.value)
  if (s.has(d.date)) s.delete(d.date)
  else s.add(d.date)
  collapsedDays.value = s
}

/** 改进2：日期组内全部图全选（自动加载所有来源组）。 */
async function toggleDaySelect(d: DayGroup, all: boolean) {
  const s = new Set(selected.value)
  if (all) {
    for (const g of d.dirs) {
      await loadAllDirImages(d, g)
      const imgs = dirImages.value[dirKey(d, g)] ?? []
      imgs.forEach((i) => s.add(i.id))
    }
  } else {
    for (const g of d.dirs) {
      const imgs = dirImages.value[dirKey(d, g)] ?? []
      imgs.forEach((i) => s.delete(i.id))
    }
  }
  selected.value = s
}

/** 改进3：全选全部（加载当前筛选下所有组）。 */
async function onSelectAll() {
  selectingAll.value = true
  try {
    const s = new Set<number>()
    for (const d of days.value) {
      for (const g of d.dirs) {
        await loadAllDirImages(d, g)
        const imgs = dirImages.value[dirKey(d, g)] ?? []
        imgs.forEach((i) => s.add(i.id))
      }
    }
    selected.value = s
    if (s.size === 0) ElMessage.info('没有可选的图片')
  } finally {
    selectingAll.value = false
  }
}
</script>

<template>
  <div class="imports-page">
    <div class="toolbar">
      <div class="filters">
        <el-select v-model="sauceFilter" size="default" style="width: 150px">
          <el-option label="溯源：全部" value="all" />
          <el-option label="已溯源" value="sauced" />
          <el-option label="不可溯源" value="un-sauced" />
          <el-option label="未溯源" value="unsauced" />
        </el-select>
        <el-select v-model="tagFilter" size="default" style="width: 150px">
          <el-option label="打标：全部" value="all" />
          <el-option label="已打标" value="tagged" />
          <el-option label="未打标" value="untagged" />
        </el-select>
        <el-select v-model="aiFilter" size="default" style="width: 150px">
          <el-option label="AI：全部" value="all" />
          <el-option label="AI 生成" value="ai" />
          <el-option label="非 AI 生成" value="not_ai" />
        </el-select>
      </div>
      <div class="spacer" />
      <el-button :loading="dedupLoading" @click="onDedup" title="对当前筛选集或选中图查重">查重</el-button>
      <el-button :loading="selectingAll" @click="onSelectAll" title="全选当前库全部图片">全选全部</el-button>
      <el-button :icon="Refresh" circle title="刷新" @click="refresh" />
      <el-button plain @click="reprocessBroken" title="重新解析解码失败的图片（修复基本信息/缩略图）">重新解析</el-button>
      <template v-if="selectedCount > 0">
        <el-button type="danger" plain @click="onBatchDelete">删除所选 ({{ selectedCount }})</el-button>
        <el-button @click="selected = new Set()">取消选择</el-button>
      </template>
      <!-- 改进1：批量行为多选下拉 + 执行（两击确认） -->
      <el-select
        v-model="batchActions"
        multiple
        collapse-tags
        placeholder="选择批量行为"
        style="width: 220px"
        size="default"
      >
        <el-option label="美学评分" value="aesthetic" />
        <el-option label="打标" value="tag" />
        <el-option label="溯源" value="sauce" />
        <el-option label="AI 检测" value="ai-detect" />
      </el-select>
      <el-checkbox v-if="batchActions.includes('sauce')" v-model="forceSauce" size="small">强制溯源</el-checkbox>
      <el-button
        :type="execArmed ? 'danger' : 'primary'"
        plain
        @click="onExecClick"
        :title="'Shift+点击直接执行'"
      >
        {{ execArmed ? '确认执行' : '执行' }}
      </el-button>
    </div>

    <div v-loading="loading">
      <el-empty v-if="days.length === 0 && !loading" description="暂无图片，导入后按日期分组显示" />

      <!-- 日期组 -->
      <div v-for="d in days" :key="d.date" class="day-group">
        <div class="day-header" @click="toggleDay(d)">
          <el-icon>
            <ArrowRight v-if="collapsedDays.has(d.date)" />
            <ArrowDown v-else />
          </el-icon>
          <span>{{ fmtDate(d.date) }}</span>
          <div class="day-actions" @click.stop>
            <el-checkbox @change="(v: boolean) => toggleDaySelect(d, v)">全选本日</el-checkbox>
          </div>
        </div>

        <!-- 来源组（日期折叠时隐藏） -->
        <template v-if="!collapsedDays.has(d.date)">
        <div v-for="g in d.dirs" :key="dirKey(d, g)" class="dir-group">
          <div class="dir-header" @click="toggleDir(d, g)">
            <el-icon>
              <ArrowRight v-if="!expanded.has(dirKey(d, g))" />
              <ArrowDown v-else />
            </el-icon>
            <span class="dir-name">{{ g.name }}</span>
            <span class="dir-count">（{{ g.count }} 张）</span>
            <div class="dir-actions" @click.stop>
              <el-checkbox
                :model-value="isDirAllSelected(d, g)"
                @change="(v: boolean) => toggleDirSelect(d, g, v)"
              >
                全选
              </el-checkbox>
            </div>
          </div>

          <div v-if="expanded.has(dirKey(d, g))" v-loading="dirLoading[dirKey(d, g)]" class="dir-images">
            <div v-for="img in dirImages[dirKey(d, g)] ?? []" :key="img.id" class="dir-cell">
              <ImageCard
                :image="img"
                :selected="selected.has(img.id)"
                @click="onCardClick"
                @toggle-select="toggleSelect(img.id)"
                @recycle="() => {}"
              />
              <div class="cell-check" @click.stop>
                <el-checkbox :model-value="selected.has(img.id)" @change="() => toggleSelect(img.id)" />
              </div>
            </div>
            <div v-if="dirNext[dirKey(d, g)]" class="load-more">
              <el-button size="small" @click="loadMore(d, g)">加载更多</el-button>
            </div>
          </div>
        </div>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.imports-page {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.filters {
  display: flex;
  gap: 8px;
}
.spacer {
  flex: 1;
}
.day-group {
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 8px;
  padding: 12px;
  margin-bottom: 12px;
}
.day-header {
  font-size: 15px;
  font-weight: 600;
  margin-bottom: 10px;
  color: var(--el-text-color-primary);
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 6px;
}
.day-actions {
  margin-left: auto;
}
.dir-group {
  margin-bottom: 8px;
}
.dir-header {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  padding: 6px 8px;
  border-radius: 6px;
  background: var(--el-fill-color-light);
  user-select: none;
}
.dir-header:hover {
  background: var(--el-fill-color);
}
.dir-name {
  font-weight: 500;
}
.dir-count {
  color: var(--el-text-color-secondary);
  font-size: 13px;
}
.dir-actions {
  margin-left: auto;
}
.dir-images {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  padding: 10px 0 4px 24px;
}
.dir-cell {
  position: relative;
  width: 160px;
}
.dir-cell :deep(.image-card) {
  cursor: pointer;
}
.cell-check {
  position: absolute;
  top: 4px;
  left: 4px;
  z-index: 3;
  background: rgba(255, 255, 255, 0.7);
  border-radius: 4px;
}
.load-more {
  width: 100%;
  display: flex;
  justify-content: center;
  padding: 8px 0;
}
</style>
