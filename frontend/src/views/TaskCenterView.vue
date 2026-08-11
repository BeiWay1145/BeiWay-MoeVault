<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'
import { useTaskStore } from '@/stores/tasks'

const taskStore = useTaskStore()

onMounted(() => taskStore.start())
onUnmounted(() => taskStore.stop())

const statusTag = (s: string) =>
  ({ pending: 'info', running: 'primary', done: 'success', failed: 'danger', cancelled: 'info' })[s] as
    | 'info'
    | 'primary'
    | 'success'
    | 'danger'

const statusText = (s: string) =>
  ({ pending: '等待中', running: '进行中', done: '已完成', failed: '失败', cancelled: '已取消' })[s] ?? s

const runningTasks = computed(() => taskStore.tasks.filter((t) => t.status === 'running' || t.status === 'pending'))
const failedTasks = computed(() => taskStore.tasks.filter((t) => t.status === 'failed'))
const doneTasks = computed(() => taskStore.tasks.filter((t) => t.status === 'done'))

function fmtTime(ts: number | null | undefined) {
  if (!ts) return '—'
  return new Date(ts * 1000).toLocaleString()
}
</script>

<template>
  <div class="tasks-page">
    <el-card class="block" header="进行中">
      <el-empty
        v-if="runningTasks.length === 0"
        description="无进行中任务"
        :image-size="60"
      />
      <div v-for="t in runningTasks" :key="t.id" class="task-item">
        <div class="task-head">
          <span>{{ t.typeLabel }} #{{ t.id }}</span>
          <span class="num-mono">{{ t.done }}/{{ t.total }}</span>
        </div>
        <el-progress :percentage="t.total > 0 ? Math.round((t.done / t.total) * 100) : 0" :stroke-width="10" />
      </div>
    </el-card>

    <el-card class="block" header="失败与重试">
      <el-empty
        v-if="failedTasks.length === 0"
        description="无失败任务"
        :image-size="60"
      />
      <el-table :data="failedTasks">
        <el-table-column prop="id" label="ID" width="70" />
        <el-table-column prop="typeLabel" label="类型" width="110" />
        <el-table-column prop="error" label="错误信息" />
        <el-table-column label="时间" width="150">
          <template #default="{ row }">{{ fmtTime(row.createdAt) }}</template>
        </el-table-column>
      </el-table>
    </el-card>

    <el-card class="block" header="历史">
      <el-empty
        v-if="doneTasks.length === 0"
        description="暂无历史任务"
        :image-size="60"
      />
      <el-table :data="doneTasks">
        <el-table-column prop="id" label="ID" width="70" />
        <el-table-column prop="typeLabel" label="类型" width="110" />
        <el-table-column label="状态" width="100">
          <template #default="{ row }">
            <el-tag :type="statusTag(row.status)">{{ statusText(row.status) }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="进度" width="120">
          <template #default="{ row }">
            <span class="num-mono">{{ row.done }}/{{ row.total }}</span>
          </template>
        </el-table-column>
        <el-table-column label="创建时间" width="150">
          <template #default="{ row }">{{ fmtTime(row.createdAt) }}</template>
        </el-table-column>
        <el-table-column label="完成时间" width="150">
          <template #default="{ row }">{{ fmtTime(row.finishedAt) }}</template>
        </el-table-column>
      </el-table>
    </el-card>
  </div>
</template>

<style scoped>
.tasks-page {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.task-item {
  margin-bottom: 14px;
}
.task-head {
  display: flex;
  justify-content: space-between;
  margin-bottom: 4px;
  font-size: 13px;
}
</style>
