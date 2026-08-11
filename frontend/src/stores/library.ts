import { defineStore } from 'pinia'
import { ref } from 'vue'
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
  const viewMode = ref<ViewMode>('grid')
  const sortKey = ref<SortKey>('imported')
  const sortAsc = ref(false)
  const selected = ref<Set<number>>(new Set())
  const filter = ref<LibraryFilter>({})

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

  return {
    viewMode,
    sortKey,
    sortAsc,
    selected,
    filter,
    images,
    total,
    loading,
    fetchImages,
    applyFilter,
    clearFilter,
    removeImageById,
    toggleSelect,
    clearSelect,
  }
})
