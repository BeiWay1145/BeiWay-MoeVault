/**
 * API 客户端（骨架阶段）。
 * 开发期经 vite 代理 /api → http://127.0.0.1:8000（Rust 主服务，尚未实现）。
 * 后端未就绪时业务调用会失败——当前页面使用 mock 数据，接入后端后逐页替换。
 */
const BASE = '/api/v1'

import { trackApi } from '@/api/tracking'

export class ApiError extends Error {
  code: string
  status: number
  constructor(status: number, code: string, message: string) {
    super(message)
    this.status = status
    this.code = code
  }
}

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const started = performance.now()
  const method = (init?.method ?? 'GET').toUpperCase()
  const full = `${BASE}${path}`
  let res: Response
  try {
    res = await fetch(full, {
      headers: { 'Content-Type': 'application/json' },
      ...init,
    })
  } catch (e) {
    trackApi(method, path, null, performance.now() - started, String(e))
    throw e
  }
  const ms = performance.now() - started
  if (!res.ok) {
    let code = 'unknown'
    let message = res.statusText
    let errBody: string | undefined
    try {
      const body = (await res.json()) as { error?: { code?: string; message?: string } }
      code = body.error?.code ?? code
      message = body.error?.message ?? message
      errBody = JSON.stringify(body)
    } catch {
      /* 非 JSON 响应 */
    }
    trackApi(method, path, res.status, ms, errBody)
    throw new ApiError(res.status, code, message)
  }
  trackApi(method, path, res.status, ms)
  return res.json() as Promise<T>
}

export const get = <T>(path: string) => api<T>(path)
export const post = <T>(path: string, body?: unknown) =>
  api<T>(path, { method: 'POST', body: body === undefined ? undefined : JSON.stringify(body) })
export const put = <T>(path: string, body?: unknown) =>
  api<T>(path, { method: 'PUT', body: body === undefined ? undefined : JSON.stringify(body) })
export const del = <T>(path: string) => api<T>(path, { method: 'DELETE' })
