<script setup lang="ts">
/**
 * 批量导出二级界面（图库/主目录共用）：
 * 图片信息列表（网格/列表切换）+ 目标目录/包名 + 打包/tag/回收站 + 打包设置二级弹窗。
 */
import { computed, onMounted, ref, watch } from 'vue'
import { ElMessage, ElNotification } from 'element-plus'
import { get, post } from '@/api/client'
import { useSettingsStore } from '@/stores/settings'

const props = defineProps<{
  modelValue: boolean
  ids: number[]
  images: Array<{ id: number; name: string; thumb: string }>
}>()
const emit = defineEmits<{
  (e: 'update:modelValue', v: boolean): void
  (e: 'exported', info: { count: number; recycled: number }): void
}>()

const visible = computed({
  get: () => props.modelValue,
  set: (v: boolean) => emit('update:modelValue', v),
})

const exportViewMode = ref<'grid' | 'list'>('grid')
// 导出设置
const exportTargetDir = ref(localStorage.getItem('moevault-export-target') ?? '')
const exportBundleName = ref('')
const exportPack = ref(localStorage.getItem('moevault-export-pack') === '1')
const exportWithTags = ref(localStorage.getItem('moevault-export-tags') === '1')
const exportRecycle = ref(false)
// 打包设置（二级弹窗）
const packSettingsVisible = ref(false)
const exportPackFormat = ref<'zip' | '7z'>(localStorage.getItem('moevault-export-format') === '7z' ? '7z' : 'zip')
const exportPackLevel = ref(Number(localStorage.getItem('moevault-export-level') ?? '2'))
const exportRunning = ref(false)

watch(exportPack, (v) => localStorage.setItem('moevault-export-pack', v ? '1' : '0'))
watch(exportWithTags, (v) => localStorage.setItem('moevault-export-tags', v ? '1' : '0'))
watch(exportPackFormat, (v) => localStorage.setItem('moevault-export-format', v))
watch(exportPackLevel, (v) => localStorage.setItem('moevault-export-level', String(v)))
watch(exportTargetDir, (v) => localStorage.setItem('moevault-export-target', v))

/** 未设置过目标目录 → 默认取通用设置导出目录，否则探测下载目录。 */
onMounted(async () => {
  if (!exportTargetDir.value.trim()) {
    const s = useSettingsStore()
    if (s.settings.export_default_dir?.trim()) {
      exportTargetDir.value = s.settings.export_default_dir.trim()
    } else {
      try {
        const r = await get<{ dir: string }>('/system/downloads-dir')
        if (r.dir && !localStorage.getItem('moevault-export-target')) {
          exportTargetDir.value = r.dir
        }
      } catch {
        /* 静默 */
      }
    }
  }
})

/** 在资源管理器中打开/选中路径（导出后跳转）。 */
async function openExplorer(path: string) {
  try {
    await post('/system/open-explorer', { path })
  } catch {
    /* 静默 */
  }
}

/** 执行批量导出。 */
async function doExport() {
  if (props.ids.length === 0) return
  if (!exportTargetDir.value.trim()) {
    ElMessage.warning('请设置导出目标目录')
    return
  }
  if (exportPack.value && exportPackFormat.value === '7z') {
    ElMessage.warning('7z 打包暂不支持（本机未安装 7-Zip），请选择 ZIP')
    return
  }
  exportRunning.value = true
  try {
    const r = await post<{
      ok: boolean
      count: number
      failed: number
      tag_files: number
      target: string
      zip?: string
      recycled: number
    }>('/images/export', {
      ids: props.ids,
      target_dir: exportTargetDir.value.trim(),
      bundle_name: exportBundleName.value.trim() || null,
      pack: exportPack.value,
      pack_format: exportPackFormat.value,
      pack_level: exportPackLevel.value,
      with_tags: exportWithTags.value,
      recycle_after: exportRecycle.value,
    })
    const target = r.zip ?? r.target
    // 打开资源管理器：一律打开目录页（zip 时打开 zip 所在目录，散件打开导出目录）
    const browseDir = r.zip ? r.zip.replace(/[\\/][^\\/]*$/, '') : r.target
    // 功能增强4：成功后提示（可点击跳转）+ 设置开启时自动打开资源管理器
    ElNotification({
      title: '导出完成',
      message: `${r.count} 张${r.failed > 0 ? `，${r.failed} 张失败` : ''}${r.tag_files > 0 ? `，${r.tag_files} 个标签文件` : ''}（点击跳转目录）`,
      type: 'success',
      duration: 6000,
      onClick: () => openExplorer(browseDir),
    })
    const s = useSettingsStore()
    if (s.settings.export_open_explorer_batch) {
      openExplorer(browseDir)
    }
    visible.value = false
    emit('exported', { count: r.count, recycled: r.recycled })
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    exportRunning.value = false
  }
}
</script>

