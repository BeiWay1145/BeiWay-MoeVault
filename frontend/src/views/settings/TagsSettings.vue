<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { ElMessage, ElNotification } from 'element-plus'
import { get, post } from '@/api/client'
import { useSettingsStore } from '@/stores/settings'

const settings = useSettingsStore()

/** 字典缓存状态（供界面展示「已缓存 / 需下载」）。 */
const dictStatus = ref<{
  cached: boolean
  path: string
  bytes: number
  cached_at: number
} | null>(null)
const dictImporting = ref(false)

/** 读取字典缓存状态。 */
async function loadDictStatus() {
  try {
    dictStatus.value = await get<{
      cached: boolean
      path: string
      bytes: number
      cached_at: number
    }>('/dict/status')
  } catch {
    dictStatus.value = null
  }
}

/** 缓存体积的人类可读格式。 */
function fmtDictBytes(n: number): string {
  if (!n) return '—'
  const mb = n / 1024 / 1024
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.round(n / 1024)} KB`
}

/** 缓存时间的人类可读格式。 */
function fmtDictTime(sec: number): string {
  if (!sec) return '—'
  const d = new Date(sec * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

/** 缓存状态文案（模板展示用）。 */
const dictStatusText = computed(() => {
  const s = dictStatus.value
  if (!s?.cached) return '未缓存（首次导入需下载约 60-80MB）'
  return `已缓存 ${fmtDictBytes(s.bytes)} · ${fmtDictTime(s.cached_at)} —— 上游日期一致时直接使用本地文件，无需重新下载`
})

/**
 * 导入中文字典（下载 ffdfkj tag.sqlite → 回填 name_cn，仅填空缺）。
 *
 * 后端已实现缓存：若本地缓存的上游指纹（Last-Modified / ETag）与上游一致，
 * 则直接使用本地文件，跳过 60-80MB 下载；`force` 可强制重新下载。
 *
 * 失败时用**右上角通知**提示（不打断使用），并在提示中给出可手动放置文件的路径。
 */
async function importCnDict(force = false) {
  dictImporting.value = true
  try {
    const r = await post<{
      matched: number
      updated: number
      missing: number
      from_cache: boolean
      cache_path: string
    }>('/dict/import', force ? { force: true } : {})
    // 成功提示：注明来源（本地缓存 / 新下载），让用户明确知道是否走了缓存
    ElMessage.success(
      `中文字典导入完成（${r.from_cache ? '使用本地缓存' : '已下载最新版'}）：` +
        `匹配 ${r.matched} 条，更新 ${r.updated} 个标签` +
        (r.missing > 0 ? `（${r.missing} 个标签未入库）` : ''),
    )
    await loadDictStatus()
  } catch (e) {
    // 失败提示用右上角通知 —— 不阻塞操作，且保留足够久供阅读
    ElNotification({
      title: '中文字典导入失败',
      message: (e as Error).message,
      type: 'error',
      duration: 12000,
    })
    // 同时给出「可手动导入」的操作提示
    ElNotification({
      title: '可手动导入',
      message:
        '从 https://github.com/ffdkj/ffdkj-Danbooru_Tag-Chinese-English-Translation-Table 下载 tag.sqlite，' +
        `放入 ${dictStatus.value?.path ?? '应用数据目录/dict/tag.sqlite'} 后再次点击导入（会自动使用本地缓存）。`,
      type: 'info',
      duration: 20000,
    })
  } finally {
    dictImporting.value = false
  }
}

onMounted(loadDictStatus)

defineExpose({ reload: loadDictStatus })
</script>

<template>
  <el-form label-width="160px" style="max-width: 720px">
    <el-form-item label="中文字典">
      <el-switch
        v-model="settings.settings.cn_dict_enabled"
        active-text="开启"
        inactive-text="关闭"
      />
      <span class="hint">打标/显示时使用中英文对照</span>
    </el-form-item>
    <el-form-item label="优先中文标签">
      <el-switch
        v-model="settings.settings.tag_show_cn_first"
        active-text="开启"
        inactive-text="关闭"
      />
      <span class="hint">开启后显示为 女孩(1girl)，关闭为 1girl(女孩)</span>
    </el-form-item>
    <el-form-item label="中文字典导入">
      <el-button :loading="dictImporting" @click="importCnDict(false)">导入中文字典</el-button>
      <el-button :loading="dictImporting" plain @click="importCnDict(true)">强制重新下载</el-button>
      <span class="hint">
        从 ffdfkj 的 Danbooru 中英对照表（tag.sqlite，317K+ 条）回填中文别名，仅填空缺
      </span>
    </el-form-item>
    <!-- 缓存状态（上游日期一致时直接复用本地文件，跳过下载） -->
    <el-form-item label="字典缓存">
      <span class="hint">{{ dictStatusText }}</span>
    </el-form-item>
  </el-form>
</template>
