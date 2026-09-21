<script setup lang="ts">
/**
 * 全局拖入导入（仅桌面壳）：
 * - 通过 Tauri 原生 drag-drop 事件拿到真实文件/文件夹路径（HTML5 DnD 在壳内被禁用且拿不到路径）
 * - 拖入时显示全屏覆盖提示；松开后弹确认框（路径预览 + 移动/复制选择）→ POST /api/v1/import
 * - 浏览器环境（isTauri()=false）不监听：沿用 TopBar 的手动输入/提示路径
 */
import { onMounted, onUnmounted, ref, watch } from 'vue'
import { useSettingsStore } from '@/stores/settings'
import { useTaskStore } from '@/stores/tasks'
import { useRouter } from 'vue-router'
import { FolderOpened, Document, UploadFilled } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { get, post } from '@/api/client'
import { reportLog } from '@/api/log'
import { isTauri } from '@/api/infer'

// ---- Tauri DragDropEvent（与 @tauri-apps/api/webview 的内部类型一致） ----
interface DragDropEvent {
  type: 'enter' | 'over' | 'drop' | 'leave'
  paths?: string[]
}
/** onDragDropEvent 回调的包装事件（Event<T>.payload 为实际载荷）。 */
interface DragDropEventWrapper {
  payload: DragDropEvent
}

const router = useRouter()

const dragActive = ref(false)
const dialogVisible = ref(false)
const paths = ref<string[]>([])
// 功能增强2：默认导入方式取自通用设置（确认框内可临时切换）
const settingsStore = useSettingsStore()
const taskStore = useTaskStore()
const mode = ref<'move' | 'copy'>(settingsStore.settings.import_default_mode ?? 'move')
const submitting = ref(false)
// 增强2：导入完成后自动提交的批量任务（对本次导入的新图生效）
const postTasks = ref<string[]>([])
/** 保存待跟进的任务配置：导入完成事件到达后据此提交。 */
let pendingFollowUp: { batchId: number; tasks: string[]; baselineId: number } | null = null

/** 打开确认框时刷新默认导入方式。 */
watch(dialogVisible, (v) => {
  if (v) {
    mode.value = settingsStore.settings.import_default_mode ?? 'move'
  }
})

const dirCount = () => paths.value.filter((p) => !p.includes('.')).length
const fileCount = () => paths.value.length - dirCount()

/** 是否像文件路径（带扩展名）。仅用于图标展示，后端会做真实判定。 */
function looksLikeFile(p: string) {
  return /\.[A-Za-z0-9]{1,8}$/.test(p)
}

async function onDragDropEvent(e: DragDropEventWrapper) {
  const ev = e.payload
  switch (ev.type) {
    case 'enter':
    case 'over':
      dragActive.value = true
      break
    case 'leave':
      dragActive.value = false
      break
    case 'drop': {
      dragActive.value = false
      const dropped = (ev.paths ?? []).filter(Boolean)
      if (dropped.length === 0) return
      if (dialogVisible.value) {
        // 确认框已打开：忽略新的拖入，避免覆盖用户正在编辑的选择
        return
      }
      paths.value = dropped
      // 尊重通用设置里的默认导入方式（此前硬编码 move，忽略了用户设置）
      mode.value = settingsStore.settings.import_default_mode ?? 'move'
      dialogVisible.value = true
      break
    }
  }
}

let unlisten: (() => void) | null = null
let unlistenNative: (() => void) | null = null

/**
 * 壳层原生拖放事件（`moevault://drag-drop`）的回调。
 *
 * 背景：wry 0.55.1 在 WebView2 上注册 OLE IDropTarget 的条件过于苛刻，
 * 导致系统层面无接受拖放的目标（拖入显示 STOP 拒绝光标），
 * Tauri 官方的 onDragDropEvent 也就永远不会触发。
 * 壳层改用自己注册的 IDropTarget（见 src-tauri/src/drag_drop.rs），
 * 事件通过下面的通道转发；两条通道同时监听，谁生效都能工作。
 */
interface NativeDropEvent {
  type: 'enter' | 'over' | 'drop' | 'leave'
  paths?: string[]
  position?: [number, number]
}
function onNativeDropEvent(ev: NativeDropEvent) {
  onDragDropEvent({ payload: { type: ev.type, paths: ev.paths } })
}

