import type { Component } from 'vue'
import { MagicStick, Star, Search, PictureFilled, Download, Delete, CopyDocument, Document } from '@element-plus/icons-vue'
import { invoke } from '@tauri-apps/api/core'
import { ElMessage, ElMessageBox } from 'element-plus'
import { post } from '@/api/client'
import { useTaskStore } from '@/stores/tasks'

/**
 * 批量操作注册表（声明式）。
 *
 * 设计目标：新增一个批量功能时，**只需在这里加一个对象**，
 * 工具栏下拉、右键菜单、参数勾选框会自动出现——
 * 替代此前"改 4 处（分支 + 模板 + 参数 ref + 调用处）"的做法。
 *
 * 每个动作声明：
 * - key/label/icon：展示信息
 * - run：执行逻辑（接收 ids 与参数）
 * - options：可选参数（由 UI 自动渲染为勾选框/子菜单）
 * - surfaces：出现在哪些入口（'toolbar' | 'context'），默认两者都出现
 * - danger：是否危险操作（UI 用红色呈现）
 * - confirm：是否需要二次确认（右键菜单场景统一为 false，符合"无需二次确认"要求）
 */

/** 批量参数：所有动作共享一个可响应对象，UI 自动渲染。 */
export interface BatchOptions {
  /** 溯源：强制重试不可溯源图 */
  forceSauce: boolean
  /** 溯源：命中后自动替换更清晰的原图 */
  autoReplaceSauce: boolean
  /** 美学：强制重评（覆盖已有分数） */
  forceAesthetic: boolean
}

export interface BatchActionOption {
  /** 参数键（对应 BatchOptions 的字段） */
  key: keyof BatchOptions
  label: string
  /** 仅在该动作下显示 */
  tip?: string
}

export interface BatchActionContext {
  ids: number[]
  options: BatchOptions
  /** 请求刷新列表（动作执行后） */
  refresh?: () => void
  /** 打开导出弹窗（图库特有，由调用方注入，避免注册表依赖具体视图） */
  openExport?: (ids: number[]) => void
  /** 清理选择 */
  clearSelection?: () => void
  /**
   * 把图片 id 解析成磁盘路径（由视图注入，因为只有视图知道当前数据源）。
   * 拖出与复制类动作依赖它；未注入时相应动作会提示不可用。
   */
  resolvePaths?: (ids: number[]) => string[]
}

export interface BatchAction {
  key: string
  label: string
  icon?: Component
  /** 执行。ids 为空时不应被调用（由 UI 禁用）。 */
  run: (ctx: BatchActionContext) => Promise<void> | void
  /** 可选参数（UI 自动渲染）。 */
  options?: BatchActionOption[]
  /** 出现位置：默认两者都出现。 */
  surfaces?: Array<'toolbar' | 'context'>
  /** 危险操作（红色）。 */
  danger?: boolean
  /** 二次确认。默认：危险操作 true，其余 false（右键菜单无需确认）。 */
  confirm?: boolean
}

/** 危险操作的统一确认。 */
async function confirmDanger(message: string, title: string) {
  await ElMessageBox.confirm(message, title, { type: 'warning', confirmButtonText: '确定' })
}

/**
 * 内置批量动作。
 *
 * 注意：这些动作只负责"提交任务/调用接口"，不持有视图状态；
 * 选择集与参数由调用方通过 BatchActionContext 传入，
 * 因此图库、主目录、右键菜单可以共用同一份定义。
 */
export const BATCH_ACTIONS: BatchAction[] = [
  {
    key: 'tag',
    label: '打标',
    icon: MagicStick,
    run: async ({ ids }) => {
      await useTaskStore().enqueueTag(ids)
    },
  },
  {
    key: 'aesthetic',
    label: '美学评分',
    icon: Star,
    options: [{ key: 'forceAesthetic', label: '强制重评（覆盖已有分数）' }],
    run: async ({ ids, options }) => {
      await useTaskStore().enqueueAesthetic(ids, options.forceAesthetic)
    },
  },
  {
    key: 'sauce',
    label: '溯源',
    icon: Search,
    options: [
      { key: 'forceSauce', label: '强制重试不可溯源' },
      { key: 'autoReplaceSauce', label: '原图替换' },
    ],
    run: async ({ ids, options }) => {
      await useTaskStore().enqueueSauce(ids, options.forceSauce, options.autoReplaceSauce)
    },
  },
  {
    key: 'ai-detect',
    label: 'AI 检测',
    icon: PictureFilled,
    // 逐张读取 AI 元信息（后端 /images/{id}/ai-info）。
    // 此前该逻辑在 LibraryView 与 ImportDirectoryView 各写了一份，现已收敛到此处。
    run: async ({ ids, refresh, clearSelection }) => {
      ElMessage.info(`正在检测 ${ids.length} 张图片的 AI 元信息…`)
      let ok = 0
      for (const id of ids) {
        try {
          await post(`/images/${id}/ai-info`)
          ok++
        } catch {
          /* 单张失败继续 */
        }
      }
      ElMessage.success(`AI 检测完成：${ok} 张已处理`)
      clearSelection?.()
      refresh?.()
    },
  },
  {
    key: 'copy-image',
    label: '复制图片',
    icon: CopyDocument,
    // 复制文件本身到剪贴板（CF_HDROP）：可在资源管理器中直接粘贴
    surfaces: ['context'],
    run: async ({ ids, resolvePaths }) => {
      const paths = resolvePaths?.(ids) ?? []
      if (paths.length === 0) {
        ElMessage.warning('无法解析文件路径')
        return
      }
      const r = await invoke<{ count: number }>('file_copy_to_clipboard', { paths })
      ElMessage.success(`已复制 ${r.count} 个文件，可在资源管理器粘贴`)
    },
  },
  {
    key: 'copy-path',
    label: '复制路径',
    icon: Document,
    surfaces: ['context'],
    run: async ({ ids, resolvePaths }) => {
      const paths = resolvePaths?.(ids) ?? []
      if (paths.length === 0) {
        ElMessage.warning('无法解析文件路径')
        return
      }
      await invoke('file_copy_paths', { paths })
      ElMessage.success(`已复制 ${paths.length} 条路径`)
    },
  },
  {
    key: 'export',
    label: '导出…',
    icon: Download,
    surfaces: ['toolbar', 'context'],
    run: async ({ ids, openExport }) => {
      if (openExport) openExport(ids)
      else ElMessage.info('当前页面不支持导出')
    },
  },
  {
    key: 'recycle',
    label: '移入回收站',
    icon: Delete,
    danger: true,
    run: async ({ ids, refresh, clearSelection }) => {
      for (const id of ids) {
        await post(`/images/${id}/recycle`, { reason: 'manual' })
      }
      ElMessage.success(`已移入回收站 ${ids.length} 张`)
      clearSelection?.()
      refresh?.()
    },
  },
]

/** 按入口筛选动作。 */
export function actionsFor(surface: 'toolbar' | 'context'): BatchAction[] {
  return BATCH_ACTIONS.filter((a) => !a.surfaces || a.surfaces.includes(surface))
}

/** 取某个动作的参数声明（空数组表示无参数）。 */
export function optionsOf(action: BatchAction): BatchActionOption[] {
  return action.options ?? []
}

/** 默认参数值。 */
export function defaultBatchOptions(): BatchOptions {
  return { forceSauce: false, autoReplaceSauce: false, forceAesthetic: false }
}
