/**
 * BUG 追踪器（增强版）：三态（关闭/本会话开启/开启）+ 全量追踪。
 * - 状态：localStorage `moevault-bug-tracker`='1' 持久开启；sessionStorage `moevault-bug-tracker-session`='1' 本会话开启
 * - 记录内容：API 请求（method/URL/状态/耗时/错误体，自动脱敏）+ 前端错误堆栈 + 关键操作
 * - 存储：POST /logs 写入后端 app_logs 表（category=track）；退出由桌面壳自动转储
 */
import { post } from '@/api/client'

const KEY = 'moevault-bug-tracker'
const SESSION_KEY = 'moevault-bug-tracker-session'

export type TrackerState = 'off' | 'session' | 'on'

export function getTrackerState(): TrackerState {
  const persistent = localStorage.getItem(KEY) === '1'
  const session = sessionStorage.getItem(SESSION_KEY) === '1'
  if (persistent) return 'on'
  if (session) return 'session'
  return 'off'
}

export function setTrackerState(s: TrackerState) {
  if (s === 'on') {
    localStorage.setItem(KEY, '1')
    sessionStorage.setItem(SESSION_KEY, '1')
  } else if (s === 'session') {
    localStorage.removeItem(KEY)
    sessionStorage.setItem(SESSION_KEY, '1')
  } else {
    localStorage.removeItem(KEY)
    sessionStorage.removeItem(SESSION_KEY)
  }
}

export function isTracking(): boolean {
  return getTrackerState() !== 'off'
}

/** 敏感字段脱敏：URL 参数 / 常见 key 名 → ***。 */
export function redact(text: string): string {
  // query 参数中的敏感字段
  let out = text.replace(/([?&](?:key|api_key|url|path|file|src|token)=)[^&\s"]*/gi, '$1***')
  // 常见磁盘路径打码
  out = out.replace(/([A-Za-z]:\\[^"'\s,}\]]{4,})/g, (m) => m.slice(0, 8) + '…\\***')
  return out
}

let logQueue: string[] = []
let flushTimer: number | undefined

/** 异步上报（批量合并，避免高频请求打爆后端）。 */
export function trackLog(detail: Record<string, unknown>, level: 'info' | 'warn' | 'error' = 'info') {
  if (!isTracking()) return
  try {
    logQueue.push(JSON.stringify(detail))
    if (flushTimer !== undefined) return
    flushTimer = window.setTimeout(() => {
      flushTimer = undefined
      const batch = logQueue
      logQueue = []
      // 脱敏后合并上报
      const msg = batch.join('\n')
      post('/logs', { level, category: 'track', message: redact(msg).slice(0, 6000) }).catch(() => {})
    }, 800)
  } catch {
    /* 静默 */
  }
}

/** 记录一次 API 请求（由 client 拦截器调用）。 */
export function trackApi(method: string, path: string, status: number | null, ms: number, errBody?: string) {
  trackLog({ t: 'api', m: method, p: redact(path).slice(0, 500), s: status, ms: Math.round(ms), e: errBody?.slice(0, 400) })
}

/** 记录前端错误（window.onerror / unhandledrejection / Vue errorHandler）。 */
export function trackError(source: string, message: string, stack?: string, extra?: Record<string, unknown>) {
  trackLog({ t: 'err', src: source, msg: redact(message).slice(0, 500), stack: stack?.slice(0, 1500), route: location.hash || location.pathname, ...extra }, 'error')
}

/** 记录关键操作（reportLog 增强）。 */
export function trackAction(action: string, extra?: Record<string, unknown>) {
  trackLog({ t: 'act', a: action.slice(0, 300), ...extra })
}
