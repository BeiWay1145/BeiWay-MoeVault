<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useLibraryStore, originalUrl } from '@/stores/library'
import { useTaskStore } from '@/stores/tasks'
import { get, post, put, del } from '@/api/client'

const route = useRoute()
const router = useRouter()
const library = useLibraryStore()
const taskStore = useTaskStore()

const image = computed(() => library.images.find((i) => i.id === Number(route.params.id)))
const tags = ref<Array<{ tag_id: number; name: string; name_cn: string | null; category: string; source: string }>>([])
const aiInfo = ref<string | null>(null)
const aiTags = ref<string[]>([])
const aiChecked = ref(false)
const stageRef = ref<HTMLElement | null>(null)
/** 全屏查看模式（点击图片进入；叉号/ESC 退出；左右键切换）。 */
const fullscreen = ref(false)

const originalSrc = computed(() => (image.value ? originalUrl(image.value.id) : undefined))

// E3: 标签按 danbooru 分类（画师/系列/角色/常规），按后端 category 字段分组
const tagGroups = computed(() => {
  const groups: Record<'artist' | 'copyright' | 'character' | 'general', typeof tags.value> = {
    artist: [],
    copyright: [],
    character: [],
    general: [],
  }
  for (const t of tags.value) {
    const cat = t.category
    if (cat === 'artist') groups.artist.push(t)
    else if (cat === 'copyright') groups.copyright.push(t)
    else if (cat === 'character') groups.character.push(t)
    else groups.general.push(t)
  }
  return groups
})
const tagGroupDefs = [
  { key: 'artist', label: '画师', type: 'danger' as const },
  { key: 'copyright', label: '系列', type: 'warning' as const },
  { key: 'character', label: '角色', type: 'success' as const },
  { key: 'general', label: '常规', type: 'primary' as const },
]
const hasAnyTags = computed(() => tags.value.length > 0)

// 上一张/下一张（基于当前列表顺序）
const indexInList = computed(() => library.images.findIndex((i) => i.id === image.value?.id))

function gotoImage(delta: number) {
  const list = library.images
  if (list.length === 0 || indexInList.value < 0) return
  const next = list[(indexInList.value + delta + list.length) % list.length]
  router.push(`/library/${next.id}`)
}

// 键盘左右键切换 + Del 删除 + ESC 退出全屏
function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    fullscreen.value = false
    return
  }
  if (e.key === 'ArrowLeft') gotoImage(-1)
  else if (e.key === 'ArrowRight') gotoImage(1)
  else if (e.key === 'Delete' || e.key === 'Del') recycle()
}

/** 点击图片区域进入全屏。 */
function enterFullscreen() {
  fullscreen.value = true
}

// 路由参数变化（左右切换/URL 直达）时刷新标签等详情数据（BUG1 修复）
watch(
  () => route.params.id,
  () => {
    tags.value = []
    aiInfo.value = null
    aiTags.value = []
    aiChecked.value = false
    loadDetail()
  },
)

async function loadDetail() {
  const id = Number(route.params.id)
  if (!image.value) {
    await library.fetchImages(500).catch(() => {})
  }
  // 标签
  try {
    const t = await get<{ tags: Array<{ tag_id: number; name: string; name_cn: string | null; category: string; source: string }> }>(
      `/images/${id}/tags`,
    )
    tags.value = t.tags
    aiTags.value = t.tags.filter((x) => x.source === 'ai').map((x) => x.name)
    aiChecked.value = !!image.value?.isAi
  } catch {
    tags.value = []
  }
  // 已存的 AI 信息
  aiChecked.value = !!image.value?.isAi
}