const taskLabel = (t: string) => ({ tag: '打标', aesthetic: '美学评分', sauce: '溯源' })[t] ?? t

/** 增强2：导入完成后，对本次导入的图片自动提交所选任务。 */
async function onImportFinished(e: Event) {
  const cfg = pendingFollowUp
  if (!cfg) return
  const d = (e as CustomEvent).detail as { batch_id?: number }
  if (d?.batch_id !== cfg.batchId) return
  pendingFollowUp = null
  try {
    // 取本次新增的图片 id（id 大于导入前的最大 id；新图按 id 递增写入）
    const r = await get<{ items: { id: number }[] }>('/images?limit=50&sort=imported&order=desc')
    const ids = (r.items ?? []).map((i) => i.id).filter((id) => id > cfg.baselineId)
    if (ids.length === 0) {
      ElMessage.warning('导入完成，但没有可提交的图片（可能全部为重复图）')
      return
    }
    for (const task of cfg.tasks) {
      if (task === 'tag') await taskStore.enqueueTag(ids)
      else if (task === 'aesthetic') await taskStore.enqueueAesthetic(ids)
      else if (task === 'sauce') await taskStore.enqueueSauce(ids)
    }
    ElMessage.success(`已为导入的 ${ids.length} 张图提交：${cfg.tasks.map(taskLabel).join('、')}`)
  } catch (err) {
    ElMessage.error(`导入后自动提交任务失败：${(err as Error).message}`)
  }
}

onMounted(async () => {
  if (!isTauri()) return
  window.addEventListener('moevault:import-done', onImportFinished)
  // 通道 1：壳层原生 IDropTarget（Windows 上的主通道）
  try {
    const { listen } = await import('@tauri-apps/api/event')
    unlistenNative = await listen<NativeDropEvent>('moevault://drag-drop', (e) =>
      onNativeDropEvent(e.payload),
    )
  } catch (e) {
    console.warn('[DragImport] 原生拖放监听注册失败', e)
  }
  // 通道 2：Tauri 官方 drag-drop（其它平台 / 未来版本恢复时可用）
  try {
    const { getCurrentWebview } = await import('@tauri-apps/api/webview')
    unlisten = await getCurrentWebview().onDragDropEvent(onDragDropEvent)
  } catch (e) {
    // 拖放监听失败不应阻塞应用：降级为无拖入功能
    console.warn('[DragImport] 注册 webview 拖放监听失败', e)
  }
})

onUnmounted(() => {
  window.removeEventListener('moevault:import-done', onImportFinished)
  unlisten?.()
  unlisten = null
  unlistenNative?.()
  unlistenNative = null
})

