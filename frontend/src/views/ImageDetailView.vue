<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { ArrowDown, ArrowRight, ArrowLeft } from '@element-plus/icons-vue'
import { displayTagName, searchTagKey } from '@/utils/tagNormalize'
import { useLibraryStore, originalUrl } from '@/stores/library'
import { useTaskStore } from '@/stores/tasks'
import { useSettingsStore } from '@/stores/settings'
import { get, post, put, del } from '@/api/client'

const route = useRoute()
const router = useRouter()
const library = useLibraryStore()
const taskStore = useTaskStore()
const settingsStore = useSettingsStore()

const image = computed(() => library.images.find((i) => i.id === Number(route.params.id)))
const tags = ref<Array<{ tag_id: number; name: string; name_cn: string | null; category: string; source: string; image_count: number }>>([])
const aiInfo = ref<string | null>(null)
const aiTags = ref<string[]>([])
const aiChecked = ref(false)
const stageRef = ref<HTMLElement | null>(null)
/** 全屏查看模式（点击图片进入；叉号/ESC 退出；左右键切换）。 */
const fullscreen = ref(false)

/** 上一张图的 src（新图加载完成前保留显示）。 */
const prevSrc = ref<string | undefined>(undefined)
/** 图片替换后强制绕过浏览器缓存（URL 加时间戳）。 */
const cacheBust = ref(0)

const originalSrc = computed(() => {
  if (!image.value) return undefined
  const base = originalUrl(image.value.id)
  return cacheBust.value ? `${base}?t=${cacheBust.value}` : base
})

/** 改进2：切换图片时旧图保留，新图加载完成后淡入（避免灰色闪烁）。 */
const imgLoaded = ref(true)
/** 当前已加载完成显示的图片 id（用于追踪旧图，避免 AB 串图）。 */
const loadedImgId = ref<number | null>(null)
function onStageImgLoad() {
  imgLoaded.value = true
  if (image.value) loadedImgId.value = image.value.id
}
/** 新图加载失败：也恢复显示（避免一直停留在旧图）。 */
function onStageImgError() {
  imgLoaded.value = true
  if (image.value) loadedImgId.value = image.value.id
}
// 路由参数变化（切换图片）→ 新图未加载完成前保留旧图（旧图 = 已加载的那张）
watch(
  () => route.params.id,
  (newId, oldId) => {
    // 已加载完成的图（loadedImgId）才是旧图；仅记住真正的已显示图
    prevSrc.value = loadedImgId.value != null ? originalUrl(loadedImgId.value) : undefined
    imgLoaded.value = false
    // 预加载前后 N 张原图（浏览器缓存命中 → 切换时立即显示，减少闪灰）
    preloadAround()
  },
)

// ---- 全屏大图（原生 img + opacity 叠加，杜绝 el-image 灰色占位） ----
/** 全屏当前显示的原图 src。 */
const fsSrc = ref<string | undefined>(undefined)
/** 全屏新图是否加载完成（加载中旧图保持显示，无灰色）。 */
const fsImgReady = ref(true)
/** 全屏切换时保留的旧图 src（新图 onload 前显示）。 */
const fsPrevSrc = ref<string | undefined>(undefined)

function openFullscreen() {
  fullscreen.value = true
  fsSrc.value = originalSrc.value
  fsImgReady.value = true
  fsPrevSrc.value = undefined
}
/** 全屏切图：新图就位前旧图保持显示（opacity 叠加），onload 后淡入。 */
function fsGoto(delta: number) {
  const ids = orderedIds.value
  if (ids.length === 0 || indexInList.value < 0) return
  const nextId = ids[(indexInList.value + delta + ids.length) % ids.length]
  const newSrc = originalUrl(nextId)
  // 保留当前图作为旧图
  fsPrevSrc.value = fsSrc.value
  fsImgReady.value = false
  // 更新当前图 + 路由（组件 watch 会触发普通视图过渡）
  router.replace(`/library/${nextId}`)
  fsSrc.value = newSrc
}

