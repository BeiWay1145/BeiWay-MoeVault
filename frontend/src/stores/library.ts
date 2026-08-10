import { defineStore } from 'pinia'
import { ref } from 'vue'

/** 图库浏览状态：筛选条件、排序、视图模式、选中集合（骨架阶段含 mock 数据） */

export interface ImageItem {
  id: number
  name: string
  width: number
  height: number
  sizeBytes: number
  clarity: number
  aesthetic: number | null
  isRedundant: boolean
  /** 导入时间（epoch 秒），骨架用 mock */
  importedAt: number
  /** 骨架占位：缩略图用 hue 生成渐变背景 */
  hue: number
}

export type ViewMode = 'grid' | 'waterfall' | 'list'
export type SortKey = 'imported' | 'aesthetic' | 'clarity' | 'size' | 'date'

function makeMockImages(n: number): ImageItem[] {
  const names = ['夏风', '海边', '少女与猫', '雨夜', '落日', '森林', '城市夜景', '花园', '星空', '街角']
  const list: ImageItem[] = []
  for (let i = 0; i < n; i++) {
    const w = 480 + ((i * 137) % 1600)
    const h = 480 + ((i * 89) % 2000)
    list.push({
      id: i + 1,
      name: `${names[i % names.length]}_${String(i + 1).padStart(4, '0')}.png`,
      width: w,
      height: h,
      sizeBytes: 512_000 + ((i * 731) % 8_000_000),
      clarity: +(3 + ((i * 37) % 70) / 10).toFixed(1),
      aesthetic: i % 7 === 0 ? null : +(2.5 + ((i * 13) % 30) / 10).toFixed(1),
      isRedundant: i % 11 === 0,
      importedAt: 1_717_000_000 + i * 3600,
      hue: (i * 47) % 360,
    })
  }
  return list
}

export const useLibraryStore = defineStore('library', () => {
  const viewMode = ref<ViewMode>('grid')
  const sortKey = ref<SortKey>('imported')
  const sortAsc = ref(false)
  const selected = ref<Set<number>>(new Set())

  // 骨架 mock；接入后端后改为从 /api/v1/images 拉取
  const images = ref<ImageItem[]>(makeMockImages(60))
  const total = ref(60)
  const loading = ref(false)

  function toggleSelect(id: number) {
    const s = new Set(selected.value)
    if (s.has(id)) s.delete(id)
    else s.add(id)
    selected.value = s
  }

  function clearSelect() {
    selected.value = new Set()
  }

  return { viewMode, sortKey, sortAsc, selected, images, total, loading, toggleSelect, clearSelect }
})
