<script setup lang="ts">
import { computed, nextTick, onActivated, onMounted, ref } from 'vue'
import { Search } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useLibraryStore, type ImageItem } from '@/stores/library'
import { useTaskStore } from '@/stores/tasks'
import { useSettingsStore } from '@/stores/settings'
import { post } from '@/api/client'
import { reportLog } from '@/api/log'
import ImageWall from '@/components/ImageWall.vue'
import ImagePreview from '@/components/ImagePreview.vue'
import { useRouter } from 'vue-router'

// keep-alive 缓存名（与路由 name 一致）
defineOptions({ name: 'search' })

const router = useRouter()
const library = useLibraryStore()
const taskStore = useTaskStore()
const settingsStore = useSettingsStore()
const keyword = ref('')
/** 美学分范围双端点（1-5，null 表示不限） */
const aestheticRange = ref<[number, number]>([1, 5])
const aestheticActive = ref(false)
/** 美学筛选时包含未评分图 */
const aestheticIncludeUnscored = ref(false)
const source = ref('')
const onlyRedundant = ref(false)
const sauceStatus = ref('')
const forceSauce = ref(false)
const tagsInput = ref('')

// 预览
const previewVisible = ref(false)
const previewImage = ref<ImageItem | null>(null)
function openPreview(img: ImageItem) {
  previewImage.value = img
  previewVisible.value = true
}

onMounted(async () => {
  await library.fetchImages().catch((e: Error) => ElMessage.error(e.message))
  // 增强1：从详情返回/重启后还原上次浏览位置
  await nextTick()
  restorePos()
})

// keep-alive 激活（从其他板块切回 / 从详情页返回）：重新拉取数据
onActivated(async () => {
  await library.fetchImages().catch((e: Error) => ElMessage.error(e.message))
  await nextTick()
  const restored = restorePos()
  if (!restored) {
    const scroller = document.querySelector('.app-main')
    if (scroller) scroller.scrollTop = 0
  }
})

/** 恢复滚动位置。 */
function restorePos() {
  const pos = library.restoreDetailPos('search')
  if (!pos) return false
  const el = document.querySelector<HTMLElement>(`.app-main [data-image-id="${pos.imageId}"]`)
  if (el) {
    el.scrollIntoView({ block: 'center' })
    return true
  }
  const scroller = document.querySelector('.app-main')
  if (scroller && pos.scrollTop > 0) scroller.scrollTop = pos.scrollTop
  return true
}

const selectedCount = computed(() => library.selected.size)

/** 点击卡片：多选模式→切换选择；否则进入详情（记录位置）。 */
function onCardClick(img: ImageItem) {
  if (library.multiSelect) {
    library.toggleSelect(img.id)
    return
  }
  library.saveDetailPos('search', img.id)
  router.push(`/library/${img.id}`)
}

