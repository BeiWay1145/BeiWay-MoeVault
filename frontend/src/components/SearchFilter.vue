<script setup lang="ts">
/**
 * 搜索式筛选输入框（danbooru 风格，自绘实现，替代 el-select remote 联想）。
 * - 输入前缀 → 联想：标签（名称/中文名，词频倒序）+ 状态（带图片数量）
 * - 选中 → 输入框立即清空，chips 出现（标签按分类着色：画师红/系列橙/角色绿/常规蓝；状态灰）
 * - Enter 选中第一条联想；Backspace（空输入）删除最后一个 chip
 * - modelValue：`t:<标签名>` | `s:<状态key>` 数组；变更时 emit change（由父组件重建筛选）
 */
import { computed, ref } from 'vue'
import { get } from '@/api/client'
import { useSettingsStore } from '@/stores/settings'
import { displayTagLabel, displayTagName, searchTagKey } from '@/utils/tagNormalize'

interface SuggestTag {
  id: number
  name: string
  name_cn: string | null
  category: string
  image_count: number
}
interface SuggestStatus {
  key: string
  label: string
  count: number
}
interface ChipMeta {
  label: string
  kind: 'tag' | 'status'
  category?: string
}

const props = defineProps<{ modelValue: string[] }>()
const emit = defineEmits<{
  (e: 'update:modelValue', v: string[]): void
  (e: 'change'): void
}>()

const keyword = ref('')
const open = ref(false)
const loading = ref(false)
const suggestTags = ref<SuggestTag[]>([])
const suggestStatuses = ref<SuggestStatus[]>([])
/** 已选 key → 显示元信息（标签分类用于着色）。 */
const chipMeta = ref<Record<string, ChipMeta>>({})

const inputRef = ref<HTMLInputElement | null>(null)

const chips = computed(() =>
  props.modelValue.map((k) => ({
    key: k,
    ...(chipMeta.value[k] ?? {
      label: k.startsWith('t:') ? k.slice(2) : k,
      kind: 'tag' as const,
    }),
  })),
)

/** 联想请求（防抖 200ms）。 */
let timer: number | undefined
async function runSuggest(q: string) {
  const term = q.trim()
  if (!term) {
    suggestTags.value = []
    suggestStatuses.value = []
    open.value = false
    return
  }
  loading.value = true
  try {
    const d = await get<{ tags: SuggestTag[]; statuses: SuggestStatus[] }>(
      `/search/suggest?q=${encodeURIComponent(term)}&limit=8`,
    )
    suggestTags.value = d.tags
    suggestStatuses.value = d.statuses
    open.value = d.tags.length > 0 || d.statuses.length > 0
  } catch {
    suggestTags.value = []
    suggestStatuses.value = []
  } finally {
    loading.value = false
  }
}

function onInput() {
  // 增强1：逗号/分号分隔 → 前面的文本立即提交为 chip，继续输入下一个
  const v = keyword.value
  const m = v.match(/^(.+?)[,，;；]\s*(.*)$/)
  if (m) {
    commitTerm(m[1])
    keyword.value = m[2]
    runSuggest(m[2])
    return
  }
  if (timer !== undefined) window.clearTimeout(timer)
  timer = window.setTimeout(() => {
    timer = undefined
    runSuggest(v)
  }, 200)
}

/** 状态关键字 → 状态 chip key（与后端 suggest 状态一致）。 */
const STATUS_KEYWORDS: Array<[RegExp, string, string]> = [
  [/^非\s*ai$/i, 'not_ai', '非 AI 生成'],
  [/^ai$|^ai生成$|^ai图$/i, 'is_ai', 'AI 生成'],
  [/^溯源$|^sauced$/i, 'sauced', '已溯源'],
  [/^未溯源$|^unsauced$/i, 'unsauced', '未溯源'],
  [/^不可溯源$|^un-sauced$/i, 'un-sauced', '不可溯源'],
  [/^冗余$|^redundant$/i, 'redundant', '冗余候选'],
  [/^已打标$|^tagged$/i, 'tagged', '已打标'],
  [/^未打标$|^untagged$/i, 'untagged', '未打标'],
]