<template>
  <el-dialog v-model="visible" title="批量导出" width="720px" append-to-body>
    <div class="export-setup">
      <div class="export-row">
        <span class="export-label">导出目标目录</span>
        <el-input v-model="exportTargetDir" placeholder="例如 D:\Downloads\训练集存放处" style="flex: 1" />
      </div>
      <div class="export-row">
        <span class="export-label">文件夹/包名</span>
        <el-input v-model="exportBundleName" placeholder="留空：不建子文件夹（散件）/ 默认 export（打包）" style="flex: 1" />
      </div>
      <div class="export-row">
        <el-checkbox v-model="exportPack">打包导出</el-checkbox>
        <el-checkbox v-model="exportWithTags">导出 tag 信息（同名 .txt）</el-checkbox>
        <el-checkbox v-model="exportRecycle">导出后从库中删除（进回收站）</el-checkbox>
      </div>
      <div class="export-row">
        <el-button size="small" :disabled="!exportPack" @click="packSettingsVisible = true">打包文件设置</el-button>
        <span class="export-hint">打包开启后可设置压缩格式与级别（当前：{{ exportPackFormat.toUpperCase() }} / 级别 {{ exportPackLevel }}）</span>
      </div>
    </div>
    <div class="export-view-toolbar">
      <el-radio-group v-model="exportViewMode" size="small">
        <el-radio-button value="grid">网格</el-radio-button>
        <el-radio-button value="list">列表</el-radio-button>
      </el-radio-group>
      <span class="export-count">共 {{ images.length }} 张</span>
    </div>
    <div class="export-images">
      <div v-if="exportViewMode === 'grid'" class="export-grid">
        <div v-for="img in images" :key="img.id" class="export-card">
          <el-image :src="img.thumb" fit="cover" class="export-thumb" />
          <div class="export-name">{{ img.name }}</div>
        </div>
      </div>
      <div v-else class="export-list">
        <div v-for="img in images" :key="img.id" class="export-list-row">
          <el-image :src="img.thumb" fit="cover" class="export-list-thumb" />
          <span class="export-list-name">{{ img.name }}</span>
        </div>
      </div>
    </div>
    <template #footer>
      <el-button @click="visible = false">取消</el-button>
      <el-button type="primary" :loading="exportRunning" @click="doExport">开始导出</el-button>
    </template>
  </el-dialog>

  <!-- 打包文件设置（二级弹窗） -->
  <el-dialog v-model="packSettingsVisible" title="打包文件设置" width="420px" append-to-body>
    <div class="export-row">
      <span class="export-label">压缩格式</span>
      <el-radio-group v-model="exportPackFormat">
        <el-radio-button value="zip">ZIP</el-radio-button>
        <el-radio-button value="7z" disabled>7Z（未安装）</el-radio-button>
      </el-radio-group>
    </div>
    <div class="export-row">
      <span class="export-label">压缩级别</span>
      <el-select v-model="exportPackLevel" style="width: 180px">
        <el-option :value="0" label="0 - 仅存储（最快）" />
        <el-option :value="1" label="1 - 快速压缩" />
        <el-option :value="2" label="2 - 正常压缩" />
        <el-option :value="3" label="3 - 极限压缩" />
      </el-select>
    </div>
    <template #footer>
      <el-button @click="packSettingsVisible = false">完成</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.export-setup {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-bottom: 12px;
}
.export-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.export-label {
  width: 100px;
  flex: none;
  font-size: 13px;
  color: var(--el-text-color-regular);
}
.export-hint {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.export-view-toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 8px;
}
.export-count {
  font-size: 13px;
  color: var(--el-text-color-secondary);
}
.export-images {
  max-height: 320px;
  overflow-y: auto;
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 6px;
  padding: 8px;
}
.export-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(96px, 1fr));
  gap: 8px;
}
.export-card {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.export-thumb {
  width: 100%;
  aspect-ratio: 1;
  border-radius: 4px;
}
.export-name {
  font-size: 11px;
  color: var(--el-text-color-secondary);
  word-break: break-all;
  line-height: 1.3;
}
.export-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.export-list-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px;
  border-radius: 4px;
}
.export-list-row:hover {
  background: var(--el-fill-color-light);
}
.export-list-thumb {
  width: 40px;
  height: 40px;
  border-radius: 4px;
  flex: none;
}
.export-list-name {
  font-size: 13px;
  word-break: break-all;
}
</style>
