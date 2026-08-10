<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { Grid, List, Close, Refresh } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useLibraryStore, type ImageItem, type ViewMode } from '@/stores/library'
import ImageWall from '@/components/ImageWall.vue'

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
  { key: 'date', label: '拍摄日期' },
  { key: 'aesthetic', label: '美学分' },
  { key: 'clarity', label: '清晰度' },
  { key: 'size', label: '文件大小' },
  { key: 'random', label: '随机' },
]

// 选中计数
const selectedCount = computed(() => library.selected.size)

function onRecycle(img: ImageItem) {
  ElMessageBox.confirm(`将「${img.name}」移入回收站？可随时恢复。`, '删除确认', {
    type: 'warning',
    confirmButtonText: '移入回收站',
  })
    .then(() => {
      ElMessage.success(`已移入回收站（骨架占位）: ${img.name}`)
    })
    .catch(() => {})
}

/** 排序变化时重新拉取（后端排序）。 */
async function onSortChange() {
  await library.fetchImages().catch((e: Error) => ElMessage.error(e.message))
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

      <div class="spacer" />

      <template v-if="selectedCount > 0">
        <el-button type="danger" plain>删除所选 ({{ selectedCount }})</el-button>
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
        @preview="router.push(`/library/${$event.id}`)"
        @recycle="onRecycle"
      />
    </div>
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
