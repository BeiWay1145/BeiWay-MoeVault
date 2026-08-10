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

/** 任务中心状态（骨架占位） */
export const useTaskStore = defineStore('tasks', () => {
  const tasks = ref<TaskItem[]>([])
  const running = ref(0)

  function loadMock() {
    tasks.value = [
      { id: 12, type: '导入批次', status: 'running', progress: 0.78, done: 128, total: 164 },
      { id: 13, type: 'SauceNAO 溯源', status: 'running', progress: 0.31, done: 2140, total: 6900 },
      { id: 14, type: '美学评分', status: 'done', progress: 1, done: 47, total: 47 },
      { id: 15, type: '打标', status: 'failed', progress: 0, done: 0, total: 1, error: '溯源失败: 相似度 32% < 75%' },
    ]
    running.value = tasks.value.filter((t) => t.status === 'running').length
  }

  return { tasks, running, loadMock }
})
