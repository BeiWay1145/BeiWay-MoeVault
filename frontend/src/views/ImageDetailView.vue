<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage } from 'element-plus'
import { useLibraryStore, thumbUrl, originalUrl } from '@/stores/library'
import { get, post } from '@/api/client'

const route = useRoute()
const router = useRouter()
const library = useLibraryStore()

const image = computed(() => library.images.find((i) => i.id === Number(route.params.id)))
const tags = ref<Array<{ name: string; name_cn: string | null; category: string; source: string }>>([])
const similar = ref<Array<{ id: number; thumb_rel: string; width: number; height: number }>>([])
const aiInfo = ref<string | null>(null)
const aiChecked = ref(false)
const loadingSimilar = ref(false)

const originalSrc = computed(() => (image.value ? originalUrl(image.value.id) : undefined))

// 上一张/下一张（基于当前列表顺序）
const indexInList = computed(() => library.images.findIndex((i) => i.id === image.value?.id))

function gotoImage(delta: number) {
  const list = library.images
  if (list.length === 0 || indexInList.value < 0) return
  const next = list[(indexInList.value + delta + list.length) % list.length]
  router.push(`/library/${next.id}`)
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
  } catch {
    tags.value = []
  }
  // 相似图
  loadingSimilar.value = true
  try {
    const s = await get<{ items: Array<{ id: number; thumb_rel: string; width: number; height: number }> }>(
      `/images/${id}/similar?limit=8`,
    )
    similar.value = s.items
  } catch {
    similar.value = []
  } finally {
    loadingSimilar.value = false
  }
  // 已存的 AI 信息（通过 image list 判断）
  aiChecked.value = !!image.value?.isAi
}

async function readAiInfo() {
  const id = Number(route.params.id)
  try {
    const r = await post<{ ok: boolean; is_ai: boolean; metadata: string | null }>(`/images/${id}/ai-info`)
    aiChecked.value = r.is_ai
    aiInfo.value = r.metadata
    if (!r.is_ai) ElMessage.info('未检测到 AI 生成元信息（可能不是 AI 图或已去除元数据）')
    else ElMessage.success('检测到 AI 生成信息')
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function recycle() {
  if (!image.value) return
  try {
    await post(`/images/${image.value.id}/recycle`, { reason: 'manual' })
    ElMessage.success('已移入回收站')
    router.push('/library')
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function genSidecar() {
  if (!image.value) return
  try {
    const r = await post<{ ok: boolean; path: string }>(`/images/${image.value.id}/sidecar`)
    ElMessage.success(`已生成 sidecar: ${r.path}`)
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

onMounted(loadDetail)
</script>

<template>
  <div v-if="image" class="detail">
    <div class="viewer">
      <div class="stage">
        <el-image :src="originalSrc" fit="contain" class="stage-img">
          <template #error>
            <span class="placeholder-name">原图加载失败</span>
          </template>
        </el-image>
      </div>
      <div class="viewer-toolbar">
        <el-button @click="gotoImage(-1)">◀ 上一张</el-button>
        <el-button @click="gotoImage(1)">下一张 ▶</el-button>
        <el-button @click="router.push('/library')">返回图库</el-button>
      </div>
      <!-- 相似图片：无相似时隐藏 -->
      <div v-if="similar.length > 0" class="similar">
        <span class="similar-title">相似图片</span>
        <div class="similar-row">
          <el-image
            v-for="s in similar"
            :key="s.id"
            class="similar-thumb"
            :src="thumbUrl(s.thumb_rel)"
            fit="cover"
            :preview-src-list="[thumbUrl(s.thumb_rel)]"
            @click="router.push(`/library/${s.id}`)"
          />
        </div>
      </div>
      <el-empty v-else-if="!loadingSimilar && similar.length === 0" description="无相似图片" :image-size="50" />
    </div>

    <div class="panel">
      <el-descriptions :column="1" title="基本信息" border>
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
        </div>
        <div v-if="tags.length > 0" class="tag-list">
          <el-tag v-for="t in tags" :key="t.name" class="tag" size="small">
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
        <el-button @click="genSidecar">生成 sidecar .txt</el-button>
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
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.stage {
  flex: 1;
  border-radius: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: #000;
  min-height: 320px;
}
.stage-img {
  max-width: 100%;
  max-height: 100%;
}
.placeholder-name {
  color: #888;
}
.viewer-toolbar {
  display: flex;
  gap: 8px;
  justify-content: center;
}
.similar-title {
  font-size: 13px;
  color: var(--el-text-color-secondary);
}
.similar-row {
  display: flex;
  gap: 8px;
  margin-top: 6px;
  flex-wrap: wrap;
}
.similar-thumb {
  width: 96px;
  height: 72px;
  border-radius: 6px;
  cursor: pointer;
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
