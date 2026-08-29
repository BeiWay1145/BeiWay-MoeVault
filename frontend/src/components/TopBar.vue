<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { Sunny, Moon, List, Plus, FolderOpened, Monitor } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { post } from '@/api/client'
import { reportLog } from '@/api/log'
import { fetchInferHealth, summarizeHealth, type InferOverall } from '@/api/infer'

const router = useRouter()

// 主题切换（暗黑/亮色，持久化 localStorage）
const isDark = ref(document.documentElement.classList.contains('dark'))
function toggleTheme() {
  isDark.value = !isDark.value
  document.documentElement.classList.toggle('dark', isDark.value)
  localStorage.setItem('moevault-theme', isDark.value ? 'dark' : 'light')
  reportLog(`切换主题为${isDark.value ? '暗黑' : '亮色'}模式`)
}

// ---- 导入（全局入口：路径输入 / 文件夹选择 / 拖拽） ----
const importVisible = ref(false)
const importPaths = ref('')
const importMode = ref<'move' | 'copy'>('move')
const submitting = ref(false)
const folderInput = ref<HTMLInputElement | null>(null)

/** 选择文件夹（webkitdirectory）填入路径提示（浏览器拿不到绝对路径）。 */
function onPickFolder(e: Event) {
  const input = e.target as HTMLInputElement
  const files = input.files
  if (!files || files.length === 0) return
  // 提示桌面壳用法（浏览器无法获取绝对路径）
  ElMessage.info('浏览器环境无法获取文件夹绝对路径，请在桌面壳（Tauri）中使用，或手动输入路径')
  input.value = ''
}

/** 全局拖拽导入：文件拖到窗口任意位置。 */
function onDropImport(e: DragEvent) {
  e.preventDefault()
  const files = e.dataTransfer?.files
  if (!files || files.length === 0) return
  ElMessage.info('已识别拖拽。浏览器环境无法获取文件路径，请使用桌面壳版本（自动支持拖拽）或手动输入路径')
}

function onDragOver(e: DragEvent) {
  e.preventDefault()
}

function onImportConfirm() {
  if (!importPaths.value.trim()) {
    ElMessage.warning('请输入文件/文件夹路径')
    return
  }
  const mode = importMode.value
  const modeLabel = mode === 'copy' ? '复制进库' : '移动进库'
  const paths = importPaths.value
    .split('\n')
    .map((s) => s.trim())
    .filter(Boolean)
  submitting.value = true
  post<{ batch_id: number }>('/import', { paths, mode })
    .then((res) => {
      ElMessage.success(`导入任务 #${res.batch_id} 已创建（${modeLabel}）`)
      reportLog(`提交导入任务 #${res.batch_id}（${paths.length} 个路径，${modeLabel}）`)
      importVisible.value = false
      importPaths.value = ''
      // 留在主目录看新分组（若已在主目录则刷新）
      if (router.currentRoute.value.name === 'imports') {
        window.dispatchEvent(new CustomEvent('moevault:import-done'))
      } else {
        router.push('/imports')
      }
    })
    .catch((e: Error) => ElMessage.error(e.message))
    .finally(() => {
      submitting.value = false
    })
}

// ---- 推理服务状态（圆点）：运行中绿 / 模型异常橙 / 未启动灰 ----
const inferState = ref<InferOverall>('stopped')
const inferTip = ref('推理服务：检测中')
let inferTimer: number | undefined

async function refreshInferState() {
  try {
    const h = await fetchInferHealth()
    inferState.value = summarizeHealth(h)
    inferTip.value =
      inferState.value === 'running'
        ? '推理服务运行中'
        : '推理服务异常（模型加载失败），点击查看'
  } catch {
    inferState.value = 'stopped'
    inferTip.value = '推理服务未启动，点击查看'
  }
}

function startInferPolling() {
  stopInferPolling()
  refreshInferState()
  inferTimer = window.setInterval(refreshInferState, 5000)
}

function stopInferPolling() {
  if (inferTimer !== undefined) {
    window.clearInterval(inferTimer)
    inferTimer = undefined
  }
}

onMounted(startInferPolling)
onBeforeUnmount(stopInferPolling)
</script>

<template>
  <el-header class="top-bar" @dragover="onDragOver" @drop="onDropImport">
    <div class="left">
      <el-button type="primary" :icon="Plus" @click="importVisible = true">导入</el-button>
    </div>
    <div class="right">
      <!-- 推理服务状态圆点：点击跳转设置页「本地推理」 -->
      <el-tooltip :content="inferTip" placement="bottom">
        <el-button
          :icon="Monitor"
          text
          class="infer-dot-btn"
          @click="router.push('/settings?tab=inference')"
        >
          <span class="infer-dot" :class="`dot-${inferState}`" />
        </el-button>
      </el-tooltip>
      <el-button :icon="List" text @click="router.push('/tasks')" title="任务中心" />
      <el-button :icon="isDark ? Sunny : Moon" text @click="toggleTheme" :title="isDark ? '切换亮色模式' : '切换暗黑模式'" />
    </div>

    <el-dialog v-model="importVisible" title="导入图片" width="560px" @dragover="onDragOver" @drop="onDropImport">
      <el-form label-width="90px">
        <el-form-item label="路径">
          <el-input
            v-model="importPaths"
            type="textarea"
            :rows="4"
            placeholder="支持文件夹或文件，每行一个&#10;例如：D:/Pictures 或 D:/a/1.png"
          />
        </el-form-item>
        <el-form-item label="选择文件夹">
          <input
            ref="folderInput"
            type="file"
            webkitdirectory
            multiple
            style="display: none"
            @change="onPickFolder"
          />
          <el-button :icon="FolderOpened" @click="folderInput?.click()">浏览文件夹…</el-button>
          <span class="hint">桌面壳版本支持直接拖拽文件夹/图片到窗口</span>
        </el-form-item>
        <el-form-item label="导入方式">
          <el-radio-group v-model="importMode">
            <el-radio value="move">移动进库（源位置清空）</el-radio>
            <el-radio value="copy">复制进库（保留源文件）</el-radio>
          </el-radio-group>
        </el-form-item>
        <el-alert type="warning" :closable="false" show-icon
          title="移动进库后原位置文件将被移走，请确认路径无误" />
      </el-form>
      <template #footer>
        <el-button @click="importVisible = false">取消</el-button>
        <el-button type="primary" :loading="submitting" @click="onImportConfirm">开始导入</el-button>
      </template>
    </el-dialog>
  </el-header>
</template>

<style scoped>
.top-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: var(--el-bg-color);
  border-bottom: 1px solid var(--el-border-color-light);
  height: 56px;
}
.left,
.right {
  display: flex;
  align-items: center;
  gap: 4px;
}
.hint {
  margin-left: 8px;
  color: var(--el-text-color-secondary);
  font-size: 12px;
}
.infer-dot-btn {
  padding: 0 10px;
}
.infer-dot {
  display: inline-block;
  width: 9px;
  height: 9px;
  border-radius: 50%;
  background: var(--el-text-color-placeholder);
  box-shadow: 0 0 0 2px var(--el-bg-color), 0 0 3px rgba(0, 0, 0, 0.2);
}
.dot-running {
  background: var(--el-color-success);
}
.dot-degraded {
  background: var(--el-color-warning);
}
.dot-stopped {
  background: var(--el-text-color-placeholder);
}
</style>
