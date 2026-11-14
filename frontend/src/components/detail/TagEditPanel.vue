<script setup lang="ts">
/**
 * 详情页标签编辑器（二级弹窗 + 三级原始文本弹窗）。
 *
 * 从 ImageDetailView.vue 抽出（原文件 1895 行，承担 8 类职责）。
 * 抽出边界：本组件只负责「标签编辑状态机 + 两个弹窗 UI + 提交接口调用」，
 * 不持有图片详情数据 —— 标签列表由父组件传入，保存后通过 saved 事件
 * 通知父组件重新拉取详情（保持单一数据源，避免两处各存一份标签）。
 */
import { computed, ref, watch } from 'vue'
import { ElMessage } from 'element-plus'
import { del, post } from '@/api/client'
import { displayTagName, searchTagKey } from '@/utils/tagNormalize'
import { groupTags, TAG_GROUP_DEFS, type DetailTag } from '@/constants/tagGroups'

const props = defineProps<{
  /** 是否显示「修改标签」弹窗（v-model）。 */
  modelValue: boolean
  /** 当前图片 id；为空时不发请求（图片已被删除）。 */
  imageId: number | null
  /** 当前图片的全部标签（父组件从详情接口取得）。 */
  tags: DetailTag[]
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', v: boolean): void
  /** 修改已落库，父组件应重新拉取详情。 */
  (e: 'saved'): void
}>()

const visible = computed({
  get: () => props.modelValue,
  set: (v: boolean) => emit('update:modelValue', v),
})

const tagGroups = computed(() => groupTags(props.tags))
const tagGroupDefs = TAG_GROUP_DEFS

interface TagEdit {
  original: string
  tagId: number
  newName?: string
  deleted?: boolean
  /** 文本是否有有效修改（一个字没动不算） */
  dirty: boolean
}
const tagEdits = ref<Record<string, TagEdit>>({})
/** 正在编辑（显示输入框）的标签名。 */
const editingName = ref<string | null>(null)
/** 新增标签（分类 → 临时新标签列表；key 用自增 id）。 */
interface NewTag {
  key: number
  category: string
  value: string
}
const newTags = ref<NewTag[]>([])
let newTagSeq = 0

function enterEditMode() {
  tagEdits.value = {}
  newTags.value = []
  editingName.value = null
  for (const t of props.tags) tagEdits.value[t.name] = { original: t.name, tagId: t.tag_id, dirty: false }
}
function exitEditMode() {
  tagEdits.value = {}
  newTags.value = []
  editingName.value = null
}

// 父组件把 visible 置 true 即进入编辑（等价于原来的 openEditDialog）
watch(
  () => props.modelValue,
  (v) => {
    if (v) enterEditMode()
  },
)

/** 点 ×：划掉（待删除），再点恢复。 */
function toggleTagDelete(name: string) {
  const e = tagEdits.value[name]
  if (e) e.deleted = !e.deleted
}
/** 点标签本体：进入编辑（显示输入框，自动聚焦）。 */
function startEditTag(name: string) {
  editingName.value = name
  const e = tagEdits.value[name]
  editBuffer.value = e?.newName ?? name
}
/** 编辑输入缓冲（v-model 双向绑定）。 */
const editBuffer = ref('')
/** 失焦/回车：提交缓冲到 tagEdits（有效修改才标记 dirty）。 */
function commitEditTag(name: string) {
  if (editingName.value !== name) return
  const e = tagEdits.value[name]
  if (e) {
    const trimmed = editBuffer.value.trim()
    const valid = trimmed !== '' && trimmed !== name
    e.dirty = valid
    e.newName = valid ? trimmed : undefined
  }
  editingName.value = null
}
/** 输入变更：有效修改（≠原名且非空）标记 dirty。 */
function applyTagEdit(name: string, newName: string) {
  const e = tagEdits.value[name]
  if (!e) return
  const trimmed = newName.trim()
  const valid = trimmed !== '' && trimmed !== name
  e.dirty = valid
  e.newName = valid ? trimmed : undefined
}
/** 失焦/回车：退出当前编辑。 */
function stopEditTag(name: string) {
  commitEditTag(name)
}
/** 新增标签：点蓝色 + 号，加入该分类的空标签输入框。 */
function addNewTag(category: string) {
  const key = ++newTagSeq
  newTags.value.push({ key, category, value: '' })
  editingNewKey.value = key
  newTagBuffer.value = ''
}
/** 正在输入的新增标签 key。 */
const editingNewKey = ref<number | null>(null)
/** 新增标签输入缓冲。 */
const newTagBuffer = ref('')
function startNewTagEdit(key: number) {
  editingNewKey.value = key
  const t = newTags.value.find((x) => x.key === key)
  newTagBuffer.value = t?.value ?? ''
}
/** 失焦/回车：提交缓冲到新标签，保留非空内容。 */
function commitNewTagEdit(key: number) {
  const t = newTags.value.find((x) => x.key === key)
  if (t) t.value = newTagBuffer.value.trim()
  editingNewKey.value = null
}
function removeNewTag(key: number) {
  newTags.value = newTags.value.filter((x) => x.key !== key)
}
function newTagsOf(category: string): NewTag[] {
  return newTags.value.filter((x) => x.category === category)
}

