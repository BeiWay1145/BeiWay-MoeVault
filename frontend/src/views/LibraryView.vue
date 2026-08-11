<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { Grid, List, Close, Refresh } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useLibraryStore, type ImageItem, type ViewMode } from '@/stores/library'
import { post } from '@/api/client'
import ImageWall from '@/components/ImageWall.vue'
import ImagePreview from '@/components/ImagePreview.vue'

const router = useRouter()
const library = useLibraryStore()

onMounted(() => {
  library.fetchImages().catch((e: Error) => ElMessage.error(e.message))
})

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

/** 移入回收站（真实 API）。 */
async function onRecycle(img: ImageItem) {
  try {
    await ElMessageBox.confirm(`将「${img.name}」移入回收站？可随时恢复。`, '删除确认', {
      type: 'warning',
      confirmButtonText: '移入回收站',
    })
  } catch {
    return // 取消
  }
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

      <div class="spacer" />

      <template v-if="selectedCount > 0">
        <el-button type="danger" plain @click="onRecycleSelected">删除所选 ({{ selectedCount }})</el-button>
        <el-button @click="library.clearSelect()">取消选择</el-button>
      </template>
      <el-button :icon="Refresh" circle title="刷新" @click="onSortChange" />
    </div>

    <div class="wall-container">
      <ImageWall
        :images="library.images"
        :view-mode="library.viewMode"
        :selected="library.selected"
        @click="(img: ImageItem) => router.push(`/library/${img.id}`)"
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
</style>