/** 预加载当前图前后各 N 张原图（用 Image 对象触发浏览器缓存）。 */
function preloadAround() {
  const ids = orderedIds.value
  const idx = indexInList.value
  const n = settingsStore.settings.preload_count || 0
  if (ids.length === 0 || idx < 0 || n <= 0) return
  const cur = Number(route.params.id)
  for (let d = 1; d <= n; d++) {
    for (const sign of [-1, 1]) {
      const target = ids[(idx + sign * d + ids.length) % ids.length]
      if (target !== cur) {
        const img = new Image()
        img.src = originalUrl(target)
      }
    }
  }
}

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
type TagGroupKey = 'artist' | 'copyright' | 'character' | 'general'
const tagGroupDefs: Array<{ key: TagGroupKey; label: string; type: 'danger' | 'warning' | 'success' | 'primary' }> = [
  { key: 'artist', label: '画师', type: 'danger' },
  { key: 'copyright', label: '系列', type: 'warning' },
  { key: 'character', label: '角色', type: 'success' },
  { key: 'general', label: '常规', type: 'primary' },
]
const hasAnyTags = computed(() => tags.value.length > 0)

/** 增强2：有序导航 id 列表——浏览上下文（来源目录组/筛选结果/搜索结果）优先，
 *  当前图不在上下文（直达 URL/刷新）时回退全局库顺序。 */
const orderedIds = computed<number[]>(() => {
  const ctx = library.viewerContext
  const id = Number(route.params.id)
  if (ctx && ctx.ids.length > 0 && ctx.ids.includes(id)) return ctx.ids
  return library.images.map((i) => i.id)
})

// 上一张/下一张（在浏览上下文内循环）
const indexInList = computed(() => orderedIds.value.indexOf(Number(route.params.id)))

