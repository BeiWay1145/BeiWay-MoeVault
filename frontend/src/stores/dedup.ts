import { defineStore } from 'pinia'
import { ref } from 'vue'
import { get, post } from '@/api/client'

export interface DedupGroup {
  id: number
  size: number
  redundant_count: number
  best_id: number | null
  best_thumb_rel: string | null
  best_clarity: number | null
}

export interface GroupMember {
  image_id: number
  rel_path: string
  thumb_rel: string
  width: number
  height: number
  clarity_score: number
  aesthetic_score: number | null
  is_redundant: boolean
  is_best: boolean
}

export interface GroupDetail {
  id: number
  state: string
  members: GroupMember[]
}

/** 缩略图 URL：thumb_rel 为相对 data/thumbs 的路径（Windows 反斜杠转正斜杠）。 */
export function thumbUrl(thumbRel: string | null): string | undefined {
  if (!thumbRel) return undefined
  return `/thumbs/${thumbRel.replace(/\\/g, '/')}`
}

/** 查重状态，由 /api/v1/dedup/* 驱动 */
export const useDedupStore = defineStore('dedup', () => {
  const groupCount = ref(0)
  const redundantCount = ref(0)
  const involvedImages = ref(0)
  const groups = ref<DedupGroup[]>([])
  const loading = ref(false)

  async function refreshStats() {
    const s = await get<{
      group_count: number
      involved_images: number
      redundant_count: number
    }>('/dedup/stats')
    groupCount.value = s.group_count
    involvedImages.value = s.involved_images
    redundantCount.value = s.redundant_count
  }

  async function fetchGroups() {
    loading.value = true
    try {
      const d = await get<{ items: DedupGroup[]; total: number }>('/dedup/groups?limit=200')
      groups.value = d.items
      await refreshStats()
    } finally {
      loading.value = false
    }
  }

  async function groupDetail(id: number): Promise<GroupDetail> {
    return get<GroupDetail>(`/dedup/groups/${id}`)
  }

  async function scan(full = false) {
    await post('/dedup/scan', { full })
  }

  async function resolve(id: number, mode: 'best_only' | 'specific', recycleIds?: number[]) {
    const body = mode === 'best_only' ? { mode } : { mode, recycle_ids: recycleIds ?? [] }
    return post<{ recycled: number }>(`/dedup/groups/${id}/resolve`, body)
  }

  return {
    groupCount,
    redundantCount,
    involvedImages,
    groups,
    loading,
    refreshStats,
    fetchGroups,
    groupDetail,
    scan,
    resolve,
  }
})
