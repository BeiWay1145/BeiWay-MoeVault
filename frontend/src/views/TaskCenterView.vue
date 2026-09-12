<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useTaskStore, taskKindLabel, type TaskItem } from '@/stores/tasks'
import { get, del } from '@/api/client'

const taskStore = useTaskStore()

onMounted(() => {
  taskStore.start()
  nowTimer = window.setInterval(() => {
    nowSec.value = Math.floor(Date.now() / 1000)
  }, 1000)
})
onUnmounted(() => {
  taskStore.stop()
  if (nowTimer !== undefined) window.clearInterval(nowTimer)
})

/** 每秒跳动的时间戳：让进行中任务的耗时/ETA 实时刷新。 */
const nowSec = ref(Math.floor(Date.now() / 1000))
let nowTimer: number | undefined

/** 任务详情弹窗。 */
const detailVisible = ref(false)
const detail = ref<Record<string, unknown> | null>(null)
async function openDetail(id: number) {
  try {
    detail.value = await get<Record<string, unknown>>(`/tasks/${id}`)
    detailVisible.value = true
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function clearHistory() {
  try {
    await ElMessageBox.confirm('清空所有已完成/失败/已中断的历史任务？进行中的任务保留。', '清空历史任务', {
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

async function onResume(id: number) {
  try {
    await taskStore.resumeTask(id)
    ElMessage.success('任务已重新开始，已处理的图片将自动跳过')
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

// ---------- 统计与分类 ----------
/** 状态筛选：all / running / done / failed / cancelled。 */
const statusFilter = ref<'all' | 'running' | 'done' | 'failed' | 'cancelled'>('all')
/** 列表条数。 */
const listLimit = ref(50)
const listLimitOptions = [20, 50, 100, 200]

const isActive = (t: TaskItem) => t.status === 'running' || t.status === 'pending'
const runningTasks = computed(() => taskStore.tasks.filter(isActive))
const failedTasks = computed(() => taskStore.tasks.filter((t) => t.status === 'failed'))
const doneTasks = computed(() => taskStore.tasks.filter((t) => t.status === 'done'))
const cancelledTasks = computed(() => taskStore.tasks.filter((t) => t.status === 'cancelled'))

const stats = computed(() => [
  { key: 'running' as const, label: '进行中', value: runningTasks.value.length, type: 'primary' as const },
  { key: 'done' as const, label: '已完成', value: doneTasks.value.length, type: 'success' as const },
  { key: 'failed' as const, label: '失败', value: failedTasks.value.length, type: 'danger' as const },
  { key: 'cancelled' as const, label: '已中断', value: cancelledTasks.value.length, type: 'info' as const },
])

/** 按筛选后的任务列表（新→旧，限制条数）。 */
const visibleTasks = computed(() => {
  const all = [...taskStore.tasks].sort((a, b) => b.id - a.id)
  const filtered =
    statusFilter.value === 'all'
      ? all
      : statusFilter.value === 'running'
        ? all.filter(isActive)
        : all.filter((t) => t.status === statusFilter.value)
  return filtered.slice(0, listLimit.value)
})

const statusTag = (s: string) =>
  (({ pending: 'info', running: 'primary', done: 'success', failed: 'danger', cancelled: 'info' })[s] ??
    'info') as 'info' | 'primary' | 'success' | 'danger'

const statusText = (s: string) =>
  ({ pending: '等待中', running: '进行中', done: '已完成', failed: '失败', cancelled: '已中断' })[s] ?? s

const kindTagType = (ty: string) =>
  (({ tag: 'primary', aesthetic: 'success', sauce: 'warning', import: 'info' })[ty] ?? 'info') as
    | 'primary'
    | 'success'
    | 'warning'
    | 'info'

const progressPct = (t: TaskItem) =>
  t.total > 0 ? Math.min(100, Math.round((t.done / t.total) * 100)) : isActive(t) ? 0 : 100

/** 耗时（秒）：进行中算到当前，结束的任务算到 finished_at。 */
function elapsedOf(t: TaskItem) {
  const end = t.finishedAt ?? (isActive(t) ? nowSec.value : t.updatedAt)
  return Math.max(0, end - t.createdAt)
}

/** 处理速度（张/分钟）：仅有耗时与进度时给出。 */
function speedOf(t: TaskItem) {
  const el = elapsedOf(t)
  if (el <= 0 || t.done <= 0) return null
  return (t.done / el) * 60
}

/** 预计剩余时间（秒）：按当前速度外推。 */
function etaOf(t: TaskItem) {
  if (!isActive(t) || t.total <= 0) return null
  const sp = speedOf(t)
  if (!sp || sp <= 0) return null
  const remain = Math.max(0, t.total - t.done)
  return Math.round((remain / sp) * 60)
}

function fmtDuration(sec: number | null | undefined) {
  if (sec === null || sec === undefined) return '—'
  const s = Math.max(0, Math.round(sec))
  if (s < 60) return `${s} 秒`
  const m = Math.floor(s / 60)
  const rs = s % 60
  if (m < 60) return rs > 0 ? `${m} 分 ${rs} 秒` : `${m} 分`
  const h = Math.floor(m / 60)
  return `${h} 小时 ${m % 60} 分`
}

function fmtTime(ts: number | null | undefined) {
  if (!ts) return '—'
  return new Date(ts * 1000).toLocaleString()
}

/** 失败行高亮（Element Plus 行样式回调）。 */
function rowClass({ row }: { row: TaskItem }) {
  return row.status === 'failed' ? 'row-failed' : ''
}

/** 错误摘要（表格里最多一行，完整内容在详情/悬浮中）。 */
function errBrief(e?: string) {
  if (!e) return '—'
  return e.length > 60 ? `${e.slice(0, 60)}…` : e
}
</script>

<template>
  <div class="tasks-page">
    <!-- 概览：点击卡片即筛选 -->
    <div class="stat-row">
      <div
        v-for="s in stats"
        :key="s.key"
        class="stat-card"
        :class="{ active: statusFilter === s.key }"
        @click="statusFilter = statusFilter === s.key ? 'all' : s.key"
      >
        <div class="stat-value" :class="`is-${s.type}`">{{ s.value }}</div>
        <div class="stat-label">{{ s.label }}</div>
      </div>
    </div>

    <!-- 进行中：卡片式，含速度/ETA -->
    <el-card class="block" shadow="never">
      <template #header>
        <div class="card-header">
          <span>进行中（{{ runningTasks.length }}）</span>
          <span class="hint">每 3 秒自动刷新</span>
        </div>
      </template>
      <el-empty v-if="runningTasks.length === 0" description="当前没有进行中的任务" :image-size="60" />
      <div v-for="t in runningTasks" :key="t.id" class="run-item">
        <div class="run-head">
          <el-tag :type="kindTagType(t.type)" size="small">{{ t.typeLabel }}</el-tag>
          <span class="run-id">#{{ t.id }}</span>
          <el-tag :type="statusTag(t.status)" size="small" effect="plain">{{ statusText(t.status) }}</el-tag>
          <span class="run-count">
            {{ t.done }} / {{ t.total }}
            <span v-if="t.failed > 0" class="fail-count">（失败 {{ t.failed }}）</span>
          </span>
          <span class="spacer" />
          <el-button size="small" text type="primary" @click="openDetail(t.id)">详情</el-button>
          <el-button size="small" type="danger" plain @click="onCancel(t.id)">中断</el-button>
        </div>
        <el-progress
          :percentage="progressPct(t)"
          :stroke-width="10"
          :status="t.failed > 0 ? 'warning' : undefined"
        />
        <div class="run-meta">
          <span>已用 <b>{{ fmtDuration(elapsedOf(t)) }}</b></span>
          <span v-if="speedOf(t) !== null">速度 <b>{{ speedOf(t)!.toFixed(1) }}</b> 张/分</span>
          <span v-if="etaOf(t) !== null">预计剩余 <b>{{ fmtDuration(etaOf(t)) }}</b></span>
          <span v-if="t.total - t.done > 0">待处理 {{ t.total - t.done }} 张</span>
        </div>
      </div>
    </el-card>

    <!-- 全部任务 -->
    <el-card class="block" shadow="never">
      <template #header>
        <div class="card-header">
          <span>全部任务</span>
          <div class="header-actions">
            <el-select v-model="statusFilter" size="small" style="width: 120px">
              <el-option value="all" label="全部状态" />
              <el-option value="running" label="进行中" />
              <el-option value="done" label="已完成" />
              <el-option value="failed" label="失败" />
              <el-option value="cancelled" label="已中断" />
            </el-select>
            <el-select v-model="listLimit" size="small" style="width: 100px">
              <el-option v-for="n in listLimitOptions" :key="n" :value="n" :label="`${n} 条`" />
            </el-select>
            <el-button size="small" @click="taskStore.load()">刷新</el-button>
            <el-button size="small" type="danger" plain @click="clearHistory">清空历史</el-button>
          </div>
        </div>
      </template>
      <el-empty v-if="visibleTasks.length === 0" description="没有符合条件的任务" :image-size="60" />
      <el-table v-else :data="visibleTasks" size="small" :row-class-name="rowClass">
        <el-table-column prop="id" label="ID" width="64" />
        <el-table-column label="类型" width="120">
          <template #default="{ row }">
            <el-tag :type="kindTagType(row.type)" size="small">{{ row.typeLabel }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="状态" width="88">
          <template #default="{ row }">
            <el-tag :type="statusTag(row.status)" size="small" effect="plain">{{ statusText(row.status) }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="进度" min-width="180">
          <template #default="{ row }">
            <div class="cell-progress">
              <el-progress
                :percentage="progressPct(row)"
                :stroke-width="8"
                :show-text="false"
                :status="row.failed > 0 ? 'warning' : undefined"
              />
              <span class="cell-progress-text">
                {{ row.done }}/{{ row.total }}<span v-if="row.failed > 0" class="fail-count"> · 失败 {{ row.failed }}</span>
              </span>
            </div>
          </template>
        </el-table-column>
        <el-table-column label="耗时" width="110">
          <template #default="{ row }">{{ fmtDuration(elapsedOf(row)) }}</template>
        </el-table-column>
        <el-table-column label="开始时间" width="170">
          <template #default="{ row }">{{ fmtTime(row.createdAt) }}</template>
        </el-table-column>
        <el-table-column label="错误" min-width="160">
          <template #default="{ row }">
            <el-tooltip v-if="row.error" :content="row.error" placement="top" :show-after="200">
              <span class="err-text">{{ errBrief(row.error) }}</span>
            </el-tooltip>
            <span v-else class="hint">—</span>
          </template>
        </el-table-column>
        <el-table-column label="操作" width="150" fixed="right">
          <template #default="{ row }">
            <el-button size="small" text type="primary" @click="openDetail(row.id)">详情</el-button>
            <el-button v-if="isActive(row)" size="small" text type="danger" @click="onCancel(row.id)">中断</el-button>
            <el-button v-if="row.status === 'cancelled'" size="small" text type="primary" @click="onResume(row.id)">继续</el-button>
          </template>
        </el-table-column>
      </el-table>
    </el-card>

    <!-- 任务详情 -->
    <el-dialog v-model="detailVisible" title="任务详情" width="600px">
      <template v-if="detail">
        <el-descriptions :column="2" border size="small">
          <el-descriptions-item label="ID">#{{ detail.id }}</el-descriptions-item>
          <el-descriptions-item label="类型">{{ detail.type_label ?? taskKindLabel[String(detail.type)] ?? detail.type }}</el-descriptions-item>
          <el-descriptions-item label="状态">
            <el-tag :type="statusTag(String(detail.status))">{{ statusText(String(detail.status)) }}</el-tag>
          </el-descriptions-item>
          <el-descriptions-item label="进度">
            {{ detail.done }}/{{ detail.total }}（失败 {{ detail.failed }}）
          </el-descriptions-item>
          <el-descriptions-item label="创建时间">{{ fmtTime(detail.created_at as number) }}</el-descriptions-item>
          <el-descriptions-item label="结束时间">{{ fmtTime(detail.finished_at as number) }}</el-descriptions-item>
        </el-descriptions>
        <div v-if="detail.error" class="detail-error">
          <div class="detail-error-title">错误信息</div>
          <pre class="err-text">{{ detail.error }}</pre>
        </div>
        <div v-if="detail.payload" class="detail-error">
          <div class="detail-error-title">请求负载</div>
          <pre class="payload">{{ detail.payload }}</pre>
        </div>
        <div v-if="detail.type === 'sauce' && (detail.keys_usage as unknown[])?.length" class="usage-block">
          <div class="detail-error-title">SauceNAO 密钥额度消耗</div>
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
.stat-row {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
}
.stat-card {
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 8px;
  padding: 12px 16px;
  cursor: pointer;
  transition: all 0.15s;
  background: var(--el-bg-color);
}
.stat-card:hover {
  border-color: var(--el-color-primary-light-5);
}
.stat-card.active {
  border-color: var(--el-color-primary);
  box-shadow: 0 0 0 1px var(--el-color-primary-light-7) inset;
}
.stat-value {
  font-size: 24px;
  font-weight: 600;
  line-height: 1.2;
}
.stat-value.is-primary {
  color: var(--el-color-primary);
}
.stat-value.is-success {
  color: var(--el-color-success);
}
.stat-value.is-danger {
  color: var(--el-color-danger);
}
.stat-value.is-info {
  color: var(--el-text-color-secondary);
}
.stat-label {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.block {
  margin-bottom: 0;
}
.card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.header-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
.run-item {
  padding: 10px 0;
  border-bottom: 1px dashed var(--el-border-color-lighter);
}
.run-item:last-child {
  border-bottom: none;
}
.run-head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
  font-size: 13px;
}
.run-id {
  font-family: monospace;
  color: var(--el-text-color-secondary);
}
.run-count {
  font-family: monospace;
}
.spacer {
  flex: 1;
}
.fail-count {
  color: var(--el-color-danger);
}
.run-meta {
  margin-top: 6px;
  display: flex;
  flex-wrap: wrap;
  gap: 16px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.run-meta b {
  color: var(--el-text-color-primary);
}
.cell-progress {
  display: flex;
  align-items: center;
  gap: 8px;
}
.cell-progress :deep(.el-progress) {
  flex: 1;
  min-width: 60px;
}
.cell-progress-text {
  font-family: monospace;
  font-size: 12px;
  white-space: nowrap;
}
.err-text {
  color: var(--el-color-danger);
  white-space: pre-wrap;
  word-break: break-all;
  font-size: 12px;
}
.hint {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.detail-error {
  margin-top: 12px;
}
.detail-error-title {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 6px;
}
.payload {
  font-size: 11px;
  word-break: break-all;
  white-space: pre-wrap;
  margin: 0;
}
.usage-block {
  margin-top: 12px;
}
:deep(.row-failed) {
  background: var(--el-color-danger-light-9);
}
</style>