async function readAiInfo() {
  const id = Number(route.params.id)
  try {
    const r = await post<{ ok: boolean; is_ai: boolean; metadata: string | null; prompt?: string | null; negative_prompt?: string | null; tags?: string[] }>(`/images/${id}/ai-info`)
    // 增强4.2：图片已带 AI 标签时，即使本次未读到元信息也不清除 AI 状态
    if (image.value?.isAi) {
      aiChecked.value = true
    } else {
      aiChecked.value = r.is_ai
    }
    aiInfo.value = r.metadata
    if (r.tags && r.tags.length > 0) {
      aiTags.value = r.tags
      ElMessage.success(`已提取 ${r.tags.length} 个 AI 生图标签`)
    } else if (aiChecked.value) {
      ElMessage.success('已标记为 AI 图片（无有效 prompt 标签）')
    } else {
      ElMessage.info('未检测到 AI 生成元信息')
    }
    // 刷新标签列表
    await loadDetail()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

// 增强4.1：手动标记/取消标记 AI（toggle）
async function toggleAiMark() {
  if (!image.value) return
  const next = !aiChecked.value
  try {
    await post(`/images/${image.value.id}/mark-ai`, { ai: next })
    aiChecked.value = next
    // 同步本地列表中的 isAi（图库筛选/角标即时生效）
    const it = library.images.find((i) => i.id === image.value!.id)
    if (it) it.isAi = next
    ElMessage.success(next ? '已标记为 AI 生成图片' : '已取消 AI 生成标记')
    await loadDetail()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

// 放入回收站：无提示；删除后跳到上一张，无上一张则下一张，无则回图库
async function recycle() {
  if (!image.value) return
  const id = image.value.id
  const list = [...library.images]
  const idx = list.findIndex((i) => i.id === id)
  try {
    await post(`/images/${id}/recycle`, { reason: 'manual' })
    // 从本地列表移除，避免 computed 失效
    library.removeImageById(id)
    // 目标：优先上一张，无则下一张，无则回图库
    let target: { id: number } | undefined
    if (idx > 0) target = list[idx - 1]
    else if (idx === 0 && list.length > 1) target = list[1]
    if (target) {
      router.push(`/library/${target.id}`)
    } else {
      router.back()
    }
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 返回来源页（画廊/搜索/主目录）——叉号点击直接返回，不受浏览多张影响。 */
function goBack() {
  const from = library.detailPos?.from
  if (from === 'search') router.push('/search')
  else if (from === 'imports') router.push('/imports')
  else router.push('/library')
}

// 手动打标（BUG3 任务化）：加入打标队列，进度见任务中心
async function manualTag() {
  if (!image.value) return
  try {
    await taskStore.enqueueTag([image.value.id])
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

// 增强5：美学处理（Q-Align 评分任务）
async function rescoreAesthetic() {
  if (!image.value) return
  try {
    await taskStore.enqueueAesthetic([image.value.id])
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

// 增强5：尝试溯源（SauceNAO 任务）
async function trySauce() {
  if (!image.value) return
  try {
    await taskStore.enqueueSauce([image.value.id])
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

function exportImage() {
  if (!image.value) return
  const a = document.createElement('a')
  a.href = originalSrc.value ?? ''
  a.download = image.value.name
  a.click()
}

/** E2: 手动编辑溯源来源链接。 */
async function editSourceUrl() {
  if (!image.value) return
  const id = image.value.id
  const cur = image.value.sourceUrl ?? ''
  const { value } = await ElMessageBox.prompt('输入溯源来源链接（留空清除）', '编辑原图链接', {
    inputValue: cur.replace(/\.json$/, ''),
    inputPlaceholder: 'https://danbooru.donmai.us/posts/...',
  }).catch(() => ({ value: null as string | null }))
  if (value === null) return
  try {
    await put(`/images/${id}/source-url`, { url: value || null })
    const it = library.images.find((i) => i.id === id)
    if (it) it.sourceUrl = value || undefined
    ElMessage.success('原图链接已更新')
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 改进1：重命名库内图片文件（保持哈希目录，冲突即失败）。 */
async function renameImage() {
  if (!image.value) return
  const id = image.value.id
  const { value } = await ElMessageBox.prompt('输入新文件名（含扩展名）', '重命名图片', {
    inputValue: image.value.name,
    inputPattern: /^[^\\/:*?"<>|]+$/,
    inputErrorMessage: '文件名含非法字符（\\ / : * ? " < > |）',
  }).catch(() => ({ value: null as string | null }))
  if (!value || value === image.value.name) return
  try {
    const r = await put<{ ok: boolean; rel_path: string }>(`/images/${id}/rename`, { name: value })
    const it = library.images.find((i) => i.id === id)
    if (it) it.name = decodeURIComponent((r.rel_path as string).split(/[\\/]/).pop() ?? value)
    ElMessage.success('已重命名')
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 原图链接展示：去掉 .json 后缀（页面链接可点击跳转）。 */
const displaySourceUrl = computed(() =>
  image.value?.sourceUrl ? image.value.sourceUrl.replace(/\.json$/, '') : undefined,
)

/** BUG4：点击原图链接 → 桌面壳用系统浏览器打开；浏览器环境 fallback window.open。
 *  根因：window.__TAURI__ 需 withGlobalTauri:true（默认 false）→ 误判为非 Tauri 走了 window.open 被拦截。
 *  改用始终注入的 window.__TAURI_INTERNALS__ 直接 invoke opener。 */
async function openSourceUrl(url: string) {
  const internals = (window as unknown as { __TAURI_INTERNALS__?: { invoke?: (cmd: string, args?: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__
  console.log('[openSourceUrl]', { url, hasInternals: !!internals })
  if (internals?.invoke) {
    try {
      console.log('[openSourceUrl] invoke plugin:opener|open_url')
      await internals.invoke('plugin:opener|open_url', { url })
      console.log('[openSourceUrl] 调用成功')
      return
    } catch (e) {
      console.error('[openSourceUrl] invoke 失败:', e)
      ElMessage.error(`打开链接失败: ${(e as Error).message}`)
      return
    }
  }
  window.open(url, '_blank', 'noopener')
}

// ---- 对比原图（图库图 vs 网络原图） ----
const compareVisible = ref(false)
const compareLoading = ref(false)
const netInfo = ref<{
  width: number | null
  height: number | null
  size_bytes: number | null
  file_url: string | null
} | null>(null)

async function openCompare() {
  if (!image.value) return
  compareVisible.value = true
  compareLoading.value = true
  netInfo.value = null
  try {
    const r = await get<{
      ok: boolean
      page_url: string
      info: { width: number | null; height: number | null; size_bytes: number | null; file_url: string | null }
    }>(`/images/${image.value.id}/source-info`)
    netInfo.value = r.info
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    compareLoading.value = false
  }
}

/** 红绿白：红=网络大，绿=库大，白=相等；null 无法比较 → 灰。 */
function cmpColor(local: number | null | undefined, net: number | null | undefined): string {
  if (net == null || local == null) return ''
  if (net > local) return 'cmp-red'
  if (net < local) return 'cmp-green'
  return 'cmp-white'
}
const localPixels = computed(() => (image.value ? image.value.width * image.value.height : null))
const netPixels = computed(() =>
  netInfo.value?.width && netInfo.value.height ? netInfo.value.width * netInfo.value.height : null,
)
const sizeColor = computed(() => cmpColor(image.value?.sizeBytes ?? null, netInfo.value?.size_bytes))
const pxColor = computed(() => cmpColor(localPixels.value, netPixels.value))

async function keepNetwork() {
  if (!image.value || !netInfo.value?.file_url) return
  try {
    await ElMessageBox.confirm(
      '下载网络原图替换库内图片？\n标签/评分/来源链接保留，旧文件将被删除。',
      '保留网络原图',
      { type: 'warning', confirmButtonText: '替换', cancelButtonText: '取消' },
    )
  } catch {
    return
  }
  try {
    await post(`/images/${image.value.id}/replace-from-url`, { url: netInfo.value.file_url })
    ElMessage.success('已用网络原图替换库内图片')
    await library.fetchImages(500)
    await loadDetail()
    compareVisible.value = false
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

// ---- 标签编辑模式 ----
const editMode = ref(false)
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
  editMode.value = true
  tagEdits.value = {}
  newTags.value = []
  for (const t of tags.value) tagEdits.value[t.name] = { original: t.name, tagId: t.tag_id, dirty: false }
}
function exitEditMode() {
  editMode.value = false
  tagEdits.value = {}
  newTags.value = []
  editingName.value = null
}
/** 点 ×：划掉（待删除），再点恢复。 */
function toggleTagDelete(name: string) {
  const e = tagEdits.value[name]
  if (e) e.deleted = !e.deleted
}
/** 点标签本体：进入编辑（显示输入框，自动聚焦）。 */
function startEditTag(name: string) {
  if (!editMode.value) return
  editingName.value = name
  // 初始化输入缓冲为当前显示值
  const e = tagEdits.value[name]
  editBuffer.value = e?.newName ?? name
}
/** 编辑输入缓冲（v-model 双向绑定）。 */
const editBuffer = ref('')
/** 失焦/回车：提交缓冲到 tagEdits（有效修改才标记 dirty）。 */
function commitEditTag(name: string) {
  if (!editMode.value || editingName.value !== name) return
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
  if (!image.value) return
  const id = image.value.id
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
    await loadDetail()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

onMounted(() => {
  loadDetail()
  window.addEventListener('keydown', onKeydown)
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
})

/** 文件大小格式化。 */
function fmtBytes(b: number): string {
  if (b >= 1 << 30) return `${(b / (1 << 30)).toFixed(2)} GB`
  if (b >= 1 << 20) return `${(b / (1 << 20)).toFixed(1)} MB`
  if (b >= 1 << 10) return `${(b / (1 << 10)).toFixed(0)} KB`
  return `${b} B`
}
</script>

<template>
  <div v-if="image" class="detail">
    <div class="viewer">
      <button class="nav-close" title="返回" @click="goBack">✕</button>
      <button class="nav-arrow left" title="上一张" @click="gotoImage(-1)">‹</button>
      <div ref="stageRef" class="stage" @click="enterFullscreen">
        <el-image :src="originalSrc" fit="contain" class="stage-img" :preview-src-list="[]">
          <template #error>
            <span class="placeholder-name">原图加载失败</span>
          </template>
        </el-image>
      </div>
      <button class="nav-arrow right" title="下一张" @click="gotoImage(1)">›</button>
    </div>

    <!-- 全屏查看模式（E1）：点击图片进入，叉号/ESC 退出，左右键切换 -->
    <Transition name="fs">
      <div v-if="fullscreen" class="fullscreen" @click="fullscreen = false">
        <button class="fs-close" title="退出全屏" @click.stop="fullscreen = false">✕</button>
        <button class="fs-arrow left" title="上一张" @click.stop="gotoImage(-1)">‹</button>
        <el-image :src="originalSrc" fit="contain" class="fs-img" @click.stop>
          <template #error>
            <span class="placeholder-name">原图加载失败</span>
          </template>
        </el-image>
        <button class="fs-arrow right" title="下一张" @click.stop="gotoImage(1)">›</button>
      </div>
    </Transition>

    <div class="panel">
      <el-descriptions :column="1" title="基本信息" border>
        <el-descriptions-item label="文件名">
          <span class="file-name">{{ image.name }}</span>
          <el-button size="small" text type="primary" style="margin-left: 8px" @click="renameImage">重命名</el-button>
        </el-descriptions-item>
        <el-descriptions-item label="格式">{{ image.format?.toUpperCase() ?? (image.name.split('.').pop() ?? '').toUpperCase() }}</el-descriptions-item>
        <el-descriptions-item label="尺寸">{{ image.width }} × {{ image.height }}</el-descriptions-item>
        <el-descriptions-item label="清晰度">
          <span class="num-mono">{{ image.clarity.toFixed(1) }}</span>
        </el-descriptions-item>
        <el-descriptions-item label="美学分">
          <span class="num-mono">{{ image.aesthetic?.toFixed(1) ?? '—' }}</span>
        </el-descriptions-item>
        <el-descriptions-item label="导入时间">{{ new Date(image.importedAt * 1000).toLocaleDateString() }}</el-descriptions-item>
        <el-descriptions-item label="原图链接">
          <a
            v-if="displaySourceUrl"
            :href="displaySourceUrl"
            class="src-link"
            @click.prevent="openSourceUrl(displaySourceUrl)"
          >{{ displaySourceUrl }}</a>
          <span v-else class="muted">未溯源</span>
          <el-button size="small" text type="primary" style="margin-left: 8px" @click="editSourceUrl">编辑</el-button>
          <el-button v-if="displaySourceUrl" size="small" text type="success" style="margin-left: 4px" @click="openCompare">对比原图</el-button>
        </el-descriptions-item>
        <el-descriptions-item label="状态">
          <el-tag v-if="image.isRedundant" type="warning">冗余候选</el-tag>
          <el-tag v-else type="success">正常</el-tag>
          <el-tag v-if="aiChecked" type="primary" style="margin-left: 6px">AI 生成</el-tag>
          <el-tag v-else type="info" plain style="margin-left: 6px">非 AI 生成</el-tag>
          <el-tag v-if="image.sourceUrl || (image.source !== undefined && image.source !== 'local' && image.source !== '')" type="success" plain style="margin-left: 6px">已溯源</el-tag>
          <el-tag v-else-if="image.noAutoSauce || aiChecked" type="warning" plain style="margin-left: 6px">不可溯源</el-tag>
          <el-tag v-else type="info" plain style="margin-left: 6px">未溯源</el-tag>
          <el-tag v-if="tags.length > 0" type="success" plain style="margin-left: 6px">已打标</el-tag>
          <el-tag v-else type="warning" plain style="margin-left: 6px">未打标</el-tag>
        </el-descriptions-item>
      </el-descriptions>

      <div class="panel-block">
        <div class="panel-title">
          标签
          <el-button size="small" type="primary" plain @click="readAiInfo">
            读取 AI 生成信息
          </el-button>
          <el-button size="small" :type="aiChecked ? 'info' : 'warning'" plain @click="toggleAiMark">
            {{ aiChecked ? '取消 AI 标记' : '手动标记为 AI' }}
          </el-button>
          <el-button
            v-if="!editMode"
            size="small"
            type="success"
            plain
            @click="enterEditMode"
          >
            编辑模式
          </el-button>
          <template v-else>
            <el-button size="small" type="success" @click="applyTagChanges">
              生效修改
            </el-button>
            <el-button size="small" plain @click="exitEditMode">取消</el-button>
          </template>
        </div>
        <div v-if="editMode" class="edit-hint">点标签编辑文本 · ×划掉删除（再点恢复） · 改字后出现↻还原文本</div>
        <div v-if="hasAnyTags || editMode" class="tag-groups">
          <div v-for="g in tagGroupDefs" :key="g.key" class="tag-group">
            <span v-if="tagGroups[g.key as keyof typeof tagGroups].length > 0 || editMode" class="tag-group-label">
              {{ g.label }}
              <!-- 改进1：编辑模式下每分类蓝色 + 号 -->
              <span
                v-if="editMode"
                class="tag-add"
                title="新增标签"
                @click="addNewTag(g.key)"
              >＋</span>
            </span>
            <template v-for="t in tagGroups[g.key as keyof typeof tagGroups]" :key="t.name">
              <!-- 编辑模式：点标签变输入框 + × 删除 + 还原 -->
              <template v-if="editMode">
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
              <!-- 普通模式 -->
              <el-tag v-else :key="t.name" class="tag" size="small" :type="g.type">
                {{ t.name_cn ? `${t.name}(${t.name_cn})` : t.name }}
              </el-tag>
            </template>
            <!-- 改进1：新增标签输入框（编辑模式下） -->
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
                class="tag-edit"
                :class="{ 'new-tag': true }"
                @click="startNewTagEdit(nt.key)"
              >
                {{ nt.value || '（空）' }}
                <span class="tag-del" @click.stop="removeNewTag(nt.key)">✕</span>
              </span>
            </template>
          </div>
        </div>
        <el-empty v-else description="暂无标签（可点击上方按钮读取 AI 生成信息）" :image-size="50" />
        <pre v-if="aiInfo" class="ai-info">{{ aiInfo }}</pre>
      </div>

      <div class="panel-block">
        <div class="panel-title">操作</div>
        <el-button type="danger" plain @click="recycle">入回收站</el-button>
        <el-button @click="exportImage">导出</el-button>
        <el-button type="primary" plain @click="manualTag">手动打标</el-button>
        <el-button type="success" plain @click="rescoreAesthetic">美学处理</el-button>
        <el-button plain @click="trySauce">尝试溯源</el-button>
      </div>
    </div>
  </div>

  <!-- 对比原图弹窗：左=图库图，右=网络原图；红=网络大 绿=库大 白=相等 -->
  <el-dialog v-model="compareVisible" title="对比原图" width="900px" :append-to-body="true">
    <div v-loading="compareLoading" class="compare-body">
      <div class="compare-side">
        <div class="compare-side-title">图库原图</div>
        <el-image :src="originalSrc" fit="contain" class="compare-img" />
        <div class="compare-meta">
          <div :class="pxColor">分辨率：{{ image?.width }} × {{ image?.height }}</div>
          <div :class="sizeColor">文件大小：{{ fmtBytes(image?.sizeBytes ?? 0) }}</div>
        </div>
      </div>
      <div class="compare-divider">vs</div>
      <div class="compare-side">
        <div class="compare-side-title">网络原图</div>
        <template v-if="netInfo">
          <el-image v-if="netInfo.file_url" :src="netInfo.file_url" fit="contain" class="compare-img" :preview-src-list="[netInfo.file_url]">
            <template #error><span class="placeholder-name">网络图加载失败</span></template>
          </el-image>
          <div v-else class="compare-img placeholder-name">无网络图链接</div>
          <div class="compare-meta">
            <div :class="pxColor">
              分辨率：{{ netInfo.width && netInfo.height ? `${netInfo.width} × ${netInfo.height}` : '未知' }}
            </div>
            <div :class="sizeColor">
              文件大小：{{ netInfo.size_bytes != null ? fmtBytes(netInfo.size_bytes) : '未知' }}
            </div>
          </div>
        </template>
        <el-empty v-else-if="!compareLoading" description="无法获取网络图信息" :image-size="50" />
      </div>
    </div>
    <template #footer>
      <span class="hint" style="margin-right: auto">红色=网络图更大 绿色=图库图更大 白色=相等</span>
      <el-button @click="compareVisible = false">保留图库原图</el-button>
      <el-button type="primary" :disabled="!netInfo?.file_url" @click="keepNetwork">保留网络原图</el-button>
    </template>
  </el-dialog>
  <el-empty v-if="!image" description="图片不存在或已删除" />
</template>

<style scoped>
.detail {
  display: flex;
  gap: 16px;
  height: 100%;
}
.viewer {
  flex: 1;
  min-width: 0;
  position: relative;
  display: flex;
}
.stage {
  flex: 1;
  border-radius: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: #000;
  min-height: 320px;
  overflow: hidden;
}
.stage-img {
  width: 100%;
  height: 100%;
}
.stage-img :deep(.el-image__inner) {
  width: 100%;
  height: 100%;
  object-fit: contain;
}
.nav-close {
  position: absolute;
  top: 12px;
  right: 12px;
  z-index: 10;
  width: 36px;
  height: 36px;
  border: none;
  border-radius: 50%;
  background: rgba(0, 0, 0, 0.45);
  color: #fff;
  font-size: 16px;
  line-height: 1;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s;
}
.nav-close:hover {
  background: rgba(0, 0, 0, 0.7);
}
.nav-arrow {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  z-index: 10;
  width: 44px;
  height: 44px;
  border: none;
  border-radius: 50%;
  background: rgba(0, 0, 0, 0.45);
  color: #fff;
  font-size: 30px;
  line-height: 1;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s;
}
.nav-arrow:hover {
  background: rgba(0, 0, 0, 0.7);
}
.nav-arrow.left {
  left: 16px;
}
.nav-arrow.right {
  right: 16px;
}
.placeholder-name {
  color: #888;
}
.src-link {
  color: var(--el-color-primary);
  word-break: break-all;
  font-size: 12px;
}
.muted {
  color: var(--el-text-color-secondary);
  font-size: 12px;
}
/* 全屏查看模式（E1） */
.fullscreen {
  position: fixed;
  inset: 0;
  z-index: 2000;
  background: rgba(0, 0, 0, 0.97);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: zoom-out;
}
.fs-img {
  width: 100%;
  height: 100%;
  cursor: zoom-out;
}
.fs-close {
  position: absolute;
  top: 16px;
  right: 16px;
  z-index: 10;
  width: 40px;
  height: 40px;
  border: none;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.18);
  color: #fff;
  font-size: 18px;
  line-height: 1;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s;
}
.fs-close:hover {
  background: rgba(255, 255, 255, 0.35);
}
.fs-arrow {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  z-index: 10;
  width: 52px;
  height: 52px;
  border: none;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.14);
  color: #fff;
  font-size: 36px;
  line-height: 1;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s;
}
.fs-arrow:hover {
  background: rgba(255, 255, 255, 0.32);
}
.fs-arrow.left {
  left: 24px;
}
.fs-arrow.right {
  right: 24px;
}
.fs-enter-active,
.fs-leave-active {
  transition: opacity 0.2s ease;
}
.fs-enter-from,
.fs-leave-to {
  opacity: 0;
}
.panel {
  width: 400px;
  flex: none;
  overflow-y: auto;
}
.panel-block {
  margin-top: 16px;
}
.panel-title {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 8px;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}
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
.ai-info {
  margin-top: 8px;
  padding: 8px;
  background: var(--el-fill-color-light);
  border-radius: 6px;
  font-size: 11px;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 200px;
  overflow-y: auto;
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
.hint {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  margin-left: 8px;
}

/* 改进1：对比原图弹窗 */
.file-name {
  word-break: break-all;
}
.compare-body {
  display: flex;
  align-items: stretch;
  gap: 16px;
  min-height: 300px;
}
.compare-side {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.compare-side-title {
  text-align: center;
  font-weight: 600;
  margin-bottom: 8px;
}
.compare-img {
  flex: 1;
  min-height: 240px;
  background: var(--el-fill-color-light);
  border-radius: 6px;
}
.compare-img :deep(.el-image__inner) {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
}
.compare-divider {
  align-self: center;
  font-weight: 700;
  color: var(--el-text-color-secondary);
}
.compare-meta {
  margin-top: 8px;
  font-size: 13px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.cmp-red {
  color: var(--el-color-danger);
  font-weight: 600;
}
.cmp-green {
  color: var(--el-color-success);
  font-weight: 600;
}
.cmp-white {
  color: var(--el-text-color-primary);
}
</style>
