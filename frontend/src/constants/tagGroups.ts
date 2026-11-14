/**
 * 详情页标签分组定义（从 ImageDetailView.vue 抽出）。
 *
 * 抽出的理由：同一份分组规则同时被「标签栏」和「修改标签弹窗」使用，
 * 弹窗已独立成 components/detail/TagEditPanel.vue；把定义放在这里，
 * 两个消费方共享一份，避免分类口径在两处各自漂移。
 */

/** 详情页单个标签（后端 /images/{id}/tags 返回项）。 */
export interface DetailTag {
  tag_id: number
  name: string
  name_cn: string | null
  category: string
  source: string
  image_count: number
}

export type TagGroupKey = 'artist' | 'copyright' | 'character' | 'general'

export type TagGroups = Record<TagGroupKey, DetailTag[]>

export interface TagGroupDef {
  key: TagGroupKey
  label: string
  type: 'danger' | 'warning' | 'success' | 'primary'
}

/** 分组顺序与配色（danbooru 分类口径）。 */
export const TAG_GROUP_DEFS: TagGroupDef[] = [
  { key: 'artist', label: '画师', type: 'danger' },
  { key: 'copyright', label: '系列', type: 'warning' },
  { key: 'character', label: '角色', type: 'success' },
  { key: 'general', label: '常规', type: 'primary' },
]

/** 按后端 category 字段分组；未知分类一律归入「常规」。 */
export function groupTags(tags: DetailTag[]): TagGroups {
  const groups: TagGroups = { artist: [], copyright: [], character: [], general: [] }
  for (const t of tags) {
    const cat = t.category
    if (cat === 'artist') groups.artist.push(t)
    else if (cat === 'copyright') groups.copyright.push(t)
    else if (cat === 'character') groups.character.push(t)
    else groups.general.push(t)
  }
  return groups
}
