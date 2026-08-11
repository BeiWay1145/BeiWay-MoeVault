import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import { get } from '@/api/client'

/** 图库浏览状态：筛选条件、排序、视图模式、选中集合。数据来自 /api/v1/images。 */

export interface ImageItem {
  id: number
  name: string
  width: number
  height: number
  sizeBytes: number
  clarity: number
  aesthetic: number | null
  isRedundant: boolean
  /** 导入时间（epoch 秒） */
  importedAt: number
  /** 缩略图相对路径（Windows 反斜杠→正斜杠，供 /thumbs 访问） */
  thumbRel: string
  /** 是否 AI 生成图片 */
  isAi: boolean
  /** 文件扩展名（如 jpg/png） */
  format?: string
}

export type ViewMode = 'grid' | 'waterfall' | 'list'
export type SortKey = 'imported' | 'aesthetic' | 'clarity' | 'size' | 'date' | 'random'

/** 缩略图 URL。 */
export function thumbUrl(thumbRel: string | null | undefined): string | undefined {
  if (!thumbRel) return undefined
  return `/thumbs/${thumbRel.replace(/\\/g, '/')}`
}

/** 原图 URL。 */
export function originalUrl(id: number): string {
  return `/api/v1/images/${id}/file`
}

export interface LibraryFilter {
  q?: string
  tags?: string[]
  excludeTags?: string[]
  aestheticMin?: number
  clarityMin?: number
  source?: string
  format?: string
  isRedundant?: boolean
  isAi?: boolean
}

export const useLibraryStore = defineStore('library', () => {
  // 视图模式持久化（关闭软件后记忆）
  const savedMode = localStorage.getItem('moevault-view-mode')
  const viewMode = ref<ViewMode>(savedMode === 'grid' || savedMode === 'waterfall' || savedMode === 'list' ? savedMode : 'grid')
  const sortKey = ref<SortKey>('imported')
  const sortAsc = ref(false)
  const selected = ref<Set<number>>(new Set())
  const filter = ref<LibraryFilter>({})
  /** 详情位置记忆：{from, imageId, scrollTop}（localStorage 持久化，退出还原） */
  const detailPos = ref<{ from: string; imageId: number; scrollTop: number } | null>(null)
  try {
    const raw = localStorage.getItem('moevault-detail-pos')
    if (raw) detailPos.value = JSON.parse(raw) as { from: string; imageId: number; scrollTop: number }
  } catch {
    /* 忽略 */
  }
  /** 多选模式（画廊/搜索共用）：开启后点击图片直接切换选择，用于批量操作 */
  const multiSelect = ref(false)

  // 视图模式变化 → 持久化
  watch(viewMode, (m) => localStorage.setItem('moevault-view-mode', m))

  const images = ref<ImageItem[]>([])
  const total = ref(0)
  const loading = ref(false)

  /** 拉取图片列表（按当前筛选/排序）。 */
  async function fetchImages(limit = 200) {
    loading.value = true
    try {
      const params = new URLSearchParams()
      params.set('limit', String(limit))
      if (sortKey.value) params.set('sort', sortKey.value)
      if (sortAsc.value) params.set('order', 'asc')
      const f = filter.value
      if (f.q) params.set('q', f.q)
      if (f.tags?.length) params.set('tags', f.tags.join(','))
      if (f.excludeTags?.length) params.set('exclude_tags', f.excludeTags.join(','))
      if (f.aestheticMin != null) params.set('aesthetic_min', String(f.aestheticMin))
      if (f.clarityMin != null) params.set('clarity_min', String(f.clarityMin))
      if (f.source) params.set('source', f.source)
      if (f.format) params.set('format', f.format)
      if (f.isRedundant != null) params.set('is_redundant', f.isRedundant ? '1' : '0')
      if (f.isAi != null) params.set('is_ai', f.isAi ? '1' : '0')

      const d = await get<{ items: Array<Record<string, unknown>>; total: number }>(
        `/images?${params.toString()}`,
      )
      images.value = d.items.map((it) => ({
        id: it.id as number,
        name: decodeURIComponent((it.rel_path as string).split('/').pop() ?? ''),
        width: it.width as number,
        height: it.height as number,
        sizeBytes: it.size_bytes as number,
        clarity: it.clarity_score as number,
        aesthetic: it.aesthetic_score as number | null,
        isRedundant: it.is_redundant as boolean,
        importedAt: it.imported_at as number,
        thumbRel: (it.thumb_rel as string) ?? '',
        isAi: it.is_ai as boolean,
        format: (it.format as string) ?? undefined,
      }))
      total.value = d.total
    } finally {
      loading.value = false
    }
  }

  /** 设置筛选并刷新。 */
  async function applyFilter(patch: Partial<LibraryFilter>) {
    filter.value = { ...filter.value, ...patch }
    await fetchImages()
  }

  function clearFilter() {
    filter.value = {}
  }

  /** 从当前列表移除一张图（详情页删除后调用）。 */
  function removeImageById(id: number) {
    const idx = images.value.findIndex((i) => i.id === id)
    if (idx >= 0) {
      images.value.splice(idx, 1)
      total.value = Math.max(0, total.value - 1)
    }
  }

  function toggleSelect(id: number) {
    const s = new Set(selected.value)
    if (s.has(id)) s.delete(id)
    else s.add(id)
    selected.value = s
  }

  function clearSelect() {
    selected.value = new Set()
  }

  /** 记录进入详情页时的位置（来源页 + 图片 id + 滚动位置），供返回/重启还原。 */
  function saveDetailPos(from: string, imageId: number) {
    const scroller = document.querySelector('.app-main')
    const scrollTop = scroller ? scroller.scrollTop : window.scrollY
    detailPos.value = { from, imageId, scrollTop }
    try {
      localStorage.setItem('moevault-detail-pos', JSON.stringify(detailPos.value))
    } catch {
      /* 忽略 */
    }
  }

  /** 取出保存的位置（from 匹配才返回）。 */
  function restoreDetailPos(from: string) {
    if (detailPos.value && detailPos.value.from === from) {
      return detailPos.value
    }
    return null
  }

  return {
    viewMode,
    sortKey,
    sortAsc,
    selected,
    filter,
    detailPos,
    multiSelect,
    images,
    total,
    loading,
    fetchImages,
    applyFilter,
    clearFilter,
    removeImageById,
    toggleSelect,
    clearSelect,
    saveDetailPos,
    restoreDetailPos,
  }
})
