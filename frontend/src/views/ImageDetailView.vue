<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useLibraryStore, originalUrl } from '@/stores/library'
import { useTaskStore } from '@/stores/tasks'
import { get, post, put } from '@/api/client'

const route = useRoute()
const router = useRouter()
const library = useLibraryStore()
const taskStore = useTaskStore()

const image = computed(() => library.images.find((i) => i.id === Number(route.params.id)))
const tags = ref<Array<{ name: string; name_cn: string | null; category: string; source: string }>>([])
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
    const t = await get<{ tags: Array<{ name: string; name_cn: string | null; category: string; source: string }> }>(
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
    inputValue: cur,
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

onMounted(() => {
  loadDetail()
  window.addEventListener('keydown', onKeydown)
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
})
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
          <a v-if="image.sourceUrl" :href="image.sourceUrl" target="_blank" rel="noopener" class="src-link">{{ image.sourceUrl }}</a>
          <span v-else class="muted">未溯源</span>
          <el-button size="small" text type="primary" style="margin-left: 8px" @click="editSourceUrl">编辑</el-button>
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
          <el-tag v-else-if="aiChecked" type="info" plain style="margin-left: 6px">无需打标</el-tag>
          <el-tag v-else type="warning" plain style="margin-left: 6px">未打标</el-tag>
        </el-descriptions-item>
      </el-descriptions>

      <div class="panel-block">
        <div class="panel-title">
          标签
          <el-button size="small" type="primary" plain style="margin-left: 8px" @click="readAiInfo">
            读取 AI 生成信息
          </el-button>
          <el-button size="small" :type="aiChecked ? 'info' : 'warning'" plain @click="toggleAiMark">
            {{ aiChecked ? '取消 AI 标记' : '手动标记为 AI' }}
          </el-button>
        </div>
        <div v-if="hasAnyTags" class="tag-groups">
          <div v-for="g in tagGroupDefs" :key="g.key" class="tag-group">
            <span v-if="tagGroups[g.key as keyof typeof tagGroups].length > 0" class="tag-group-label">{{ g.label }}</span>
            <el-tag v-for="t in tagGroups[g.key as keyof typeof tagGroups]" :key="t.name" class="tag" size="small" :type="g.type">
              {{ t.name_cn ? `${t.name}(${t.name_cn})` : t.name }}
            </el-tag>
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
  <el-empty v-else description="图片不存在或已删除" />
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
</style>
