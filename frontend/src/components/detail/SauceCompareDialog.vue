<script setup lang="ts">
/**
 * 详情页「对比原图」弹窗（图库图 vs 网络原图）。
 *
 * 从 ImageDetailView.vue 抽出（原文件 1895 行，承担 8 类职责）。
 * 抽出边界：本组件只负责「拉取源站信息 + 左右对比渲染 + 触发替换」，
 * 图片本身的详情状态仍归父组件 —— 替换成功后通过 replaced 事件
 * 通知父组件刷新列表与详情（父组件还要处理浏览器缓存绕过）。
 *
 * 颜色口径：红=网络图更大，绿=图库图更大，白=相等，灰=无法比较。
 */
import { computed, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { get, post } from '@/api/client'
import { fmtBytes } from '@/utils/format'

const props = defineProps<{
  /** 是否显示弹窗（v-model）。 */
  modelValue: boolean
  /** 当前图片 id；为空时不发请求。 */
  imageId: number | null
  /** 图库图的分辨率与大小（右栏「对比原图」需要与网络图对比）。 */
  width: number | null
  height: number | null
  sizeBytes: number | null
  /** 图库原图的 URL（含缓存绕过参数，由父组件计算）。 */
  originalSrc: string | undefined
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', v: boolean): void
  /** 已用网络原图替换库内图片，父组件应刷新列表/详情并绕过图片缓存。 */
  (e: 'replaced'): void
}>()

const visible = computed({
  get: () => props.modelValue,
  set: (v: boolean) => emit('update:modelValue', v),
})

const compareLoading = ref(false)
const netInfo = ref<{
  width: number | null
  height: number | null
  size_bytes: number | null
  file_url: string | null
} | null>(null)
/** 对比原图错误提示（信息为空时显示）。 */
const compareError = ref('')
/** 网络原图是否加载成功（原生 img @load/@error）。 */
const netImgOk = ref(false)

async function load() {
  if (props.imageId == null) return
  compareLoading.value = true
  netInfo.value = null
  netImgOk.value = false
  compareError.value = ''
  try {
    const r = await get<{
      ok: boolean
      page_url: string
      info: { width: number | null; height: number | null; size_bytes: number | null; file_url: string | null }
    }>(`/images/${props.imageId}/source-info`)
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

// 打开即拉取（等价于原来的 openCompare 入口）
watch(
  () => props.modelValue,
  (v) => {
    if (v) load()
  },
)

/** 网络图经后端代理加载（绕开 WebView2 网络栈差异 + 防盗链）。 */
function proxyUrl(url: string | null): string | undefined {
  if (!url) return undefined
  return `/api/v1/proxy-image?url=${encodeURIComponent(url)}`
}

/** 红绿白：红=网络大，绿=库大，白=相等；null 无法比较 → 无样式（灰）。 */
function cmpColor(local: number | null | undefined, net: number | null | undefined): string {
  if (net == null || local == null) return ''
  if (net > local) return 'cmp-red'
  if (net < local) return 'cmp-green'
  return 'cmp-white'
}

const localPixels = computed(() => (props.width != null && props.height != null ? props.width * props.height : null))
const netPixels = computed(() =>
  netInfo.value?.width && netInfo.value.height ? netInfo.value.width * netInfo.value.height : null,
)
const sizeColor = computed(() => cmpColor(props.sizeBytes, netInfo.value?.size_bytes))
const pxColor = computed(() => cmpColor(localPixels.value, netPixels.value))

async function keepNetwork() {
  if (props.imageId == null || !netInfo.value?.file_url) return
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
    await post(`/images/${props.imageId}/replace-from-url`, { url: netInfo.value.file_url })
    visible.value = false
    emit('replaced')
    ElMessage.success('已用网络原图替换库内图片')
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}
</script>

<template>
  <!-- 对比原图弹窗：左=图库图，右=网络原图；红=网络大 绿=库大 白=相等 -->
  <el-dialog v-model="visible" title="对比原图" width="900px" :append-to-body="true">
    <div v-loading="compareLoading" class="compare-body">
      <div class="compare-side">
        <div class="compare-side-title">图库原图</div>
        <el-image :src="originalSrc" fit="contain" class="compare-img" />
        <div class="compare-meta">
          <div :class="pxColor">分辨率：{{ width }} × {{ height }}</div>
          <div :class="sizeColor">文件大小：{{ fmtBytes(sizeBytes ?? 0) }}</div>
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
      <el-button @click="visible = false">保留图库原图</el-button>
      <el-button type="primary" :disabled="!netInfo?.file_url" @click="keepNetwork">保留网络原图</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.hint {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  margin-left: 8px;
}
.placeholder-name {
  color: #888;
}

/* 改进1：对比原图弹窗 */
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
