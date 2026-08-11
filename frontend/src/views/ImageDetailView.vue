<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import { useLibraryStore, originalUrl } from '@/stores/library'
import { get, post } from '@/api/client'

const route = useRoute()
const router = useRouter()
const library = useLibraryStore()

const image = computed(() => library.images.find((i) => i.id === Number(route.params.id)))
const tags = ref<Array<{ name: string; name_cn: string | null; category: string; source: string }>>([])
const aiInfo = ref<string | null>(null)
const aiTags = ref<string[]>([])
const aiChecked = ref(false)
const stageRef = ref<HTMLElement | null>(null)

const originalSrc = computed(() => (image.value ? originalUrl(image.value.id) : undefined))

// 上一张/下一张（基于当前列表顺序）
const indexInList = computed(() => library.images.findIndex((i) => i.id === image.value?.id))

function gotoImage(delta: number) {
  const list = library.images
  if (list.length === 0 || indexInList.value < 0) return
  const next = list[(indexInList.value + delta + list.length) % list.length]
  router.push(`/library/${next.id}`)
}

// 键盘左右键切换 + Del 删除（放入回收站）
function onKeydown(e: KeyboardEvent) {
  if (e.key === 'ArrowLeft') gotoImage(-1)
  else if (e.key === 'ArrowRight') gotoImage(1)
  else if (e.key === 'Delete' || e.key === 'Del') recycle()
}

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
    aiChecked.value = r.is_ai
    aiInfo.value = r.metadata
    if (r.tags && r.tags.length > 0) {
      aiTags.value = r.tags
      ElMessage.success(`已提取 ${r.tags.length} 个 AI 生图标签`)
    } else if (!r.is_ai) {
      ElMessage.info('未检测到 AI 生成元信息')
    } else {
      ElMessage.success('已标记为 AI 图片（无有效 prompt 标签）')
    }
    // 刷新标签列表
    await loadDetail()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function markAsAi() {
  if (!image.value) return
  try {
    await post(`/images/${image.value.id}/mark-ai`)
    aiChecked.value = true
    ElMessage.success('已手动标记为 AI 图片')
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
      router.push('/library')
    }
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

// 手动打标：本地 cl_tagger 模型打标（替代原 sidecar 功能）
async function manualTag() {
  if (!image.value) return
  try {
    const r = await post<{ ok: boolean; count?: number; message?: string }>(`/images/${image.value.id}/retag`, { force: true })
    ElMessage.success(r.count != null ? `打标完成，写入 ${r.count} 个标签` : (r.message ?? '打标完成'))
    tags.value = []
    // 刷新标签
    const t = await get<{ tags: Array<{ name: string; name_cn: string | null; category: string; source: string }> }>(
      `/images/${image.value.id}/tags`,
    )
    tags.value = t.tags
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
      <button class="nav-close" title="返回图库" @click="router.push('/library')">✕</button>
      <button class="nav-arrow left" title="上一张" @click="gotoImage(-1)">‹</button>
      <div ref="stageRef" class="stage">
        <el-image :src="originalSrc" fit="contain" class="stage-img">
          <template #error>
            <span class="placeholder-name">原图加载失败</span>
          </template>
        </el-image>
      </div>
      <button class="nav-arrow right" title="下一张" @click="gotoImage(1)">›</button>
    </div>

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
        <el-descriptions-item label="状态">
          <el-tag v-if="image.isRedundant" type="warning">冗余候选</el-tag>
          <el-tag v-else type="success">正常</el-tag>
          <el-tag v-if="aiChecked" type="primary" style="margin-left: 6px">AI 生成</el-tag>
        </el-descriptions-item>
      </el-descriptions>

      <div class="panel-block">
        <div class="panel-title">
          标签
          <el-button size="small" type="primary" plain style="margin-left: 8px" @click="readAiInfo">
            读取 AI 生成信息
          </el-button>
          <el-button size="small" type="warning" plain :disabled="aiChecked" @click="markAsAi">
            手动标记为 AI
          </el-button>
        </div>
        <div v-if="tags.length > 0" class="tag-list">
          <el-tag v-for="t in tags" :key="t.name" class="tag" size="small"
            :type="t.source === 'ai' ? 'primary' : 'info'">
            {{ t.name_cn ? `${t.name}(${t.name_cn})` : t.name }}
          </el-tag>
        </div>
        <el-empty v-else description="暂无标签（可点击上方按钮读取 AI 生成信息）" :image-size="50" />
        <pre v-if="aiInfo" class="ai-info">{{ aiInfo }}</pre>
      </div>

      <div class="panel-block">
        <div class="panel-title">操作</div>
        <el-button type="danger" plain @click="recycle">入回收站</el-button>
        <el-button @click="exportImage">导出</el-button>
        <el-button type="primary" plain @click="manualTag">手动打标</el-button>
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
.tag-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
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