async function onRecycle(img: ImageItem) {
  try {
    await post(`/images/${img.id}/recycle`, { reason: 'manual' })
    ElMessage.success('已移入回收站')
    await library.fetchImages()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function doSearch() {
  await library
    .applyFilter({
      q: keyword.value || undefined,
      aestheticMin: aestheticActive.value ? aestheticRange.value[0] : undefined,
      aestheticMax: aestheticActive.value ? aestheticRange.value[1] : undefined,
      aestheticIncludeUnscored: aestheticActive.value ? aestheticIncludeUnscored.value : undefined,
      source: source.value || undefined,
      isRedundant: onlyRedundant.value ? true : undefined,
      sauceStatus: sauceStatus.value || undefined,
      tags: tagsInput.value
        ? tagsInput.value.split(',').map((s) => s.trim()).filter(Boolean)
        : undefined,
    })
    .catch((e: Error) => ElMessage.error(e.message))
  reportLog(
    `执行搜索筛选（${keyword.value ? `关键字「${keyword.value}」` : ''}${aestheticActive.value ? ` 美学分 ${aestheticRange.value[0].toFixed(1)}-${aestheticRange.value[1].toFixed(1)}${aestheticIncludeUnscored.value ? '(含未评分)' : ''}` : ''}${source.value ? ` 来源=${source.value}` : ''}${onlyRedundant.value ? ' 仅冗余' : ''}${sauceStatus.value ? ` 溯源=${sauceStatus.value}` : ''}${tagsInput.value ? ` 标签=${tagsInput.value}` : ''}）`,
  )
}

/** 美学分滑块松手后自动查询（拖动中仅实时显示数值）。 */
function onAestheticChange() {
  if (aestheticActive.value) doSearch()
}

/** 美学筛选开关变化：关闭则清除美学条件并刷新。 */
async function onToggleAesthetic(val: boolean) {
  aestheticActive.value = val
  if (!val) {
    await library.applyFilter({ aestheticMin: undefined, aestheticMax: undefined, aestheticIncludeUnscored: undefined })
      .catch((e: Error) => ElMessage.error(e.message))
  } else {
    await doSearch()
  }
}

async function clearSearch() {
  keyword.value = ''
  aestheticRange.value = [1, 5]
  aestheticActive.value = false
  aestheticIncludeUnscored.value = false
  source.value = ''
  onlyRedundant.value = false
  sauceStatus.value = ''
  tagsInput.value = ''
  library.clearFilter()
  await library.fetchImages().catch((e: Error) => ElMessage.error(e.message))
}

/** 批量操作（增强3）：删除 / 打标 / 美学 / 溯源 / 检测 AI（打标+检测AI+溯源自动跳过 AI 图）。 */
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
  reportLog(`批量回收 ${ok}/${ids.length} 张图片到回收站`)
  library.clearSelect()
  await library.fetchImages()
}

async function onBatchTag() {
  const ids = [...library.selected]
  if (ids.length === 0) return
  try {
    await taskStore.enqueueTag(ids)
    library.clearSelect()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

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

async function onBatchSauce() {
  const ids = [...library.selected]
  if (ids.length === 0) return
  try {
    await taskStore.enqueueSauce(ids, forceSauce.value)
    library.clearSelect()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

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
</script>

<template>
  <div class="search-page">
    <div class="search-bar">
      <el-input
        v-model="keyword"
        size="large"
        placeholder="搜索文件名关键字"
        :prefix-icon="Search"
        clearable
        @keyup.enter="doSearch"
      />
    </div>

    <div class="filter-panel">
      <el-form label-width="70px" inline>
        <el-form-item label="标签">
          <el-input v-model="tagsInput" placeholder="逗号分隔，如 1girl,blue_archive" style="width: 240px" clearable />
        </el-form-item>
        <el-form-item label="美学分">
          <div class="aesthetic-filter">
            <el-switch v-model="aestheticActive" @change="onToggleAesthetic" />
            <template v-if="aestheticActive">
              <el-slider
                v-model="aestheticRange"
                range
                :min="1"
                :max="5"
                :step="0.1"
                :format-tooltip="(v: number) => v.toFixed(1)"
                style="width: 180px"
                @change="onAestheticChange"
              />
              <span class="aesthetic-val">{{ aestheticRange[0].toFixed(1) }} ~ {{ aestheticRange[1].toFixed(1) }}</span>
              <el-checkbox v-model="aestheticIncludeUnscored">含未评分</el-checkbox>
            </template>
            <span v-else class="aesthetic-val">不限</span>
          </div>
        </el-form-item>
        <el-form-item label="来源">
          <el-select v-model="source" clearable placeholder="不限" style="width: 130px">
            <el-option value="danbooru" label="danbooru" />
            <el-option value="gelbooru" label="gelbooru" />
            <el-option value="local" label="本地" />
          </el-select>
        </el-form-item>
        <el-form-item label="冗余候选">
          <el-switch v-model="onlyRedundant" />
        </el-form-item>
        <el-form-item label="溯源状态">
          <el-select v-model="sauceStatus" clearable placeholder="不限" style="width: 130px">
            <el-option value="sauced" label="已溯源" />
            <el-option value="un-sauced" label="不可溯源" />
            <el-option value="unsauced" label="未溯源" />
          </el-select>
        </el-form-item>
        <el-form-item>
          <el-checkbox v-model="library.multiSelect">多选模式</el-checkbox>
        </el-form-item>
      </el-form>
      <div class="filter-actions">
        <el-button type="primary" @click="doSearch">搜索</el-button>
        <el-button @click="clearSearch">清空筛选</el-button>
        <template v-if="selectedCount > 0">
          <el-button type="danger" plain @click="onRecycleSelected">删除所选 ({{ selectedCount }})</el-button>
          <el-button type="primary" plain @click="onBatchTag">批量打标</el-button>
          <el-button type="success" plain @click="onBatchAesthetic">批量美学</el-button>
          <el-button plain @click="onBatchSauce">批量溯源</el-button>
          <el-checkbox v-model="forceSauce" size="small">强制重试不可溯源</el-checkbox>
          <el-button plain @click="onBatchDetectAi">批量检测 AI</el-button>
          <el-button @click="library.clearSelect()">取消选择</el-button>
        </template>
      </div>
    </div>

    <div class="result-head">
      <span>共 <b class="num-mono">{{ library.total }}</b> 张</span>
      <span class="hint">按 标签/美学分/来源/冗余候选 组合筛选（后端排序）</span>
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

    <!-- 大图预览 -->
    <ImagePreview v-model="previewVisible" :image="previewImage" />
  </div>
</template>

<style scoped>
.search-page {
  display: flex;
  flex-direction: column;
  height: 100%;
  gap: 12px;
}
.search-bar {
  max-width: 640px;
}
.filter-panel {
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 8px;
  padding: 12px;
}
.filter-actions {
  margin-top: 4px;
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.result-head {
  display: flex;
  align-items: baseline;
  gap: 12px;
}
.hint {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.aesthetic-filter {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: nowrap;
}
.aesthetic-val {
  font-size: 12px;
  color: var(--el-text-color-primary);
  min-width: 64px;
  font-variant-numeric: tabular-nums;
}
</style>
