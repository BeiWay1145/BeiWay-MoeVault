import { defineStore } from 'pinia'
import { ref } from 'vue'

export interface TaskItem {
  id: number
  type: string
  status: 'pending' | 'running' | 'done' | 'failed' | 'cancelled'
  progress: number
  total: number
  done: number
  error?: string
}

/**
 * 任务中心状态。
 * 说明：任务队列（jobs 表）的后端查询 API 尚未实现，
 * 当前无数据时显示空态；后续接入 /api/v1/tasks 后填充。
 */
export const useTaskStore = defineStore('tasks', () => {
  const tasks = ref<TaskItem[]>([])
  const running = ref(0)

  /** 从后端加载任务（暂未实现，保持空数组）。 */
  async function load() {
    tasks.value = []
    running.value = 0
  }

  return { tasks, running, load }
})