/** 提交一个标签/状态项为 chip（逗号分隔或精确输入）。 */
function commitTerm(termRaw: string) {
  const term = termRaw.trim()
  if (!term) return
  // 状态关键字优先
  for (const [re, key, label] of STATUS_KEYWORDS) {
    if (re.test(term)) {
      const k = `s:${key}`
      if (!props.modelValue.includes(k)) {
        chipMeta.value[k] = { label, kind: 'status' }
        emit('update:modelValue', [...props.modelValue, k])
        emit('change')
      }
      return
    }
  }
  // 标签 chip（BUG1：空格输入归一化为下划线规范名；未知分类回退 general 蓝）
  const k = `t:${searchTagKey(term)}`
  if (!props.modelValue.includes(k)) {
    chipMeta.value[k] = { label: displayTagName(term), kind: 'tag' }
    emit('update:modelValue', [...props.modelValue, k])
    emit('change')
  }
}

/** 选中联想项：追加 chip、清空输入、关闭下拉。 */
function select(key: string) {
  if (props.modelValue.includes(key)) {
    keyword.value = ''
    open.value = false
    return
  }
  const tag = suggestTags.value.find((t) => `t:${t.name}` === key)
  const st = suggestStatuses.value.find((s) => `s:${s.key}` === key)
  if (tag) {
    chipMeta.value[key] = {
      label: displayTag(tag),
      kind: 'tag',
      category: tag.category,
    }
  } else if (st) {
    chipMeta.value[key] = { label: st.label, kind: 'status' }
  } else {
    return
  }
  emit('update:modelValue', [...props.modelValue, key])
  keyword.value = ''
  open.value = false
  inputRef.value?.focus()
  emit('change')
}

/** 移除 chip（×）。 */
function remove(key: string) {
  emit('update:modelValue', props.modelValue.filter((k) => k !== key))
  delete chipMeta.value[key]
  chipMeta.value = { ...chipMeta.value }
  emit('change')
}

function removeLast() {
  if (keyword.value !== '') return
  const arr = props.modelValue
  if (arr.length === 0) return
  remove(arr[arr.length - 1])
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter') {
    const tag = suggestTags.value[0]
    const st = suggestStatuses.value[0]
    if (tag || st) {
      e.preventDefault()
      const key = tag ? `t:${tag.name}` : `s:${(st as SuggestStatus).key}`
      select(key)
    }
  } else if (e.key === 'Backspace') {
    removeLast()
  } else if (e.key === 'Escape') {
    open.value = false
  }
}

function onBlur() {
  // BUG1：失焦后清空候选（避免重新聚焦时残留上次关键词的联想结果）
  window.setTimeout(() => {
    open.value = false
    suggestTags.value = []
    suggestStatuses.value = []
  }, 150)
}

function onFocus() {
  // 聚焦时：仅当输入框有内容才重新联想并展开
  const q = keyword.value.trim()
  if (q) {
    runSuggest(q)
  } else {
    open.value = false
  }
}

/** chips 着色：标签按分类，状态灰。 */
function chipType(meta: ChipMeta): 'danger' | 'warning' | 'success' | 'primary' | 'info' {
  if (meta.kind === 'status') return 'info'
  switch (meta.category) {
    case 'artist':
      return 'danger'
    case 'copyright':
      return 'warning'
    case 'character':
      return 'success'
    default:
      return 'primary'
  }
}

function displayTag(t: SuggestTag) {
  return displayTagLabel(t.name, t.name_cn, showCnFirst.value)
}

function focusInput() {
  inputRef.value?.focus()
}

