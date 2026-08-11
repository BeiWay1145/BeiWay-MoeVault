<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { ArrowDown, ArrowRight } from '@element-plus/icons-vue'
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
const sauceFilter = ref<'all' | 'sauced' | 'unsauced'>('all')
const tagFilter = ref<'all' | 'tagged' | 'untagged' | 'no_need'>('all')
const aiFilter = ref<'all' | 'ai' | 'not_ai'>('all')

// 来源组展开状态 + 组内图片缓存 + 分页游标
const expanded = ref<Set<string>>(new Set())
const dirImages = ref<Record<string, ImageItem[]>>({})
const dirNext = ref<Record<string, string | null>>({})
const dirLoading = ref<Record<string, boolean>>({})

// 选中（跨组）
const selected = ref<Set<number>>(new Set())
const selectedCount = computed(() => selected.value.size)

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

/** 组内全选/取消全选。 */
function toggleDirSelect(d: DayGroup, g: DirGroup, all: boolean) {
  const k = dirKey(d, g)
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

async function onBatchTag() {
  const ids = [...selected.value]
  if (ids.length === 0) return
  try {
    await taskStore.enqueueTag(ids)
    selected.value = new Set()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function onBatchAesthetic() {
  const ids = [...selected.value]
  if (ids.length === 0) return
  try {
    await taskStore.enqueueAesthetic(ids)
    selected.value = new Set()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function onBatchSauce() {
  const ids = [...selected.value]
  if (ids.length === 0) return
  try {
    await taskStore.enqueueSauce(ids)
    selected.value = new Set()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function onBatchDetectAi() {
  const ids = [...selected.value]
  if (ids.length === 0) return
  const todo = ids.filter((id) => {
    const all = Object.values(dirImages.value).flat()
    const it = all.find((i) => i.id === id)
    return it ? !it.isAi : true
  })
  if (todo.length === 0) {
    ElMessage.info('所选图片均已标记为 AI 生成')
    selected.value = new Set()
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
  selected.value = new Set()
  await loadTree()
}

function fmtDate(date: string) {
  // date 形如 2026-08-19
  const [y, m, d] = date.split('-').map(Number)
  return `${y}年${m}月${d}日`
}

watch([sauceFilter, tagFilter, aiFilter], loadTree)
onMounted(() => {
  library.fetchImages(50).catch(() => {})
  loadTree()
})
</script>

<template>
  <div class="imports-page">
    <div class="toolbar">
      <div class="filters">
        <el-select v-model="sauceFilter" size="default" style="width: 130px">
          <el-option label="溯源：全部" value="all" />
          <el-option label="已溯源" value="sauced" />
          <el-option label="未溯源" value="unsauced" />
        </el-select>
        <el-select v-model="tagFilter" size="default" style="width: 150px">
          <el-option label="打标：全部" value="all" />
          <el-option label="已打标" value="tagged" />
          <el-option label="未打标" value="untagged" />
          <el-option label="无需打标(AI)" value="no_need" />
        </el-select>
        <el-select v-model="aiFilter" size="default" style="width: 150px">
          <el-option label="AI：全部" value="all" />
          <el-option label="AI 生成" value="ai" />
          <el-option label="非 AI 生成" value="not_ai" />
        </el-select>
      </div>
      <div class="spacer" />
      <template v-if="selectedCount > 0">
        <el-button type="danger" plain @click="onBatchDelete">删除所选 ({{ selectedCount }})</el-button>
        <el-button type="primary" plain @click="onBatchTag">批量打标</el-button>
        <el-button type="success" plain @click="onBatchAesthetic">批量美学</el-button>
        <el-button plain @click="onBatchSauce">批量溯源</el-button>
        <el-button plain @click="onBatchDetectAi">批量检测 AI</el-button>
        <el-button @click="selected = new Set()">取消选择</el-button>
      </template>
    </div>

    <div v-loading="loading">
      <el-empty v-if="days.length === 0 && !loading" description="暂无图片，导入后按日期分组显示" />

      <!-- 日期组 -->
      <div v-for="d in days" :key="d.date" class="day-group">
        <div class="day-header">{{ fmtDate(d.date) }}</div>

        <!-- 来源组 -->
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
