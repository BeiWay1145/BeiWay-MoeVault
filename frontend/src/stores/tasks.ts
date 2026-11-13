import { defineStore } from 'pinia'
import { ref } from 'vue'
import { get, post } from '@/api/client'
import { ElMessage } from 'element-plus'
import { reportLog } from '@/api/log'

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
  dedup: '查重',
  export: '导出',
  replace: '原图替换',
}

/** 后端返回的原始任务（snake_case）。 */
interface RawTask {
  id: number
  type: string
  type_label?: string
  status: string
  total?: number
  done?: number
  failed?: number
  error?: string | null
  created_at?: number
  updated_at?: number
  finished_at?: number | null
}

/**
 * 归一化：后端 JSON 是 snake_case，前端统一用 camelCase。
 * 此前直接按 camelCase 读取导致类型/时间全是 undefined（任务中心"没有有效信息"的主因）。
 */
export function normalizeTask(r: RawTask): TaskItem {
  return {
    id: r.id,
    type: r.type,
    typeLabel: r.type_label || taskKindLabel[r.type] || r.type,
    status: r.status as TaskItem['status'],
    total: r.total ?? 0,
    done: r.done ?? 0,
    failed: r.failed ?? 0,
    error: r.error ?? undefined,
    createdAt: r.created_at ?? 0,
    updatedAt: r.updated_at ?? 0,
    finishedAt: r.finished_at ?? null,
  }
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
  /** 增强3：任务完成回调注册表（job_id → callback），单张溯源比大小用。 */
  const doneCallbacks = new Map<number, (t: TaskItem) => void>()
  /** 首次加载已完成（初始化集合，不弹历史通知）。 */
  let initialized = false
  let timer: number | undefined

  // 架构修复：WS 任务的完成/失败事件到达时立刻刷新，不再只依赖 3s 轮询。
  // 轮询一旦被意外停掉（曾经离开任务中心就会发生），这里仍能收到通知。
  let wsBound = false
  function bindWsEvents() {
    if (wsBound) return
    wsBound = true
    window.addEventListener('moevault:task-event', () => {
      load().catch(() => {})
    })
    // 服务端丢过事件 → 状态可能已不一致，重拉一次权威数据
    window.addEventListener('moevault:resync', () => {
      initialized = false
      load().catch(() => {})
    })
  }

  async function load() {
    try {
      const d = await get<{ items: RawTask[] }>('/tasks?limit=100')
      const items = (d.items ?? []).map(normalizeTask)
      tasks.value = items
      running.value = items.filter((t) => t.status === 'running' || t.status === 'pending').length
      if (!initialized) {
        // 首次加载：把现有完成/失败任务记入已通知集合（历史任务不再重复提示），
        // 之后轮询只提示本会话新出现的完成/失败任务。
        for (const t of items) {
          if (t.status === 'done' || t.status === 'failed' || t.status === 'cancelled') notified.value.add(t.id)
        }
        initialized = true
        return
      }
      // 检测新完成/中断的任务 → 顶部通知
      for (const t of items) {
        if (
          (t.status === 'done' || t.status === 'failed' || t.status === 'cancelled') &&
          !notified.value.has(t.id)
        ) {
          notified.value.add(t.id)
          notifyFinished(t)
          const cb = doneCallbacks.get(t.id)
          if (cb) {
            doneCallbacks.delete(t.id)
            cb(t)
          }
        }
      }
    } catch {
      /* 后端不可用时静默 */
    }
  }

  // 轮询订阅者计数。
  //
  // 为什么需要它：轮询是**全局**的（AppLayout 常驻），但任务中心自己也调 start/stop。
  // 之前 stop() 无条件清 timer，于是「进任务中心 → 离开」就把全局轮询永久掐死了，
  // 之后任何页面都不会再收到任务完成通知。
  // 改成引用计数：谁 start 谁来 stop，最后一个订阅者走了才真正停。
  let subscribers = 0

  /** 立即加载一次并启动轮询（页面挂载时调用，可重入）。 */
  function start() {
    subscribers += 1
    bindWsEvents()
    load()
    if (timer !== undefined) return
    timer = window.setInterval(load, 3000)
  }

  /** 释放一个订阅者；全部释放后才停止轮询。 */
  function stop() {
    subscribers = Math.max(0, subscribers - 1)
    if (subscribers > 0) return
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
    } else if (t.status === 'cancelled') {
      // 中断是用户主动行为，用警告色而不是错误色——
      // 之前被中断的任务最终会被写成 done，用户根本看不出中断生没生效。
      ElMessage({
        message: `${label}任务 #${t.id} 已中断：成功 ${t.done} 张，失败 ${t.failed} 张`,
        type: 'warning',
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
    reportLog(`提交批量打标任务 #${r.job_id}（${ids.length} 张）`)
    load()
    return r
  }

  /** 提交美学任务。force=true 时忽略「已有美学分」过滤，强制重新评分（覆盖旧分数）。 */
  async function enqueueAesthetic(ids: number[], force = false) {
    const r = await post<{ started: boolean; job_id: number; kind: string }>('/aesthetic/run', {
      force_ids: ids,
      force,
    })
    notifyEnqueued(r.kind, r.job_id, ids.length)
    reportLog(`提交批量美学任务 #${r.job_id}（${ids.length} 张${force ? '，强制重评' : ''}）`)
    load()
    return r
  }

  /** 提交 SauceNAO 溯源任务（force=true 时忽略不可溯源标记，强制重试；onDone 任务完成回调）。 */
  async function enqueueSauce(ids: number[], force = false, autoReplace = false, onDone?: (t: TaskItem) => void) {
    const r = await post<{ started: boolean; job_id: number; kind: string }>('/sauce/run', {
      force_ids: ids,
      force_sauce: force,
      auto_replace: autoReplace,
    })
    notifyEnqueued(r.kind, r.job_id, ids.length)
    reportLog(`提交批量溯源任务 #${r.job_id}（${ids.length} 张${force ? '，强制重试' : ''}${autoReplace ? '，智能替换' : ''}）`)
    if (onDone) doneCallbacks.set(r.job_id, onDone)
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