/** 确认导入：与 TopBar 手动导入同一后端契约（POST /api/v1/import）。 */
async function onConfirm() {
  if (paths.value.length === 0) return
  const modeLabel = mode.value === 'copy' ? '复制进库' : '移动进库'
  submitting.value = true
  try {
    // 增强2：先记录当前最大图片 id，导入完成后据此圈定"本次新增的图"
    let baselineId = 0
    if (postTasks.value.length > 0) {
      try {
        const s = await get<{ items: { id: number }[] }>('/images?limit=1&sort=imported&order=desc')
        baselineId = s.items?.[0]?.id ?? 0
      } catch {
        baselineId = 0
      }
    }
    const res = await post<{ batch_id: number }>('/import', { paths: paths.value, mode: mode.value })
    // 记录导入后要自动提交的批量任务（等导入完成事件再对新图提交）
    pendingFollowUp =
      postTasks.value.length > 0 ? { batchId: res.batch_id, tasks: [...postTasks.value], baselineId } : null
    const followHint = postTasks.value.length > 0 ? `，导入完成后自动提交：${postTasks.value.map(taskLabel).join('、')}` : ''
    ElMessage.success(`导入任务 #${res.batch_id} 已创建（${modeLabel}，共 ${paths.value.length} 个路径${followHint}）`)
    reportLog(`拖入导入：提交任务 #${res.batch_id}（${paths.value.length} 个路径，${modeLabel}${followHint}）`)
    dialogVisible.value = false
    paths.value = []
    if (router.currentRoute.value.name === 'imports') {
      window.dispatchEvent(new CustomEvent('moevault:import-done'))
    } else {
      router.push('/imports')
    }
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <!-- 拖入覆盖层：pointer-events:none，纯视觉提示，不拦截任何交互 -->
  <transition name="drag-fade">
    <div v-if="dragActive && !dialogVisible" class="drag-overlay">
      <div class="drag-hint">
        <el-icon :size="52" class="drag-icon"><UploadFilled /></el-icon>
        <div class="drag-text">松开以导入图片</div>
        <div class="drag-sub">支持文件 / 多个文件 / 文件夹 / 多个文件夹</div>
      </div>
    </div>
  </transition>

  <!-- 拖入确认框 -->
  <el-dialog
    v-model="dialogVisible"
    title="拖入导入"
    width="620px"
    :close-on-click-modal="false"
    append-to-body
  >
    <div class="path-summary">
      共 {{ paths.length }} 项：文件夹 {{ dirCount() }} 个，文件 {{ fileCount() }} 个
    </div>
    <div class="path-list">
      <div v-for="p in paths" :key="p" class="path-row">
        <el-icon class="path-icon">
          <FolderOpened v-if="!looksLikeFile(p)" />
          <Document v-else />
        </el-icon>
        <span class="path-text" :title="p">{{ p }}</span>
      </div>
    </div>
    <el-form label-width="90px" style="margin-top: 14px">
      <el-form-item label="导入方式">
        <el-radio-group v-model="mode">
          <el-radio value="move">移动进库（源位置清空）</el-radio>
          <el-radio value="copy">复制进库（保留源文件）</el-radio>
        </el-radio-group>
      </el-form-item>
      <el-form-item label="导入后">
        <el-checkbox-group v-model="postTasks">
          <el-checkbox value="tag">自动打标</el-checkbox>
          <el-checkbox value="aesthetic">自动美学评分</el-checkbox>
          <el-checkbox value="sauce">自动溯源</el-checkbox>
        </el-checkbox-group>
        <div class="follow-hint">导入完成后，对本次导入的图片自动创建对应批量任务（不勾选则不提交）</div>
      </el-form-item>
      <el-alert
        v-if="mode === 'move'"
        type="warning"
        :closable="false"
        show-icon
        title="移动进库后原位置文件将被移走，请确认路径无误"
      />
    </el-form>
    <template #footer>
      <el-button @click="dialogVisible = false">取消</el-button>
      <el-button type="primary" :loading="submitting" @click="onConfirm">开始导入</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.drag-overlay {
  position: fixed;
  inset: 0;
  z-index: 3000;
  background: rgba(64, 158, 255, 0.14);
  display: flex;
  align-items: center;
  justify-content: center;
  pointer-events: none;
}
.drag-hint {
  border: 3px dashed var(--el-color-primary);
  border-radius: 16px;
  padding: 36px 64px;
  background: rgba(255, 255, 255, 0.82);
  text-align: center;
  color: var(--el-color-primary);
}
:global(.dark) .drag-hint {
  background: rgba(30, 30, 30, 0.82);
}
.drag-text {
  font-size: 22px;
  font-weight: 700;
  margin-top: 10px;
}
.drag-sub {
  font-size: 13px;
  margin-top: 6px;
  opacity: 0.75;
}
.drag-fade-enter-active,
.drag-fade-leave-active {
  transition: opacity 0.15s ease;
}
.drag-fade-enter-from,
.drag-fade-leave-to {
  opacity: 0;
}

.follow-hint {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  line-height: 1.5;
}
.path-summary {
  font-size: 13px;
  color: var(--el-text-color-secondary);
  margin-bottom: 8px;
}
.path-list {
  max-height: 240px;
  overflow: auto;
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 8px;
  padding: 8px 10px;
}
.path-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 2px;
}
.path-icon {
  color: var(--el-color-primary);
  flex-shrink: 0;
}
.path-text {
  font-size: 13px;
  word-break: break-all;
  color: var(--el-text-color-primary);
}
</style>
