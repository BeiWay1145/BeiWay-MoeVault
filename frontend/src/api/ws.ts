/**
 * 后端 WebSocket 事件客户端（增强1：导入完成后自动刷新当前界面）。
 *
 * 后端通过 WS 广播 `{"type":"batch.done","payload":{batch_id,done,failed,duplicate}}`
 * 等事件（GET /ws，axum WS Hub）。前端此前从未消费——导入批次完成后界面不会自动刷新。
 *
 * 本模块负责：
 * - 建连 + 断线 5s 自动重连
 * - batch.done / batch.failed → 派发 window 事件：
 *   - 'moevault:import-done'   payload = { batch_id, done, failed, duplicate }
 *   - 'moevault:import-failed' payload = { batch_id, error }
 * 各视图监听对应事件刷新自身数据；全局通知在 AppLayout 统一弹出。
 */
import { ref } from 'vue'

let ws: WebSocket | null = null
let retryTimer: number | undefined
let stopped = false
/** 连接状态（供诊断展示）。 */
const connected = ref(false)

function wsUrl(): string {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  // 壳内同源（9178）；开发期经 vite 代理 /ws → 9178
  return `${proto}://${location.host}/ws`
}

function scheduleRetry() {
  if (stopped || retryTimer !== undefined) return
  retryTimer = window.setTimeout(() => {
    retryTimer = undefined
    connect()
  }, 5000)
}

function handleMessage(raw: MessageEvent) {
  let msg: { type?: string; payload?: Record<string, unknown> }
  try {
    msg = JSON.parse(typeof raw.data === 'string' ? raw.data : '')
  } catch {
    return // 非 JSON（如 hello 之外的杂项）忽略
  }
  if (msg.type === 'batch.done' || msg.type === 'batch.failed') {
    const p = (msg.payload ?? {}) as {
      batch_id?: number
      done?: number
      failed?: number
      duplicate?: number
      error?: string
    }
    const name = msg.type === 'batch.done' ? 'moevault:import-done' : 'moevault:import-failed'
    window.dispatchEvent(new CustomEvent(name, { detail: p }))
  }
}

function connect() {
  if (stopped) return
  if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) return
  try {
    ws = new WebSocket(wsUrl())
  } catch {
    ws = null
    scheduleRetry()
    return
  }
  ws.onopen = () => {
    connected.value = true
  }
  ws.onmessage = handleMessage
  ws.onclose = () => {
    connected.value = false
    ws = null
    scheduleRetry()
  }
  ws.onerror = () => {
    // onclose 会随后触发并安排重连
  }
}

/** 启动事件连接（AppLayout 挂载时调用，幂等）。 */
export function startWsEvents() {
  stopped = false
  connect()
}

/** 停止并清理（AppLayout 卸载时调用）。 */
export function stopWsEvents() {
  stopped = true
  if (retryTimer !== undefined) {
    window.clearTimeout(retryTimer)
    retryTimer = undefined
  }
  ws?.close()
  ws = null
  connected.value = false
}
