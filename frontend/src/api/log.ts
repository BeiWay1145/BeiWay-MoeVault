/**
 * 前端操作日志上报：批量打标/溯源/美学/检测AI/删除、导入、设置修改等关键操作。
 * 通过 POST /api/v1/logs 写入后端 app_logs 表（设置页日志面板可查看）。
 */
import { post } from '@/api/client'

export function reportLog(message: string, level: 'info' | 'warn' | 'error' = 'info') {
  // 静默失败：日志上报不阻塞主流程
  post('/logs', { level, category: 'frontend', message }).catch(() => {})
}
