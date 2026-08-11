import { defineStore } from 'pinia'
import { ref } from 'vue'
import { get, post } from '@/api/client'
import { ElMessage } from 'element-plus'

export interface TaskItem {
  id: number
  type: string
  typeLabel: string
  status: 'pending' | 'running' | 'done' | 'failed' | 'cancelled'
  total: number
  done: number
  failed: number
  error?: string
  createdAt: number
  updatedAt: number
  finishedAt?: number | null
}

/** 任务类型 → 提示文案。 */
export const taskKindLabel: Record<string, string> = {
  tag: '打标',
  aesthetic: '美学评分',
  sauce: 'SauceNAO 溯源',
  'ai-detect': 'AI 生成检测',
  import: '导入',
}

/**
 * 任务中心：轮询 /api/v1/tasks 获取进度与历史（后端 jobs 表持久化）。
 * 提交任务时弹顶部"已添加任务"；轮询发现新完成/失败任务时弹"任务完成"。
 */
export const useTaskStore = defineStore('tasks', () => {
  const tasks = ref<TaskItem[]>([])
  const running = ref(0)
  /** 已通知过完成的任务 id 集合（避免重复弹窗）。 */
  const notified = ref<Set<number>>(new Set())
  /** 首次加载已完成（初始化集合，不弹历史通知）。 */
  let initialized = false
  let timer: number | undefined

  async function load() {
    try {
      const d = await get<{ items: TaskItem[] }>('/tasks?limit=100')
      tasks.value = d.items
      running.value = d.items.filter((t) => t.status === 'running' || t.status === 'pending').length
      if (!initialized) {
        // 首次加载：把现有完成/失败任务记入已通知集合（历史任务不再重复提示），
        // 之后轮询只提示本会话新出现的完成/失败任务。
        for (const t of d.items) {
          if (t.status === 'done' || t.status === 'failed') notified.value.add(t.id)
        }
        initialized = true
        return
      }
      // 检测新完成的任务 → 顶部通知
      for (const t of d.items) {
        if ((t.status === 'done' || t.status === 'failed') && !notified.value.has(t.id)) {
          notified.value.add(t.id)
          notifyFinished(t)
        }
      }
    } catch {
      /* 后端不可用时静默 */
    }
  }

  /** 立即加载一次并启动轮询（页面挂载时调用）。 */
  function start() {
    load()
    if (timer !== undefined) return
    timer = window.setInterval(load, 3000)
  }

  /** 停止轮询（页面卸载时调用）。 */
  function stop() {
    if (timer !== undefined) {
      window.clearInterval(timer)
      timer = undefined
    }
  }

  /** 顶部提示：任务已加入队列。 */
  function notifyEnqueued(kind: string, jobId: number, count: number) {
    const label = taskKindLabel[kind] ?? kind
    ElMessage({
      message: `${label}任务 #${jobId} 已加入队列${count > 0 ? `（${count} 张）` : ''}，可在右上角任务中心查看进度`,
      type: 'info',
      duration: 3000,
      showClose: true,
    })
  }

  function notifyFinished(t: TaskItem) {
    const label = t.typeLabel || taskKindLabel[t.type] || t.type
    if (t.status === 'done') {
      ElMessage({
        message: `${label}任务 #${t.id} 已完成：成功 ${t.done} 张${t.failed > 0 ? `，失败 ${t.failed} 张` : ''}`,
        type: 'success',
        duration: 5000,
        showClose: true,
      })
    } else {
      ElMessage({
        message: `${label}任务 #${t.id} 失败：${t.error ?? '未知错误'}`,
        type: 'error',
        duration: 6000,
        showClose: true,
      })
    }
  }

  /** 提交打标任务（force_ids 指定图片）。 */
  async function enqueueTag(ids: number[]) {
    const r = await post<{ started: boolean; job_id: number; kind: string }>('/tagging/run', { force_ids: ids })
    notifyEnqueued(r.kind, r.job_id, ids.length)
    load()
    return r
  }

  /** 提交美学任务。 */
  async function enqueueAesthetic(ids: number[]) {
    const r = await post<{ started: boolean; job_id: number; kind: string }>('/aesthetic/run', { force_ids: ids })
    notifyEnqueued(r.kind, r.job_id, ids.length)
    load()
    return r
  }

  /** 提交 SauceNAO 溯源任务。 */
  async function enqueueSauce(ids: number[]) {
    const r = await post<{ started: boolean; job_id: number; kind: string }>('/sauce/run', { force_ids: ids })
    notifyEnqueued(r.kind, r.job_id, ids.length)
    load()
    return r
  }

  /** 重置通知状态（用于测试/切换）。 */
  function resetNotified() {
    notified.value = new Set()
  }

  /** 中断任务（置 cancelled，worker 检测后停止）。 */
  async function cancelTask(id: number) {
    await post(`/tasks/${id}/cancel`)
    load()
  }

  /** 继续被中断的任务（重新从 payload 入队，已处理图自动跳过）。 */
  async function resumeTask(id: number) {
    await post(`/tasks/${id}/resume`)
    load()
  }

  return {
    tasks,
    running,
    load,
    start,
    stop,
    notifyEnqueued,
    enqueueTag,
    enqueueAesthetic,
    enqueueSauce,
    cancelTask,
    resumeTask,
    resetNotified,
  }
})
