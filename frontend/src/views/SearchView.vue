<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue'
import { Search } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useLibraryStore, type ImageItem } from '@/stores/library'
import { useTaskStore } from '@/stores/tasks'
import { post } from '@/api/client'
import ImageWall from '@/components/ImageWall.vue'
import ImagePreview from '@/components/ImagePreview.vue'
import { useRouter } from 'vue-router'

const router = useRouter()
const library = useLibraryStore()
const taskStore = useTaskStore()
const keyword = ref('')
const aestheticMin = ref<number | null>(null)
const source = ref('')
const onlyRedundant = ref(false)
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

/** 恢复滚动位置。 */
function restorePos() {
  const pos = library.restoreDetailPos('search')
  if (!pos) return
  const el = document.querySelector<HTMLElement>(`.app-main [data-image-id="${pos.imageId}"]`)
  if (el) {
    el.scrollIntoView({ block: 'center' })
    return
  }
  const scroller = document.querySelector('.app-main')
  if (scroller && pos.scrollTop > 0) scroller.scrollTop = pos.scrollTop
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
      aestheticMin: aestheticMin.value ?? undefined,
      source: source.value || undefined,
      isRedundant: onlyRedundant.value ? true : undefined,
      tags: tagsInput.value
        ? tagsInput.value.split(',').map((s) => s.trim()).filter(Boolean)
        : undefined,
    })
    .catch((e: Error) => ElMessage.error(e.message))
}

async function clearSearch() {
  keyword.value = ''
  aestheticMin.value = null
  source.value = ''
  onlyRedundant.value = false
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
    await taskStore.enqueueSauce(ids)
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
        <el-form-item label="美学分≥">
          <el-input-number v-model="aestheticMin" :min="1" :max="5" :step="0.1" placeholder="不限" />
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
</style>