/** 增强2：优先显示中文标签（设置 tag_show_cn_first，默认英文在前）。 */
const settingsStore = useSettingsStore()
const showCnFirst = computed(() => settingsStore.settings.tag_show_cn_first === true)
</script>

<template>
  <div class="search-filter" @click="focusInput">
    <div class="sf-box" :class="{ focused: open }">
      <span v-for="c in chips" :key="c.key" class="sf-chip-wrap">
        <el-tag
          :type="chipType(c)"
          size="small"
          closable
          class="sf-chip"
          @close.stop="remove(c.key)"
        >
          {{ c.label }}
        </el-tag>
      </span>
      <input
        ref="inputRef"
        v-model="keyword"
        class="sf-input"
        placeholder="搜索标签/状态（如 1girl、black_、ai、溯源）…"
        @input="onInput"
        @keydown="onKeydown"
        @focus="onFocus"
        @blur="onBlur"
      />
    </div>

    <!-- 联想下拉 -->
    <div v-if="open" class="sf-dropdown">
      <template v-if="suggestTags.length > 0">
        <div class="sf-group-title">标签</div>
        <div
          v-for="t in suggestTags"
          :key="`t:${t.name}`"
          class="sf-item"
          @mousedown.prevent="select(`t:${t.name}`)"
        >
          <span class="sf-item-name">{{ displayTag(t) }}</span>
          <span class="sf-item-count">{{ t.image_count }}</span>
        </div>
      </template>
      <template v-if="suggestStatuses.length > 0">
        <div class="sf-group-title">状态</div>
        <div
          v-for="s in suggestStatuses"
          :key="`s:${s.key}`"
          class="sf-item"
          @mousedown.prevent="select(`s:${s.key}`)"
        >
          <span class="sf-item-name">{{ s.label }}</span>
          <span class="sf-item-count">{{ s.count }}</span>
        </div>
      </template>
      <div v-if="loading" class="sf-loading">加载中…</div>
      <div
        v-if="!loading && suggestTags.length === 0 && suggestStatuses.length === 0 && keyword.trim()"
        class="sf-empty"
      >
        无匹配（试试 black_ / ai / 溯源 / danbooru）
      </div>
    </div>
  </div>
</template>

<style scoped>
.search-filter {
  position: relative;
  width: 360px;
  flex: 0 1 auto;
  cursor: text;
}
.sf-box {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px;
  min-height: 32px;
  padding: 2px 10px;
  border: 1px solid var(--el-border-color);
  border-radius: var(--el-border-radius-base);
  background: var(--el-input-bg-color, var(--el-fill-color-blank));
  transition: border-color 0.15s;
}
.sf-box:hover {
  border-color: var(--el-color-primary-light-5);
}
.sf-box.focused {
  border-color: var(--el-color-primary);
  box-shadow: 0 0 0 1px var(--el-color-primary) inset;
}
.sf-chip {
  max-width: 200px;
}
.sf-input {
  flex: 1;
  min-width: 90px;
  border: none;
  outline: none;
  background: transparent;
  font-size: 13px;
  height: 24px;
  padding: 0;
  color: var(--el-text-color-primary);
}
.sf-dropdown {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  right: 0;
  z-index: 3000;
  max-height: 320px;
  overflow: auto;
  background: var(--el-bg-color-overlay);
  border: 1px solid var(--el-border-color-light);
  border-radius: 8px;
  box-shadow: var(--el-box-shadow-light);
  padding: 4px;
}
.sf-group-title {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  padding: 4px 8px 2px;
}
.sf-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 5px 8px;
  border-radius: 6px;
  cursor: pointer;
  font-size: 13px;
}
.sf-item:hover {
  background: var(--el-fill-color-light);
}
.sf-item-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--el-text-color-primary);
}
.sf-item-count {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  flex-shrink: 0;
}
.sf-loading,
.sf-empty {
  padding: 8px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
  text-align: center;
}
</style>
