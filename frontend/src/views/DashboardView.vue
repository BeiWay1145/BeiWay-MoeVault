<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useDedupStore } from '@/stores/dedup'
import { useTaskStore } from '@/stores/tasks'
import { useLibraryStore, thumbUrl } from '@/stores/library'
import { get } from '@/api/client'

const router = useRouter()
const dedupStore = useDedupStore()
const taskStore = useTaskStore()
const libraryStore = useLibraryStore()

// 真实统计（/api/v1/stats + /tagging/stats）
const totalImages = ref(0)
const monthImported = ref(0)
const avgAesthetic = ref('—')
const untaggedCount = ref(0)
const runningTasks = ref(0)

onMounted(async () => {
  // 总览统计（/api/v1/stats 含平均美学分/本月导入）
  try {
    const s = await get<{
      total_images: number
      month_imported: number
      avg_aesthetic: number | null
    }>('/stats')
    totalImages.value = s.total_images
    monthImported.value = s.month_imported
    avgAesthetic.value = s.avg_aesthetic != null ? s.avg_aesthetic.toFixed(1) : '—'
  } catch {
    /* 静默 */
  }
  // 冗余候选
  dedupStore.refreshStats().catch(() => {})
  // 待打标
  try {
    const t = await get<{ untagged: number; tagged: number; active_images: number }>(
      '/tagging/stats',
    )
    untaggedCount.value = t.untagged
  } catch {
    /* 静默 */
  }
  taskStore.loadMock()
  runningTasks.value = taskStore.running
  libraryStore.fetchImages(12).catch(() => {})
})

const quickLinks = [
  { label: '本月新图', desc: '按导入时间', to: '/library' },
  { label: '高分佳作 (≥4.5)', desc: '美学分筛选', to: '/search' },
  { label: '低清晰度', desc: '待人工判断', to: '/search' },
  { label: 'danbooru 来源', desc: '溯源成功', to: '/search' },
  { label: '冗余候选', desc: '查重管理', to: '/dedup' },
]
</script>

<template>
  <div class="dashboard">
    <el-row :gutter="16" class="stat-row">
      <el-col :span="4">
        <el-card shadow="hover" class="stat-card" @click="router.push('/library')">
          <div class="stat-num num-mono">{{ totalImages.toLocaleString() }}</div>
          <div class="stat-label">图片总数</div>
        </el-card>
      </el-col>
      <el-col :span="4">
        <el-card shadow="hover" class="stat-card" @click="router.push('/library')">
          <div class="stat-num num-mono">{{ monthImported.toLocaleString() }}</div>
          <div class="stat-label">本月导入</div>
        </el-card>
      </el-col>
      <el-col :span="4">
        <el-card shadow="hover" class="stat-card warn" @click="router.push('/dedup')">
          <div class="stat-num num-mono">{{ dedupStore.redundantCount }}</div>
          <div class="stat-label">冗余候选</div>
        </el-card>
      </el-col>
      <el-col :span="4">
        <el-card shadow="hover" class="stat-card" @click="router.push('/tasks')">
          <div class="stat-num num-mono">{{ runningTasks }}</div>
          <div class="stat-label">进行中任务</div>
        </el-card>
      </el-col>
      <el-col :span="4">
        <el-card shadow="hover" class="stat-card" @click="router.push('/search')">
          <div class="stat-num num-mono">{{ untaggedCount }}</div>
          <div class="stat-label">待打标</div>
        </el-card>
      </el-col>
      <el-col :span="4">
        <el-card shadow="hover" class="stat-card" @click="router.push('/search')">
          <div class="stat-num num-mono">{{ avgAesthetic }}</div>
          <div class="stat-label">平均美学分</div>
        </el-card>
      </el-col>
    </el-row>

    <el-card class="block" header="进行中任务">
      <el-empty v-if="taskStore.tasks.length === 0" description="暂无任务" :image-size="60" />
      <div v-for="t in taskStore.tasks" v-else :key="t.id" class="task-line">
        <el-progress
          :percentage="Math.round(t.progress * 100)"
          :status="t.status === 'failed' ? 'exception' : t.status === 'done' ? 'success' : undefined"
          :stroke-width="8"
        >
          <span class="task-label">{{ t.type }} #{{ t.id }}</span>
          <span class="num-mono">{{ t.done }}/{{ t.total }}</span>
        </el-progress>
      </div>
    </el-card>

    <el-card class="block" header="最近导入">
      <div class="recent">
        <el-tooltip v-for="img in libraryStore.images.slice(0, 12)" :key="img.id" :content="img.name">
          <el-image
            class="recent-thumb"
            :src="thumbUrl(img.thumbRel)"
            fit="cover"
            @click="router.push(`/library/${img.id}`)"
          >
            <template #error>
              <div class="recent-thumb" style="background: var(--el-fill-color-light)" />
            </template>
          </el-image>
        </el-tooltip>
      </div>
    </el-card>

    <el-card class="block" header="快捷筛选">
      <div class="quick-links">
        <el-button v-for="q in quickLinks" :key="q.label" size="large" @click="router.push(q.to)">
          {{ q.label }}<span class="quick-desc">{{ q.desc }}</span>
        </el-button>
      </div>
    </el-card>
  </div>
</template>

<style scoped>
.stat-card {
  cursor: pointer;
  text-align: center;
}
.stat-num {
  font-size: 26px;
  font-weight: 600;
}
.stat-label {
  color: var(--el-text-color-secondary);
  font-size: 13px;
  margin-top: 4px;
}
.stat-card.warn .stat-num {
  color: var(--el-color-warning);
}
.block {
  margin-top: 16px;
}
.task-line {
  margin-bottom: 12px;
}
.task-line .el-progress {
  display: flex;
  align-items: center;
  gap: 12px;
}
.task-label {
  font-size: 13px;
  margin-left: 8px;
}
.recent {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.recent-thumb {
  width: 88px;
  height: 66px;
  border-radius: 6px;
  cursor: pointer;
}
.quick-links {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
}
.quick-desc {
  color: var(--el-text-color-secondary);
  font-size: 12px;
  margin-left: 6px;
}
</style>
