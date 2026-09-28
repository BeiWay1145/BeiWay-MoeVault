import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import { get } from '@/api/client'
import { findScrollContainer } from '@/utils/scrollTarget'

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
  /** 库内相对路径（原图）：拖出/复制类操作据此定位真实文件（壳层会拼接库目录）。 */
  relPath?: string
  /** 是否 AI 生成图片 */
  isAi: boolean
  /** 文件扩展名（如 jpg/png） */
  format?: string
  /** 溯源来源链接（danbooru/gelbooru 页面或自定义） */
  sourceUrl?: string
  /** 来源（danbooru/gelbooru/local） */
  source?: string
  /** 不可自动溯源标记（尝试过无结果/AI 图） */
  noAutoSauce?: boolean
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
  aestheticMax?: number
  /** 美学筛选时包含未评分图（默认 false：未评分图被排除） */
  aestheticIncludeUnscored?: boolean
  clarityMin?: number
  source?: string
  format?: string
  isRedundant?: boolean
  isAi?: boolean
  /** 溯源状态：sauced / unsauced / un-sauced */
  sauceStatus?: string
  /** 打标状态：true=已打标 / false=未打标 */
  tagged?: boolean
}

export const useLibraryStore = defineStore('library', () => {
  // 视图模式持久化（关闭软件后记忆）
  const savedMode = localStorage.getItem('moevault-view-mode')
  const viewMode = ref<ViewMode>(savedMode === 'grid' || savedMode === 'waterfall' || savedMode === 'list' ? savedMode : 'grid')
  const sortKey = ref<SortKey>('imported')
  const sortAsc = ref(false)
  const selected = ref<Set<number>>(new Set())
  const filter = ref<LibraryFilter>({})
  /**
   * 详情位置记忆：{from, imageId, scrollTop, thumbRect}。
   *
   * thumbRect 是进入详情时该图缩略图在视口中的矩形——
   * 视觉改进1 的"缩回缩略图"动画需要它作为终点。
   * 之所以在**进入详情时**记录而不是返回时查询：
   * 图库被 keep-alive 缓存时通常以 display:none 隐藏，
   * 返回时 getBoundingClientRect() 会返回全 0，无法作为动画目标。
   * 注意：thumbRect 是"当时的视口坐标"，返回时需结合滚动位置换算（见视图侧）。
   */
  const detailPos = ref<{
    from: string
    imageId: number
    scrollTop: number
    thumbRect?: { x: number; y: number; w: number; h: number }
} | null>(null)

  /**
   * 图库当前布局快照（供详情页在翻页后推算目标缩略图坐标）。
   *
   * 为什么需要：详情页翻到别的图后返回时，目标缩略图与最初那张不在同一位置，
   * 必须重算坐标；而图库此时处于 keep-alive 隐藏态，直接读 DOM 坐标不可靠。
   * 因此由图库在其可见时记录布局参数，详情页据「目标索引 + 参数」纯计算得出坐标。
   */
  /**
   * 图库布局快照：**直接记录每张缩略图的真实视口矩形**，而非只记参数。
   *
   * 为什么记坐标而不是记参数：瀑布流是"各列独立堆叠"，每列高度不同，
   * 用"行列估算"复现必然产生偏差（实测偏约 1/4 卡片高度）。
   * 图库可见时把所有缩略图的真实矩形一次性采下来，详情页翻页时直接查表，
   * 与渲染结果**逐像素一致**，不存在算法复现误差。
   *
   * rects 为 Map：imageId -> 记录时的视口矩形（相对当时滚动位置）。
   */
  const listLayout = ref<{
    rects: Record<number, { x: number; y: number; w: number; h: number }>
    /** 记录时的滚动位置（用于把"记录时坐标"换算为"返回后坐标"）。 */
    scrollTop: number
} | null>(null)
  try {
    const raw = localStorage.getItem('moevault-detail-pos')
    if (raw) detailPos.value = JSON.parse(raw) as { from: string; imageId: number; scrollTop: number }
  } catch {
    /* 忽略 */
  }
  // 架构重构：移除 multiSelect（"多选模式"）概念。
  // 选择改为资源管理器式无模式交互，由 composables/useSelection.ts 统一管理；
  // selected 仍保留供视图读取（由 useSelection 同步写入）。

  /**
   * 详情页浏览上下文（增强2）：来源视图的有序图片 id 列表。
   * 打开详情前由来源视图设置（主目录来源组 / 图库当前筛选 / 搜索结果），
   * 详情页的上一张/下一张/预加载/删除后在上下文内导航——
   * 在二级目录里点 B 切上下张就是同组的 A/C，而不是全局库的其他图。
   * 之后的筛选/分类视图同样只需 setViewerContext 即可获得一致行为。
   * 内存态（刷新/直达 URL 丢失 → 详情页回退全局库顺序）。
   */
  const viewerContext = ref<{ ids: number[]; label: string } | null>(null)
  function setViewerContext(ids: number[], label: string) {
    viewerContext.value = { ids: [...ids], label }
  }
  function clearViewerContext() {
    viewerContext.value = null
  }
  /** 删除图片后同步从上下文移除，保持下/上一张导航正确。 */
  function removeFromViewerContext(id: number) {
    if (viewerContext.value) {
      viewerContext.value = {
        ...viewerContext.value,
        ids: viewerContext.value.ids.filter((x) => x !== id),
      }
    }
  }

  // 视图模式变化 → 持久化
  watch(viewMode, (m) => localStorage.setItem('moevault-view-mode', m))

  const images = ref<ImageItem[]>([])
  const total = ref(0)
  const loading = ref(false)
  /** 游标分页的下一页游标（cursor 模式时由后端返回）。 */
  const nextCursor = ref<string | null>(null)

  /** 拉取图片列表（按当前筛选/排序）。
   *  opts.cursor 存在时用游标分页（E6），否则按 limit 一次拉取。 */
  async function fetchImages(limit = 200, opts: { cursor?: string | null } = {}) {
    loading.value = true
    try {
      const params = new URLSearchParams()
      params.set('limit', String(limit))
      if (opts.cursor != null) params.set('cursor', String(opts.cursor))
      if (sortKey.value) params.set('sort', sortKey.value)
      if (sortAsc.value) params.set('order', 'asc')
      const f = filter.value
      if (f.q) params.set('q', f.q)
      if (f.tags?.length) params.set('tags', f.tags.join(','))
      if (f.excludeTags?.length) params.set('exclude_tags', f.excludeTags.join(','))
      if (f.aestheticMin != null) params.set('aesthetic_min', String(f.aestheticMin))
      if (f.aestheticMax != null) params.set('aesthetic_max', String(f.aestheticMax))
      if (f.aestheticIncludeUnscored != null) params.set('aesthetic_include_unscored', f.aestheticIncludeUnscored ? '1' : '0')
      if (f.clarityMin != null) params.set('clarity_min', String(f.clarityMin))
      if (f.source) params.set('source', f.source)
      if (f.format) params.set('format', f.format)
      if (f.isRedundant != null) params.set('is_redundant', f.isRedundant ? '1' : '0')
      if (f.isAi != null) params.set('is_ai', f.isAi ? '1' : '0')
      if (f.sauceStatus) params.set('sauce_status', f.sauceStatus)
      if (f.tagged != null) params.set('tagged', f.tagged ? 'tagged' : 'untagged')

      const d = await get<{ items: Array<Record<string, unknown>>; total: number; next_cursor?: string | null }>(
        `/images?${params.toString()}`,
      )
      images.value = d.items.map((it) => ({
        id: it.id as number,
        name: decodeURIComponent((it.rel_path as string).split(/[\\/]/).pop() ?? ''),
        width: it.width as number,
        height: it.height as number,
        sizeBytes: it.size_bytes as number,
        clarity: it.clarity_score as number,
        aesthetic: it.aesthetic_score as number | null,
        isRedundant: it.is_redundant as boolean,
        importedAt: it.imported_at as number,
        thumbRel: (it.thumb_rel as string) ?? '',
        relPath: (it.rel_path as string) ?? undefined,
        isAi: it.is_ai as boolean,
        format: (it.format as string) ?? undefined,
        sourceUrl: (it.source_url as string) ?? undefined,
        source: (it.source as string) ?? undefined,
        noAutoSauce: (it.no_auto_sauce as boolean) ?? false,
      }))
      total.value = d.total
      nextCursor.value = (d.next_cursor as string | null) ?? null
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


  /**
   * 记录进入详情页时的位置（来源页 + 图片 id + 滚动位置 + 缩略图矩形），
   * 供返回还原与"缩回缩略图"动画使用。
   *
   * thumbRect：可选，调用方传入目标缩略图当前在视口中的矩形。
   */
  function saveDetailPos(
    from: string,
    imageId: number,
    thumbRect?: { x: number; y: number; w: number; h: number },
  ) {
    // 用探测工具找出**真正在滚动**的容器。
    // 此前硬编码 .app-main —— 而实际滚动的是 .wall-container（图片墙）。
    // 结果 scrollTop 恒为 0，返回时"恢复"到 0 → 表现为回顶（长期未定位到的根因）。
    const scroller = findScrollContainer()
    const scrollTop = scroller ? scroller.scrollTop : window.scrollY
    detailPos.value = { from, imageId, scrollTop, thumbRect }
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
    viewerContext,
    setViewerContext,
    clearViewerContext,
    removeFromViewerContext,
    images,
    total,
    loading,
    nextCursor,
    fetchImages,
    applyFilter,
    clearFilter,
    removeImageById,
    toggleSelect,
    listLayout,
    clearSelect,
    saveDetailPos,
    restoreDetailPos,
  }
})
