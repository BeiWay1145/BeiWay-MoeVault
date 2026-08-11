<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useTaskStore } from '@/stores/tasks'
import { get, del } from '@/api/client'

const taskStore = useTaskStore()

onMounted(() => taskStore.start())
onUnmounted(() => taskStore.stop())

// E8: 任务详情弹窗
const detailVisible = ref(false)
const detail = ref<Record<string, unknown> | null>(null)
async function openDetail(id: number) {
  try {
    const d = await get<Record<string, unknown>>(`/tasks/${id}`)
    detail.value = d
    detailVisible.value = true
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

// E8: 清空历史任务
async function clearHistory() {
  try {
    await ElMessageBox.confirm('清空所有已完成/失败的历史任务？进行中的任务保留。', '清空历史任务', {
      type: 'warning',
      confirmButtonText: '清空',
    })
  } catch {
    return
  }
  try {
    const r = await del<{ cleared: number }>('/tasks')
    ElMessage.success(`已清空 ${r.cleared} 条历史任务`)
    await taskStore.load()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

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
const cancelledTasks = computed(() => taskStore.tasks.filter((t) => t.status === 'cancelled'))

/** 中断任务。 */
async function onCancel(id: number) {
  try {
    await ElMessageBox.confirm('中断该任务？已处理的图片保留，未处理的可稍后「继续」。', '中断任务', {
      type: 'warning',
      confirmButtonText: '中断',
    })
  } catch {
    return
  }
  try {
    await taskStore.cancelTask(id)
    ElMessage.success('任务已中断')
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 继续被中断的任务。 */
async function onResume(id: number) {
  try {
    await taskStore.resumeTask(id)
    ElMessage.success('任务已重新开始，已处理的图片将自动跳过')
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

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
          <el-button size="small" type="danger" plain @click="onCancel(t.id)">中断</el-button>
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

    <el-card class="block" header="已中断（可继续）">
      <el-empty
        v-if="cancelledTasks.length === 0"
        description="无已中断任务"
        :image-size="60"
      />
      <el-table :data="cancelledTasks">
        <el-table-column prop="id" label="ID" width="70" />
        <el-table-column prop="typeLabel" label="类型" width="110" />
        <el-table-column label="进度" width="120">
          <template #default="{ row }">
            <span class="num-mono">{{ row.done }}/{{ row.total }}</span>
          </template>
        </el-table-column>
        <el-table-column label="创建时间" width="150">
          <template #default="{ row }">{{ fmtTime(row.createdAt) }}</template>
        </el-table-column>
        <el-table-column label="操作" width="110">
          <template #default="{ row }">
            <el-button size="small" type="primary" plain @click="onResume(row.id)">继续</el-button>
          </template>
        </el-table-column>
      </el-table>
    </el-card>

    <el-card class="block" header="历史">
      <template #header>
        <div class="card-header">
          <span>历史</span>
          <el-button size="small" type="danger" plain @click="clearHistory">清空历史任务</el-button>
        </div>
      </template>
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
        <el-table-column label="操作" width="90">
          <template #default="{ row }">
            <el-button size="small" text type="primary" @click="openDetail(row.id)">详情</el-button>
          </template>
        </el-table-column>
      </el-table>
    </el-card>

    <!-- 任务详情弹窗（E8）：错误信息 + 各 key 额度消耗 -->
    <el-dialog v-model="detailVisible" title="任务详情" width="560px">
      <template v-if="detail">
        <el-descriptions :column="1" border size="small">
          <el-descriptions-item label="ID">#{{ detail.id }}</el-descriptions-item>
          <el-descriptions-item label="类型">{{ detail.type_label }}</el-descriptions-item>
          <el-descriptions-item label="状态">
            <el-tag :type="statusTag(String(detail.status))">{{ statusText(String(detail.status)) }}</el-tag>
          </el-descriptions-item>
          <el-descriptions-item label="进度">{{ detail.done }}/{{ detail.total }}（失败 {{ detail.failed }}）</el-descriptions-item>
          <el-descriptions-item label="错误" v-if="detail.error">
            <span class="err-text">{{ detail.error }}</span>
          </el-descriptions-item>
          <el-descriptions-item label="请求负载" v-if="detail.payload">
            <code class="payload">{{ detail.payload }}</code>
          </el-descriptions-item>
        </el-descriptions>
        <div v-if="(detail.keys_usage as unknown[])?.length" class="usage-block">
          <div class="usage-title">SauceNAO 密钥额度消耗</div>
          <el-table :data="detail.keys_usage as unknown[]" size="small">
            <el-table-column prop="name" label="名称" width="100" />
            <el-table-column prop="total_requests" label="请求数" width="90" />
            <el-table-column prop="long_remaining" label="当日剩余" width="100" />
            <el-table-column label="状态" width="90">
              <template #default="{ row }">
                <el-tag v-if="row.daily_paused" type="danger" size="small">已停用</el-tag>
                <el-tag v-else-if="(row.cooldown_secs as number) > 0" type="warning" size="small">冷却中</el-tag>
                <el-tag v-else type="success" size="small">可用</el-tag>
              </template>
            </el-table-column>
          </el-table>
        </div>
      </template>
    </el-dialog>
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
.card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.err-text {
  color: var(--el-color-danger);
  white-space: pre-wrap;
  word-break: break-all;
}
.payload {
  font-size: 11px;
  word-break: break-all;
}
.usage-block {
  margin-top: 12px;
}
.usage-title {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 8px;
}
</style>
