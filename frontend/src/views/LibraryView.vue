<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { Grid, List, Close, Refresh } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useLibraryStore, type ImageItem, type ViewMode } from '@/stores/library'
import { useTaskStore } from '@/stores/tasks'
import { useSettingsStore } from '@/stores/settings'
import { post } from '@/api/client'
import ImageWall from '@/components/ImageWall.vue'
import ImagePreview from '@/components/ImagePreview.vue'

// keep-alive 缓存名（与路由 name 一致）
defineOptions({ name: 'library' })

const router = useRouter()
const library = useLibraryStore()
const taskStore = useTaskStore()
const settingsStore = useSettingsStore()

// E6: 分页模式（通用设置开启时生效）
const page = ref(1)
const pageCursors = ref<Record<number, string>>({})
const paginationOn = computed(() => settingsStore.settings.pagination_enabled)
const pageSize = computed(() => settingsStore.settings.page_size || 50)

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
})

/** 恢复滚动位置：定位到上次查看详情的图片附近。 */
function restorePos() {
  const pos = library.restoreDetailPos('library')
  if (!pos) return
  const el = document.querySelector<HTMLElement>(`.app-main [data-image-id="${pos.imageId}"]`)
  if (el) {
    el.scrollIntoView({ block: 'center' })
    return
  }
  // 图片不在当前列表（可能已删除/筛选变化）：按比例恢复滚动
  const scroller = document.querySelector('.app-main')
  if (scroller && pos.scrollTop > 0) scroller.scrollTop = pos.scrollTop
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

/** 点击卡片：多选模式→切换选择；否则进入详情（记录位置）。 */
function onCardClick(img: ImageItem) {
  if (library.multiSelect) {
    library.toggleSelect(img.id)
    return
  }
  library.saveDetailPos('library', img.id)
  router.push(`/library/${img.id}`)
}

/** 移入回收站（卡片叉号触发，叉号两击/Shift 点击已是确认动作，不再弹框）。 */
async function onRecycle(img: ImageItem) {
  try {
    await post(`/images/${img.id}/recycle`, { reason: 'manual' })
    ElMessage.success('已移入回收站')
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
}
</script>

<template>
  <div class="library">
    <div class="toolbar">
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
        :model-value="library.filter.isAi === true"
        @change="onToggleAiFilter"
      >
        AI 生成显示
      </el-checkbox>

      <el-select
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

      <div class="spacer" />

      <template v-if="selectedCount > 0">
        <el-button type="danger" plain @click="onRecycleSelected">删除所选 ({{ selectedCount }})</el-button>
        <el-button type="primary" plain @click="onBatchTag">批量打标</el-button>
        <el-button type="success" plain @click="onBatchAesthetic">批量美学</el-button>
        <el-button plain @click="onBatchSauce">批量溯源</el-button>
        <el-checkbox v-model="forceSauce" size="small">强制重试不可溯源</el-checkbox>
        <el-button plain @click="onBatchDetectAi">批量检测 AI</el-button>
        <el-button @click="library.clearSelect()">取消选择</el-button>
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
        @size-change="(s: number) => { settingsStore.settings.page_size = s; settingsStore.save().catch(() => {}) }"
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
.spacer {
  flex: 1;
}
.pager {
  display: flex;
  justify-content: center;
  margin-top: 12px;
}
</style>