function gotoImage(delta: number) {
  const ids = orderedIds.value
  if (ids.length === 0 || indexInList.value < 0) return
  const next = ids[(indexInList.value + delta + ids.length) % ids.length]
  // 组件内切换：router.replace 不推入历史（返回时直接回图库而非逐张回退）
  router.replace(`/library/${next}`)
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
  openFullscreen()
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
    const t = await get<{ tags: Array<{ tag_id: number; name: string; name_cn: string | null; category: string; source: string; image_count: number }> }>(
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

// 放入回收站：无提示；删除后跳到上下文内的上一张，无则下一张，无则回图库
async function recycle() {
  if (!image.value) return
  const id = image.value.id
  const ids = [...orderedIds.value]
  const idx = ids.indexOf(id)
  try {
    await post(`/images/${id}/recycle`, { reason: 'manual' })
    // 从本地列表与浏览上下文移除，避免 computed 失效
    library.removeImageById(id)
    library.removeFromViewerContext(id)
    // 目标：优先上下文内上一张，无则下一张，无则回图库
    let targetId: number | undefined
    if (idx > 0) targetId = ids[idx - 1]
    else if (idx === 0 && ids.length > 1) targetId = ids[1]
    if (targetId != null) {
      router.push(`/library/${targetId}`)
    } else {
      router.back()
    }
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 返回来源页（画廊/主目录）——叉号点击直接返回，不受浏览多张影响。 */
function goBack() {
  const from = library.detailPos?.from
  if (from === 'imports') router.push('/imports')
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
  netImgOk.value = false
  compareError.value = ''
  try {
    const r = await get<{
      ok: boolean
      page_url: string
      info: { width: number | null; height: number | null; size_bytes: number | null; file_url: string | null }
    }>(`/images/${image.value.id}/source-info`)
    netInfo.value = r.info
    // 信息为空 → 明确提示（源站不可达 / 域名未收录），而非静默空白
    if (!r.info.file_url && !r.info.width && !r.info.size_bytes) {
      compareError.value = `无法获取网络图信息：源站（${r.page_url || '未知'}）不可达或域名未收录`
    }
  } catch (e) {
    compareError.value = (e as Error).message
  } finally {
    compareLoading.value = false
  }
}
/** 对比原图错误提示（信息为空时显示）。 */
const compareError = ref('')
/** 网络原图是否加载成功（原生 img @load/@error）。 */
const netImgOk = ref(false)

/** 网络图经后端代理加载（绕开 WebView2 网络栈差异 + 防盗链）。 */
function proxyUrl(url: string | null): string | undefined {
  if (!url) return undefined
  return `/api/v1/proxy-image?url=${encodeURIComponent(url)}`
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
    // 改进3：替换后彻底刷新——更新库列表 + 详情 + 强制绕过浏览器图片缓存（URL 加时间戳）
    await library.fetchImages(500)
    await loadDetail()
    prevSrc.value = originalSrc.value
    imgLoaded.value = false
    cacheBust.value++
    compareVisible.value = false
    ElMessage.success('已用网络原图替换库内图片')
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
    editDialogVisible.value = false
    await loadDetail()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

// ---- 增强2：标签栏（左栏 danbooru 风格）----
/** 修改标签二级窗口。 */
const editDialogVisible = ref(false)
/** 标签栏折叠状态（增强1：整栏收起；增强2：localStorage 持久化）。 */
const tagPanelCollapsed = ref(localStorage.getItem('moevault-detail-tag-collapsed') === '1')
/** 基本信息栏折叠状态（增强1：整栏收起；增强2：localStorage 持久化）。 */
const infoPanelCollapsed = ref(localStorage.getItem('moevault-detail-info-collapsed') === '1')
watch(tagPanelCollapsed, (v) => localStorage.setItem('moevault-detail-tag-collapsed', v ? '1' : '0'))
watch(infoPanelCollapsed, (v) => localStorage.setItem('moevault-detail-info-collapsed', v ? '1' : '0'))

function openEditDialog() {
  enterEditMode()
  editDialogVisible.value = true
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
  if (!image.value) return
  const id = image.value.id
  const parsed = parseRawTags(rawTagsText.value)
  if (typeof parsed === 'string') {
    ElMessage.error(parsed)
    return
  }
  try {
    // 删旧
    for (const t of tags.value) {
      await del(`/images/${id}/tags/${t.tag_id}`)
    }
    // 建新（按分类；BUG1：空格输入归一化为下划线规范名）
    for (const p of parsed) {
      await post(`/images/${id}/tags`, { name: searchTagKey(p.name), category: p.category })
    }
    ElMessage.success(`已重建 ${parsed.length} 个标签`)
    rawTagsVisible.value = false
    exitEditMode()
    editDialogVisible.value = false
    await loadDetail()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 点标签 → 跳到图库页并只搜索该标签。 */
function jumpTag(name: string) {
  library.filter = { ...library.filter, tags: [name] }
  router.push('/library')
}

/** 一键复制标签（用户指定格式）：
 *  画师名,
 *  角色名,
 *  作品名,
 *  常规标签1,常规标签2,常规标签3,常规标签4,
 *  每类一行，行内逗号分隔，行尾逗号。 */
async function copyTags() {
  const lines: string[] = []
  for (const g of tagGroupDefs) {
    const list = tagGroups.value[g.key]
    if (list.length === 0) continue
    lines.push(`${list.map((t) => t.name).join(',')},`)
  }
  const text = lines.join('\n')
  try {
    await navigator.clipboard.writeText(text)
    ElMessage.success(`已复制 ${tags.value.length} 个标签到剪贴板`)
  } catch {
    ElMessage.error('复制失败（剪贴板不可用）')
  }
}

onMounted(async () => {
  await settingsStore.load().catch(() => {})
  loadDetail()
  window.addEventListener('keydown', onKeydown)
  preloadAround()
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
    <!-- 增强1：左栏收起后的展开条 -->
    <div v-if="tagPanelCollapsed" class="panel-expand-bar left" title="展开标签栏" @click="tagPanelCollapsed = false">
      <el-icon><ArrowRight /></el-icon>
    </div>
    <!-- 增强2：左侧标签栏；BUG3：常驻元素 + 宽度过渡（动画生效、无瞬时偏移） -->
    <div class="tag-panel-wrap" :class="{ collapsed: tagPanelCollapsed }">
    <div class="tag-panel">
      <div class="tag-panel-header">
        <span class="tag-panel-title">
          标签（{{ tags.length }}）
        </span>
        <div class="tag-panel-actions">
          <el-button size="small" type="primary" plain @click="copyTags">一键复制</el-button>
          <el-button size="small" type="success" plain @click="openEditDialog">修改标签</el-button>
          <el-button size="small" text :title="'收起标签栏'" @click="tagPanelCollapsed = true">◀ 收起</el-button>
        </div>
      </div>
      <div class="tag-panel-sub">
        <el-button size="small" text type="primary" @click="readAiInfo">读取元数据</el-button>
        <el-button size="small" :type="aiChecked ? 'info' : 'warning'" text plain @click="toggleAiMark">
          {{ aiChecked ? '取消 AI 标记' : '标记为 AI' }}
        </el-button>
      </div>
      <div v-if="hasAnyTags" class="tag-panel-body">
        <div v-for="g in tagGroupDefs" :key="g.key" class="tag-sec">
          <div v-if="tagGroups[g.key as keyof typeof tagGroups].length > 0" class="tag-sec-label">
            {{ g.label }}（{{ tagGroups[g.key as keyof typeof tagGroups].length }}）
          </div>
          <div
            v-for="t in tagGroups[g.key as keyof typeof tagGroups]"
            :key="t.name"
            class="tag-line"
            :title="`${t.name}：${t.image_count} 张`"
            @click="jumpTag(t.name)"
          >
            <span class="tag-line-name">{{ displayTagName(t.name) }}</span>
            <span v-if="t.name_cn" class="tag-line-cn">[“{{ t.name_cn }}”]</span>
            <span class="tag-line-freq">{{ t.image_count }}</span>
          </div>
        </div>
      </div>
      <el-empty v-else description="暂无标签（可读取元数据）" :image-size="50" />
      <pre v-if="aiInfo" class="ai-info">{{ aiInfo }}</pre>
    </div>
    </div>

    <div class="viewer">
      <button class="nav-close" title="返回" @click="goBack">✕</button>
      <!-- 增强2：浏览上下文位置（来源目录组/筛选结果内的序号；无上下文时为全局库顺序） -->
      <div class="nav-pos" :title="library.viewerContext ? `浏览范围：${library.viewerContext.label}` : '浏览范围：全部图片'">
        {{ indexInList >= 0 ? indexInList + 1 : '?' }} / {{ orderedIds.length }}
        <span v-if="library.viewerContext" class="nav-pos-label">{{ library.viewerContext.label }}</span>
      </div>
      <button class="nav-arrow left" title="上一张" @click="gotoImage(-1)">‹</button>
      <div ref="stageRef" class="stage" @click="enterFullscreen">
        <!-- 旧图：新图加载完成前保持显示（v-show 不卸载；无旧图时隐藏） -->
        <el-image
          v-show="!imgLoaded && !!prevSrc"
          :src="prevSrc"
          fit="contain"
          class="stage-img prev-img"
        />
        <!-- 新图：始终渲染（v-show 而非 v-if，确保加载时能触发 @load），加载完成淡入 -->
        <el-image
          v-show="imgLoaded"
          :key="image.id"
          :src="originalSrc"
          fit="contain"
          class="stage-img"
          :class="{ 'img-fade-in': imgLoaded }"
          :preview-src-list="[]"
          @load="onStageImgLoad"
          @error="onStageImgError"
        >
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
        <button class="fs-arrow left" title="上一张" @click.stop="fsGoto(-1)">‹</button>
        <!-- 全屏大图：原生 img + opacity 交叉淡化（新图就位前旧图显示；就位后旧图淡出+新图淡入） -->
        <img
          v-if="fsPrevSrc"
          :key="'prev-' + fsPrevSrc"
          :src="fsPrevSrc"
          class="fs-img"
          :style="{ opacity: fsImgReady ? 0 : 1, transition: 'opacity 0.25s ease' }"
          alt=""
          @click.stop
        />
        <img
          :key="'cur-' + fsSrc"
          :src="fsSrc"
          class="fs-img"
          :style="{ opacity: fsImgReady ? 1 : 0, transition: 'opacity 0.25s ease' }"
          alt=""
          @load="fsImgReady = true"
          @click.stop
        />
        <button class="fs-arrow right" title="下一张" @click.stop="fsGoto(1)">›</button>
      </div>
    </Transition>

    <!-- 增强1：右栏收起后的展开条 -->
    <div v-if="infoPanelCollapsed" class="panel-expand-bar" title="展开右侧信息栏" @click="infoPanelCollapsed = false">
      <el-icon><ArrowLeft /></el-icon>
    </div>
    <!-- BUG3：右栏常驻 + 宽度过渡 -->
    <div class="panel-wrap" :class="{ collapsed: infoPanelCollapsed }">
    <div class="panel">
      <!-- 增强1：整栏收起按钮（右栏收起 → 图片区变大） -->
      <div class="panel-collapse-bar">
        <el-button size="small" text :title="'收起右侧信息栏'" @click="infoPanelCollapsed = true">▶ 收起</el-button>
      </div>
      <div class="panel-block">
        <div class="panel-title">
          基本信息
        </div>
        <el-descriptions :column="1" border>
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
          <img
            v-if="netInfo.file_url"
            :src="proxyUrl(netInfo.file_url)"
            class="compare-img compare-img-native"
            alt="网络原图"
            @load="netImgOk = true"
            @error="netImgOk = false"
          />
          <div v-if="netInfo.file_url && !netImgOk" class="compare-img placeholder-name">网络图加载失败</div>
          <div v-else-if="!netInfo.file_url" class="compare-img placeholder-name">无网络图链接</div>
          <div class="compare-meta">
            <div :class="pxColor">
              分辨率：{{ netInfo.width && netInfo.height ? `${netInfo.width} × ${netInfo.height}` : '未知' }}
            </div>
            <div :class="sizeColor">
              文件大小：{{ netInfo.size_bytes != null ? fmtBytes(netInfo.size_bytes) : '未知' }}
            </div>
          </div>
        </template>
        <el-empty v-else-if="!compareLoading && !compareError" description="无法获取网络图信息" :image-size="50" />
        <el-empty v-else-if="!compareLoading && compareError" :description="compareError" :image-size="50" />
      </div>
    </div>
    <template #footer>
      <span class="hint" style="margin-right: auto">红色=网络图更大 绿色=图库图更大 白色=相等</span>
      <el-button @click="compareVisible = false">保留图库原图</el-button>
      <el-button type="primary" :disabled="!netInfo?.file_url" @click="keepNetwork">保留网络原图</el-button>
    </template>
  </el-dialog>

  <!-- 增强2：修改标签二级窗口（复用原标签编辑模式逻辑） -->
  <el-dialog v-model="editDialogVisible" title="修改标签" width="560px" append-to-body @closed="exitEditMode">
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
        <template v-for="t in tagGroups[g.key as keyof typeof tagGroups]" :key="t.name">
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
    <template #footer>
      <el-button @click="editDialogVisible = false">取消</el-button>
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
  <el-empty v-if="!image" description="图片不存在或已删除" />
</template>

<style scoped>
.detail {
  display: flex;
  gap: 16px;
  height: 100%;
}
/* 增强2：左侧标签栏；BUG3：宽度过渡（动画生效、无瞬时偏移；内容 min-width 防挤压） */
.tag-panel-wrap {
  width: 300px;
  flex: none;
  overflow: hidden;
  transition: width 0.22s ease;
}
.tag-panel-wrap.collapsed {
  width: 0;
}
.tag-panel {
  width: 300px;
  min-width: 300px;
  height: 100%;
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 8px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-height: 0;
}
.tag-panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  border-bottom: 1px solid var(--el-border-color-lighter);
}
.tag-panel-title {
  font-size: 14px;
  font-weight: 600;
}
.tag-panel-actions {
  display: flex;
  gap: 6px;
}
.tag-panel-sub {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 12px;
  border-bottom: 1px solid var(--el-border-color-lighter);
}
.tag-panel-body {
  flex: 1;
  overflow-y: auto;
  padding: 8px 12px;
}
.tag-sec {
  margin-bottom: 12px;
}
.tag-sec-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--el-text-color-secondary);
  margin-bottom: 4px;
}
.tag-line {
  display: flex;
  align-items: baseline;
  gap: 6px;
  padding: 3px 6px;
  border-radius: 6px;
  cursor: pointer;
  line-height: 1.5;
  font-size: 13px;
  user-select: none;
  flex-wrap: wrap;
}
.tag-line:hover {
  background: var(--el-fill-color-light);
}
.tag-line-name {
  color: var(--el-color-primary);
  word-break: break-all;
}
.tag-line-cn {
  color: var(--el-text-color-secondary);
  font-size: 12px;
}
.tag-line-freq {
  color: var(--el-text-color-placeholder);
  font-size: 12px;
  margin-left: auto;
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
/* BUG1：el-image 加载中默认浅灰占位 → 改为透明（露出黑色 stage），暗色模式不刺眼 */
.stage-img :deep(.el-image__placeholder),
.stage-img :deep(.el-image__error) {
  background: transparent;
  color: var(--el-text-color-secondary);
}
/* 改进2：切换图片过渡——旧图保持，新图淡入 */
.stage-img.prev-img {
  position: absolute;
  inset: 0;
}
/* 切换图片：新图加载完成（v-show 显示）时淡入 */
.img-fade-in {
  animation: imgFade 0.25s ease;
}
@keyframes imgFade {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
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
/* 增强2：上下文位置指示 */
.nav-pos {
  position: absolute;
  top: 18px;
  left: 16px;
  z-index: 10;
  padding: 4px 12px;
  border-radius: 14px;
  background: rgba(0, 0, 0, 0.45);
  color: #fff;
  font-size: 12px;
  line-height: 1.4;
  user-select: none;
}
.nav-pos-label {
  margin-left: 6px;
  opacity: 0.75;
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
  object-fit: contain;
  position: absolute;
  inset: 0;
  transition: opacity 0.25s ease;
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
.panel-wrap {
  width: 400px;
  flex: none;
  overflow: hidden;
  transition: width 0.22s ease;
}
.panel-wrap.collapsed {
  width: 0;
}
.panel {
  width: 400px;
  min-width: 400px;
  height: 100%;
  overflow-y: auto;
}
/* 增强1：右栏收起条/展开条 */
.panel-collapse-bar {
  display: flex;
  justify-content: flex-end;
  padding: 0 8px;
  border-bottom: 1px solid var(--el-border-color-lighter);
}
.panel-expand-bar {
  width: 24px;
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  color: var(--el-text-color-secondary);
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 6px;
  user-select: none;
}
.panel-expand-bar.left {
  height: 100%;
  align-self: stretch;
}
.panel-expand-bar:hover {
  color: var(--el-color-primary);
  background: var(--el-fill-color-light);
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
.compare-img-native {
  width: 100%;
  max-height: 100%;
  object-fit: contain;
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