/** 生效修改：一次性提交删除 + 重命名 + 新增（仅本图；空白新增自动丢弃）。 */
async function applyTagChanges() {
  if (props.imageId == null) return
  const id = props.imageId
  let ok = 0
  try {
    for (const e of Object.values(tagEdits.value)) {
      if (e.deleted) {
        await del(`/images/${id}/tags/${e.tagId}`)
        ok++
      } else if (e.dirty && e.newName) {
        // 仅本图重命名 = 删旧标签 + 添加新文本标签
        await del(`/images/${id}/tags/${e.tagId}`)
        await post(`/images/${id}/tags`, { name: e.newName, category: 'general' })
        ok++
      }
    }
    // 新增标签：空白自动丢弃
    for (const t of newTags.value) {
      const name = t.value.trim()
      if (!name) continue
      await post(`/images/${id}/tags`, { name, category: t.category })
      ok++
    }
    ElMessage.success(`已生效 ${ok} 项修改`)
    exitEditMode()
    visible.value = false
    emit('saved')
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

// ---- 追加增强1：修改原始标签文本（三级窗口）----
const rawTagsVisible = ref(false)
const rawTagsText = ref('')

/** 生成当前标签的原始文本（画师名,\n角色名,\n作品名,\n常规1,常规2,…，显示用空格形式）。 */
function buildRawTagsText(): string {
  const lines: string[] = []
  for (const g of tagGroupDefs) {
    const list = tagGroups.value[g.key]
    if (list.length === 0) continue
    lines.push(`${list.map((t) => displayTagName(t.name)).join(',')},`)
  }
  return lines.join('\n')
}

function openRawTagsDialog() {
  rawTagsText.value = buildRawTagsText()
  rawTagsVisible.value = true
}

/** 解析原始文本（每行逗号分隔）→ 分类标签。返回失败信息或 null。 */
function parseRawTags(text: string): Array<{ name: string; category: string }> | string {
  const out: Array<{ name: string; category: string }> = []
  const cats: Record<string, string> = { 画师: 'artist', 角色: 'character', 作品: 'copyright', 常规: 'general' }
  const lines = text.split('\n')
  for (const raw of lines) {
    const line = raw.trim()
    if (!line) continue
    // 支持「画师: xxx」或裸行；行尾逗号可忽略
    const content = line.replace(/,$/, '')
    const catMatch = content.match(/^(画师|角色|作品|常规)\s*[:：]\s*(.*)$/)
    const items = catMatch ? catMatch[2].split(',') : content.split(',')
    const cat = catMatch ? cats[catMatch[1]] : 'general'
    for (const it of items) {
      const name = it.trim()
      if (name) out.push({ name, category: cat })
    }
  }
  return out
}

/** 生效：删除全部旧标签，按解析结果重建（保留来源 manual）。 */
async function applyRawTags() {
  if (props.imageId == null) return
  const id = props.imageId
  const parsed = parseRawTags(rawTagsText.value)
  if (typeof parsed === 'string') {
    ElMessage.error(parsed)
    return
  }
  try {
    // 删旧
    for (const t of props.tags) {
      await del(`/images/${id}/tags/${t.tag_id}`)
    }
    // 建新（按分类；BUG1：空格输入归一化为下划线规范名）
    for (const p of parsed) {
      await post(`/images/${id}/tags`, { name: searchTagKey(p.name), category: p.category })
    }
    ElMessage.success(`已重建 ${parsed.length} 个标签`)
    rawTagsVisible.value = false
    exitEditMode()
    visible.value = false
    emit('saved')
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}
</script>

<template>
  <!-- 增强2：修改标签二级窗口 -->
  <el-dialog v-model="visible" title="修改标签" width="560px" append-to-body @closed="exitEditMode">
    <div class="edit-hint">点标签编辑文本 · ×划掉删除（再点恢复） · 改字后出现↻还原文本</div>
    <!-- 追加增强1：修改原始标签文本入口 -->
    <div class="edit-hint">
      <el-button size="small" text type="primary" @click="openRawTagsDialog">修改原始标签文本…</el-button>
    </div>
    <div class="tag-groups">
      <div v-for="g in tagGroupDefs" :key="g.key" class="tag-group">
        <!-- 追加BUG2：空分类也显示 + 号，允许新增任意分类标签 -->
        <span class="tag-group-label">
          {{ g.label }}
          <span class="tag-add" title="新增标签" @click="addNewTag(g.key)">＋</span>
        </span>
        <template v-for="t in tagGroups[g.key]" :key="t.name">
          <el-input
            v-if="editingName === t.name"
            v-model="editBuffer"
            size="small"
            class="tag-edit-input"
            autofocus
            @blur="stopEditTag(t.name)"
            @keyup.enter="stopEditTag(t.name)"
          />
          <span
            v-else
            class="tag-edit"
            :class="{ deleted: tagEdits[t.name]?.deleted }"
            @click="startEditTag(t.name)"
          >
            {{ tagEdits[t.name]?.newName ?? (t.name_cn ? `${t.name}(${t.name_cn})` : t.name) }}
            <span
              v-if="tagEdits[t.name]?.dirty && !tagEdits[t.name]?.deleted"
              class="tag-revert"
              title="还原文本（不还原删除状态）"
              @click.stop="applyTagEdit(t.name, t.name)"
            >↻</span>
            <span
              class="tag-del"
              :class="{ armed: tagEdits[t.name]?.deleted }"
              :title="tagEdits[t.name]?.deleted ? '再点恢复' : '标记删除'"
              @click.stop="toggleTagDelete(t.name)"
            >✕</span>
          </span>
        </template>
        <template v-for="nt in newTagsOf(g.key)" :key="nt.key">
          <el-input
            v-if="editingNewKey === nt.key"
            v-model="newTagBuffer"
            size="small"
            class="tag-edit-input"
            autofocus
            placeholder="输入新标签…"
            @blur="commitNewTagEdit(nt.key)"
            @keyup.enter="commitNewTagEdit(nt.key)"
          />
          <span
            v-else
            class="tag-edit new-tag"
            @click="startNewTagEdit(nt.key)"
          >
            {{ nt.value || '（空）' }}
            <span class="tag-del" @click.stop="removeNewTag(nt.key)">✕</span>
          </span>
        </template>
      </div>
    </div>
    <template #footer>
      <el-button @click="visible = false">取消</el-button>
      <el-button type="success" @click="applyTagChanges">生效修改</el-button>
    </template>
  </el-dialog>

  <!-- 追加增强1：修改原始标签文本（三级窗口） -->
  <el-dialog v-model="rawTagsVisible" title="修改原始标签文本" width="480px" append-to-body>
    <div class="edit-hint">每行一个分类，行内逗号分隔，行尾逗号可省略</div>
    <el-input
      v-model="rawTagsText"
      type="textarea"
      :rows="10"
      placeholder="画师名,&#10;角色名,&#10;作品名,&#10;常规标签1,常规标签2,常规标签3,常规标签4,"
    />
    <template #footer>
      <el-button @click="rawTagsVisible = false">取消</el-button>
      <el-button type="success" @click="applyRawTags">生效</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.edit-hint {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  margin: 0 0 8px;
  line-height: 1.5;
}
.tag-groups {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.tag-group {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  align-items: center;
}
.tag-group-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--el-text-color-secondary);
  margin-right: 2px;
}

/* 改进2：标签编辑模式 */
.tag-edit {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  margin: 1px 3px 1px 0;
  padding: 1px 5px;
  border: 1px dashed var(--el-border-color);
  border-radius: 6px;
  background: var(--el-fill-color-light);
  cursor: text;
  font-size: 12px;
  line-height: 20px;
  height: 22px;
  transition: opacity 0.15s;
  user-select: none;
  max-width: 100%;
}
.tag-edit:hover {
  border-color: var(--el-color-primary);
}
.tag-edit.deleted {
  opacity: 0.45;
  text-decoration: line-through;
}
.tag-edit-input {
  width: 110px;
  margin: 1px 3px 1px 0;
}
.tag-edit-input :deep(.el-input__wrapper) {
  box-shadow: none;
  padding: 0 6px;
  min-height: 24px;
}
.tag-add {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  margin-left: 4px;
  border-radius: 50%;
  color: #fff;
  background: var(--el-color-primary);
  font-size: 12px;
  line-height: 1;
  cursor: pointer;
  vertical-align: middle;
  user-select: none;
}
.tag-add:hover {
  opacity: 0.85;
}
.tag-del {
  cursor: pointer;
  color: var(--el-color-danger);
  font-size: 12px;
  line-height: 1;
  padding: 2px;
  border-radius: 4px;
  user-select: none;
}
.tag-del:hover {
  background: var(--el-color-danger-light-7);
}
.tag-del.armed {
  color: #fff;
  background: var(--el-color-danger);
}
.tag-revert {
  cursor: pointer;
  color: var(--el-color-warning);
  font-size: 14px;
  line-height: 1;
  padding: 2px;
  border-radius: 4px;
  user-select: none;
}
.tag-revert:hover {
  background: var(--el-color-warning-light-7);
}
</style>
