/**
 * 推理服务（Python /infer）状态访问。
 * - 健康状态：经后端 /api/v1/infer/health 代理获取（浏览器/桌面壳通用）
 * - 启动/停止：仅桌面壳（Tauri invoke），浏览器环境无权限
 */

import { get } from '@/api/client'

export interface InferModelState {
  state: 'ok' | 'failed' | 'not_loaded'
  error: string | null
}

export interface InferHealth {
  status: string
  models: {
    tagger: InferModelState
    aesthetic: InferModelState
  }
  paths: {
    tagger_model_dir: string
    aesthetic_model: string
  }
}

/** 推理服务健康状态（经后端代理转发）。服务未启动时抛错。 */
export const fetchInferHealth = () => get<InferHealth>('/infer/health')

/** 汇总状态：running / degraded（模型加载失败）/ stopped（服务未启动）。 */
export type InferOverall = 'running' | 'degraded' | 'stopped'

export function summarizeHealth(h: InferHealth | null): InferOverall {
  if (!h) return 'stopped'
  const models = [h.models?.tagger?.state, h.models?.aesthetic?.state]
  if (models.some((s) => s === 'failed')) return 'degraded'
  return 'running'
}

// ---- 桌面壳控制（Tauri invoke，浏览器环境不可用） ----
// 与 ImageDetailView 相同的模式：用 __TAURI_INTERNALS__ 调 invoke（无需前端依赖）

interface TauriInternals {
  invoke?: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>
}

function tauriInvoke(): TauriInternals['invoke'] {
  const internals = (window as unknown as { __TAURI_INTERNALS__?: TauriInternals }).__TAURI_INTERNALS__
  return internals?.invoke
}

/** 是否运行在桌面壳（Tauri）内。 */
export const isTauri = (): boolean => !!tauriInvoke()

/** 请求桌面壳拉起推理服务。返回启动结果信息。 */
export async function inferStart(): Promise<string> {
  const invoke = tauriInvoke()
  if (!invoke) throw new Error('仅桌面版支持启动推理服务')
  return (await invoke('infer_start')) as string
}

/** 请求桌面壳停止推理服务。 */
export async function inferStop(): Promise<void> {
  const invoke = tauriInvoke()
  if (!invoke) throw new Error('仅桌面版支持停止推理服务')
  await invoke('infer_stop')
}