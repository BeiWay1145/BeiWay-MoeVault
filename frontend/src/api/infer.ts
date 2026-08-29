/**
 * 推理服务（Python /infer）状态访问。
 * - 健康状态：经后端 /api/v1/infer/health 代理获取（浏览器/桌面壳通用）
 * - 启动/停止：仅桌面壳（Tauri invoke），浏览器环境无权限
 */

import { get } from '@/api/client'

export interface InferModelState {
  state: 'ok' | 'failed' | 'not_loaded'
  error: string | null
  /** 打标模型种类：cl_tagger / wd14（美学模型无此字段） */
  kind?: string
}

export interface InferHealth {
  status: string
  models: {
    tagger: InferModelState
    aesthetic: InferModelState
  }
  paths: {
    tagger_model_dir: string
    tagger_model_kind: string
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

/** 请求桌面壳为推理服务安装缺失依赖（fastapi/uvicorn/transformers 等 CPU 包）。 */
export async function inferInstallDeps(): Promise<string> {
  const invoke = tauriInvoke()
  if (!invoke) throw new Error('仅桌面版支持安装推理服务依赖')
  return (await invoke('infer_install_deps')) as string
}

/** 桌面壳推理服务命令状态（依赖缺失等启动前诊断信息）。 */
export interface InferShellStatus {
  running: boolean
  owned: boolean
  deps_missing: string[]
}

/** 查询桌面壳对推理服务的诊断状态（服务未运行时含依赖缺失列表）。 */
export async function inferShellStatus(): Promise<InferShellStatus | null> {
  const invoke = tauriInvoke()
  if (!invoke) return null // 浏览器环境无壳诊断
  return (await invoke('infer_status')) as InferShellStatus
}