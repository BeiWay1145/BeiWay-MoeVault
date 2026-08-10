<script setup lang="ts">
import { onMounted } from 'vue'
import { ElMessage } from 'element-plus'
import { useTaskStore } from '@/stores/tasks'

const taskStore = useTaskStore()

onMounted(() => taskStore.loadMock())

const statusTag = (s: string) =>
  ({ pending: 'info', running: 'primary', done: 'success', failed: 'danger', cancelled: 'info' })[s] as
    | 'info'
    | 'primary'
    | 'success'
    | 'danger'

function retry(id: number) {
  ElMessage.success(`重试任务 #${id}（骨架占位）`)
}
</script>

<template>
  <div class="tasks-page">
    <el-card class="block" header="进行中">
      <el-empty v-if="taskStore.tasks.filter((t) => t.status === 'running').length === 0" description="无进行中任务" :image-size="60" />
      <div v-for="t in taskStore.tasks.filter((x) => x.status === 'running')" :key="t.id" class="task-item">
        <div class="task-head">
          <span>{{ t.type }} #{{ t.id }}</span>
          <span class="num-mono">{{ t.done }}/{{ t.total }}</span>
        </div>
        <el-progress :percentage="Math.round(t.progress * 100)" :stroke-width="10" />
      </div>
    </el-card>

    <el-card class="block" header="失败与重试">
      <el-empty v-if="taskStore.tasks.filter((t) => t.status === 'failed').length === 0" description="无失败任务" :image-size="60" />
      <el-table :data="taskStore.tasks.filter((t) => t.status === 'failed')">
        <el-table-column prop="id" label="ID" width="80" />
        <el-table-column prop="type" label="类型" width="120" />
        <el-table-column prop="error" label="错误信息" />
        <el-table-column label="操作" width="160">
          <template #default="{ row }">
            <el-button size="small" type="primary" @click="retry(row.id)">重试</el-button>
            <el-button size="small">忽略</el-button>
          </template>
        </el-table-column>
      </el-table>
    </el-card>

    <el-card class="block" header="历史">
      <el-table :data="taskStore.tasks.filter((t) => t.status === 'done')">
        <el-table-column prop="id" label="ID" width="80" />
        <el-table-column prop="type" label="类型" />
        <el-table-column label="状态" width="100">
          <template #default="{ row }">
            <el-tag :type="statusTag(row.status)">{{ row.status }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="进度" width="160">
          <template #default="{ row }">
            <span class="num-mono">{{ row.done }}/{{ row.total }}</span>
          </template>
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
