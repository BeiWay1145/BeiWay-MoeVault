<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter, onBeforeRouteLeave } from 'vue-router'
import { Delete, CaretRight, VideoPause, Refresh, Download } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { get, post, del, put } from '@/api/client'
import { useSettingsStore, type SauceKeyConfig } from '@/stores/settings'
import { reportLog } from '@/api/log'
import {
  fetchInferHealth,
  inferInstallDeps,
  inferShellStatus,
  inferStart,
  inferStop,
  isTauri,
  summarizeHealth,
  type InferHealth,
  type InferModelState,
  type InferOverall,
} from '@/api/infer'

const settings = useSettingsStore()
const route = useRoute()
// 默认打开「通用」设置页；支持 ?tab=inference 直达本地推理状态
const activeTab = ref('library')
if (route.query.tab === 'inference') {
  activeTab.value = 'inference'
}

// ---- SauceNAO 多 key ----
const newKey = ref('')
const newKeyName = ref('')
const newKeyTier = ref('free')
const keys = ref<SauceKeyConfig[] & Array<Record<string, unknown>>>([])
const manageVisible = ref(false)
const saving = ref(false)

// ---- 打标模型：模型种类 + 自动探测（推荐）/ 自定义目录 ----
// 模型种类：auto=按目录内容自动判定；cl_tagger=SIGLIP2 ONNX；wd14=wd14 tagger ONNX
const taggerKindOptions = [
  { value: 'auto', label: '自动探测（推荐）' },
  { value: 'cl_tagger', label: 'cl-tagger (SIGLIP2 ONNX)' },
  { value: 'wd14', label: 'WD14 Tagger (ONNX)' },
]

const kindLabel = (k?: string) =>
  taggerKindOptions.find((o) => o.value === (k || 'auto'))?.label ?? '自动探测'

// 自动探测顺序：项目内 models/tagger → 旧位置 D:/Game/AI/cl_tagger/models → 自定义目录
const taggerModelOptions = [
  { name: '自动探测（推荐）', dir: '' },
  { name: '自定义目录', dir: '__custom__' },
]

function onKindSelect(value: string) {
  settings.settings.tagger_model_kind = value
  settings.settings.tagger_model_name = kindLabel(value)
  ElMessage.info(`模型种类已切换为「${kindLabel(value)}」，保存设置后重跑打标任务生效`)
}

// ---- 推理服务状态卡片 ----
const inferHealth = ref<InferHealth | null>(null)
const inferOverall = ref<InferOverall>('stopped')
const inferLoading = ref(false)
const inferBusy = ref(false)

async function loadInferHealth() {
  inferLoading.value = true
  try {
    const h = await fetchInferHealth()
    inferHealth.value = h
    inferOverall.value = summarizeHealth(h)
  } catch {
    inferHealth.value = null
    inferOverall.value = 'stopped'
  } finally {
    inferLoading.value = false
  }
}

async function onInferStart() {
  inferBusy.value = true
  try {
    const msg = await inferStart()
    ElMessage.success(msg)
    // 稍等片刻再刷新（服务启动需要时间）
    setTimeout(loadInferHealth, 1500)
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    inferBusy.value = false
  }
}

async function onInferStop() {
  inferBusy.value = true
  try {
    await inferStop()
    ElMessage.success('推理服务已停止')
    inferHealth.value = null
    inferOverall.value = 'stopped'
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    inferBusy.value = false
  }
}

const inferStatusText = () =>
  ({
    running: '运行中',
    degraded: '异常（模型加载失败）',
    stopped: '未启动',
  })[inferOverall.value]

const inferStatusType = () =>
  ({ running: 'success', degraded: 'warning', stopped: 'info' })[inferOverall.value] as
    | 'success'
    | 'warning'
    | 'info'

/** 单个模型的加载状态文案。 */
const modelStateText = (m?: InferModelState) =>
  !m
    ? '未知'
    : m.state === 'ok'
      ? '已加载'
      : m.state === 'failed'
        ? `加载失败：${m.error ?? ''}`
        : '未加载（首次调用时加载）'

// ---- 桌面壳诊断：依赖缺失检测 + 一键安装（仅桌面版） ----
const shellDepsMissing = ref<string[]>([])
const installingDeps = ref(false)

async function loadShellDiagnostics() {
  if (!isTauri()) return
  try {
    const s = await inferShellStatus()
    shellDepsMissing.value = s?.deps_missing ?? []
  } catch {
    shellDepsMissing.value = []
  }
}

async function onInstallDeps() {
  installingDeps.value = true
  try {
    const msg = await inferInstallDeps()
    ElMessage.success(msg)
    shellDepsMissing.value = []
    // 安装完成后自动尝试启动
    await onInferStart()
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    installingDeps.value = false
  }
}

async function loadKeys() {
  try {
    // 现在 list_keys 返回实时配额（short/long/cooldown/status）
    const d = await get<{ keys: SauceKeyConfig[] & Array<Record<string, unknown>>; count: number }>(
      '/settings/saucenao-keys',
    )
    keys.value = d.keys
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** E7: 手动修改某 key 的当日额度。 */
async function editQuota(name: string) {
  const k = keys.value.find((x) => x.name === name) as (SauceKeyConfig & Record<string, unknown>) | undefined
  const cur = Number(k?.long_remaining ?? 95)
  const { value } = await ElMessageBox.prompt('设置当日剩余额度', `修改额度 · ${name}`, {
    inputValue: String(cur),
    inputPattern: /^\d+$/,
    inputErrorMessage: '请输入数字',
  }).catch(() => ({ value: null as string | null }))
  if (value === null) return
  try {
    await put(`/settings/saucenao-keys/${encodeURIComponent(name)}/quota`, { long_remaining: Number(value) })
    ElMessage.success('额度已更新')
    await loadKeys()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** E7: 安全删除密钥（红垃圾桶，需确认）。 */
async function removeKey(name: string) {
  try {
    await ElMessageBox.confirm(`确定删除密钥「${name}」？删除后无法恢复。`, '删除密钥', {
      type: 'warning',
      confirmButtonText: '删除',
      cancelButtonText: '取消',
    })
  } catch {
    return
  }
  try {
    await del(`/settings/saucenao-keys/${encodeURIComponent(name)}`)
    ElMessage.success(`已删除 ${name}`)
    await loadKeys()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function loadKeyStatuses() {
  // 额度已合并进 /settings/saucenao-keys（list_keys 实时返回），此函数保留兼容空实现
}

async function addKey() {
  if (!newKey.value.trim()) {
    ElMessage.warning('请输入 API key')
    return
  }
  try {
    const r = await post<{ ok: boolean; name: string }>('/settings/saucenao-keys', {
      key: newKey.value.trim(),
      name: newKeyName.value.trim() || undefined,
      tier: newKeyTier.value,
    })
    ElMessage.success(`已添加密钥 ${r.name}`)
    newKey.value = ''
    newKeyName.value = ''
    await loadKeys()
    await loadKeyStatuses()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function openManage() {
  manageVisible.value = true
  await loadKeys()
}

async function saveSettings() {
  saving.value = true
  try {
    await settings.save()
    settingsDirty.value = false
    reportLog('用户修改并保存了设置')
    ElMessage.success('设置已保存')
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    saving.value = false
  }
}

// ---- 增强2：未保存设置离开提醒 ----
const settingsDirty = ref(false)
/** 追踪设置快照，检测是否有未保存修改。 */
let settingsSnapshot = ''
function snapshotSettings(): string {
  return JSON.stringify(settings.settings)
}
watch(
  () => settings.settings,
  () => {
    if (settingsSnapshot === '') settingsSnapshot = snapshotSettings()
    else settingsDirty.value = snapshotSettings() !== settingsSnapshot
  },
  { deep: true },
)
/** 保存后重置快照。 */
watch(
  () => settingsDirty.value,
  (d) => {
    if (!d && settingsSnapshot !== '') settingsSnapshot = snapshotSettings()
  },
)

/** 离开设置页确认：未保存时三选（保存/放弃/取消）。 */
onBeforeRouteLeave(async () => {
  if (!settingsDirty.value) return true
  const action = await ElMessageBox.confirm(
    '设置尚未保存，离开将丢失修改。',
    '未保存的设置',
    {
      confirmButtonText: '保存并离开',
      cancelButtonText: '放弃修改',
      distinguishCancelAndClose: true,
      type: 'warning',
    },
  )
    .then(() => 'save' as const)
    .catch((action: string | 'cancel' | 'close') => (action === 'cancel' ? ('discard' as const) : ('stay' as const)))
  if (action === 'save') {
    await saveSettings()
    return true
  }
  if (action === 'discard') return true
  return false
})

/** 增强1：导入中文字典（下载 ffdfkj tag.sqlite → 回填 name_cn，仅填空缺）。 */
const dictImporting = ref(false)
async function importCnDict() {
  dictImporting.value = true
  try {
    const r = await post<{ matched: number; updated: number; missing: number }>('/dict/import')
    ElMessage.success(
      `中文字典导入完成：匹配 ${r.matched} 条，更新 ${r.updated} 个标签${r.missing > 0 ? `（${r.missing} 个标签未入库）` : ''}`,
    )
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    dictImporting.value = false
  }
}

// ---- 日志面板（设置页「日志」tab） ----
interface LogEntry {
  id: number
  level: string
  category: string
  message: string
  created_at: number
}
const logs = ref<LogEntry[]>([])
const logLoading = ref(false)
const logListRef = ref<HTMLElement | null>(null)

// 日志显示设置（localStorage 持久化）：自动刷新频率（秒，0=关闭）+ 自动滚动
const LOG_SETTINGS_KEY = 'moevault-log-settings'
const logAutoRefresh = ref(5)
const logAutoScroll = ref(true)
const logRefreshOptions = [
  { value: 0, label: '关闭' },
  { value: 5, label: '5 秒' },
  { value: 10, label: '10 秒' },
  { value: 30, label: '30 秒' },
  { value: 60, label: '60 秒' },
]
let logTimer: number | undefined

function loadLogSettings() {
  try {
    const raw = localStorage.getItem(LOG_SETTINGS_KEY)
    if (!raw) return
    const s = JSON.parse(raw) as { refresh?: number; scroll?: boolean }
    if (typeof s.refresh === 'number' && logRefreshOptions.some((o) => o.value === s.refresh)) {
      logAutoRefresh.value = s.refresh
    }
    if (typeof s.scroll === 'boolean') logAutoScroll.value = s.scroll
  } catch {
    /* 解析失败用默认值 */
  }
}
function saveLogSettings() {
  try {
    localStorage.setItem(
      LOG_SETTINGS_KEY,
      JSON.stringify({ refresh: logAutoRefresh.value, scroll: logAutoScroll.value }),
    )
  } catch {
    /* 忽略 */
  }
}
function stopLogTimer() {
  if (logTimer !== undefined) {
    window.clearInterval(logTimer)
    logTimer = undefined
  }
}
function startLogTimer() {
  stopLogTimer()
  if (logAutoRefresh.value > 0) {
    logTimer = window.setInterval(() => {
      loadLogs()
    }, logAutoRefresh.value * 1000)
  }
}

/** 判断日志滚动容器是否在底部（30px 容差）。 */
function isAtBottom(el: HTMLElement) {
  return el.scrollHeight - el.scrollTop - el.clientHeight < 30
}

async function loadLogs() {
  const el = logListRef.value
  const wasAtBottom = el ? isAtBottom(el) : false
  logLoading.value = true
  try {
    const d = await get<{ items: LogEntry[] }>('/logs?limit=200')
    // 最新在下（后端返回倒序，反转显示）
    logs.value = [...d.items].reverse()
    await nextTick()
    // 自动滚动：仅在用户本就在底部时跟随到最新日志
    if (logAutoScroll.value && wasAtBottom && logListRef.value) {
      logListRef.value.scrollTop = logListRef.value.scrollHeight
    }
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    logLoading.value = false
  }
}

// 增强1：BUG追踪器——追踪开关（localStorage）+ 后端转储
const bugTrackerEnabled = ref(localStorage.getItem('moevault-bug-tracker') === '1')
watch(bugTrackerEnabled, (v) => {
  localStorage.setItem('moevault-bug-tracker', v ? '1' : '0')
  reportLog(v ? 'BUG追踪器已开启' : 'BUG追踪器已关闭')
})
const dumpLoading = ref(false)
async function dumpLogsBackend() {
  dumpLoading.value = true
  try {
    const r = await get<{ path: string; count: number }>('/logs/export')
    ElMessage.success(`已转储 ${r.count} 条日志：${r.path}`)
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    dumpLoading.value = false
  }
}

// 日志设置变化 → 持久化 + 重启定时器；切到日志 tab → 立即刷新 + 启动定时器
watch([logAutoRefresh, logAutoScroll], () => {
  saveLogSettings()
  if (activeTab.value === 'logs') startLogTimer()
})
watch(activeTab, (tab) => {
  if (tab === 'logs') {
    loadLogs()
    startLogTimer()
  } else {
    stopLogTimer()
  }
})
onBeforeUnmount(stopLogTimer)

async function clearLogs() {
  try {
    await ElMessageBox.confirm('清空全部日志？', '清空日志', { type: 'warning' })
  } catch {
    return
  }
  try {
    const r = await del<{ cleared: number }>('/logs')
    ElMessage.success(`已清空 ${r.cleared} 条日志`)
    await loadLogs()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

function exportLogs() {
  if (logs.value.length === 0) {
    ElMessage.warning('当前没有日志可导出')
    return
  }
  const lines = logs.value.map(
    (l) =>
      `[${new Date(l.created_at * 1000).toLocaleString()}] [${l.level}] [${l.category}] ${l.message}`,
  )
  const blob = new Blob([lines.join('\n')], { type: 'text/plain;charset=utf-8' })
  const a = document.createElement('a')
  a.href = URL.createObjectURL(blob)
  a.download = `moevault-logs-${Date.now()}.txt`
  document.body.appendChild(a)
  a.click()
  // 延迟释放 URL，确保浏览器完成下载
  setTimeout(() => {
    URL.revokeObjectURL(a.href)
    a.remove()
  }, 500)
  ElMessage.success(`已导出 ${lines.length} 条日志`)
}

const logLevelType = (l: string) =>
  ({ info: 'info', warn: 'warning', error: 'danger' })[l] as 'info' | 'warning' | 'danger'
const logCategoryLabel = (c: string) =>
  ({ task: '任务', sauce: '溯源', tag: '打标', aesthetic: '美学', frontend: '前端', import: '导入', system: '系统' })[c] ?? c

function onModelSelect(name: string) {
  const opt = taggerModelOptions.find((o) => o.name === name)
  if (!opt) return
  if (opt.dir === '') {
    // 自动探测：清空自定义目录
    settings.settings.tagger_model_dir = ''
  } else if (opt.dir === '__custom__') {
    // 自定义目录：保留用户当前输入（若为空，提示先输入）
    if (settings.settings.tagger_model_dir.trim()) {
      ElMessage.info('已切换到自定义目录，请保存设置后生效')
    } else {
      ElMessage.info('请输入自定义模型目录，或保持自动探测')
    }
  }
}

// ---- 推理设备（打标/美学）----
interface DeviceOption {
  id: string
  name: string
  kind: string
}
const taggerDevices = ref<DeviceOption[]>([])
const aestheticDevices = ref<DeviceOption[]>([])
async function loadDevices() {
  try {
    const d = await get<{ devices: DeviceOption[] }>('/devices')
    // 打标设备：onnxruntime 的 cuda/cpu；美学设备：torch 的 cuda:/cpu
    const all = d.devices
    taggerDevices.value = [
      { id: 'auto', name: '自动', kind: 'tagger' },
      ...all.filter((x) => x.kind === 'tagger'),
    ]
    aestheticDevices.value = [
      { id: 'auto', name: '自动', kind: 'aesthetic' },
      ...all.filter((x) => x.kind === 'aesthetic'),
    ]
    // 若后端返回的只有一种 kind（如只 tagger），美学兜底 auto/cpu
    if (aestheticDevices.value.length === 1) {
      aestheticDevices.value.push({ id: 'cpu', name: 'CPU', kind: 'aesthetic' })
    }
    if (taggerDevices.value.length === 1) {
      taggerDevices.value.push({ id: 'cpu', name: 'CPU', kind: 'tagger' })
    }
  } catch {
    // 推理服务未启动：给默认选项
    taggerDevices.value = [
      { id: 'auto', name: '自动', kind: 'tagger' },
      { id: 'cpu', name: 'CPU', kind: 'tagger' },
    ]
    aestheticDevices.value = [
      { id: 'auto', name: '自动', kind: 'aesthetic' },
      { id: 'cpu', name: 'CPU', kind: 'aesthetic' },
    ]
  }
}

onMounted(async () => {
  await settings.load()
  await loadKeys()
  await loadDevices()
  loadLogSettings()
  loadInferHealth()
  loadShellDiagnostics()
})
</script>

<template>
  <div class="settings-page">
    <el-tabs v-model="activeTab">
      <el-tab-pane label="通用" name="library">
        <el-form label-width="160px" style="max-width: 720px">
          <el-form-item label="库目录">
            <el-input v-model="settings.settings.library_dir" placeholder="data/library" />
          </el-form-item>
          <el-form-item label="关闭时最小化到托盘">
            <el-switch v-model="settings.settings.close_to_tray" active-text="开启" inactive-text="关闭" />
            <span class="hint">开启后点关闭按钮最小化到系统托盘（后台任务继续），托盘图标可恢复/退出</span>
          </el-form-item>
          <el-form-item label="瀑布流列数">
            <el-select v-model="settings.settings.waterfall_columns" style="width: 200px">
              <el-option label="自动（传统瀑布流）" value="auto" />
              <el-option label="2 列" value="2" />
              <el-option label="3 列" value="3" />
              <el-option label="4 列" value="4" />
              <el-option label="5 列" value="5" />
              <el-option label="6 列" value="6" />
            </el-select>
            <span class="hint">自动=传统瀑布流（紧密错落、按列填充）；固定列=网格按行排（无空隙）。影响图库/搜索</span>
          </el-form-item>
          <el-form-item label="侧边栏悬停展开">
            <el-switch v-model="settings.settings.sidebar_hover_expand" active-text="开启" inactive-text="关闭" />
            <span class="hint">侧边栏收起后，鼠标悬停自动展开（默认开启）</span>
          </el-form-item>
          <el-form-item label="分页模式">
            <el-switch v-model="settings.settings.pagination_enabled" active-text="开启" inactive-text="关闭" />
            <span class="hint">开启后图库按每页固定条数分页（每页条数在图库页右下角设置），关闭则一次加载全部</span>
          </el-form-item>
          <el-form-item label="预加载图片张数">
            <el-input-number v-model="settings.settings.preload_count" :min="0" :max="5" />
            <span class="hint">详情页切换图片时预加载前后各 N 张原图（0=关闭，默认 4），减少切换闪灰</span>
          </el-form-item>
        </el-form>
      </el-tab-pane>

      <el-tab-pane label="SauceNAO" name="saucenao">
        <el-form label-width="160px" style="max-width: 720px">
          <el-form-item label="API key">
            <el-input v-model="newKey" type="password" show-password placeholder="输入 SauceNAO API key" style="width: 300px" />
            <el-input v-model="newKeyName" placeholder="密钥名称（默认 Key0/1/2...）" style="width: 160px; margin-left: 8px" />
            <el-select v-model="newKeyTier" style="width: 110px; margin-left: 8px">
              <el-option label="免费" value="free" />
              <el-option label="付费" value="member" />
            </el-select>
            <el-button type="primary" style="margin-left: 8px" @click="addKey">添加</el-button>
          </el-form-item>
          <el-form-item label="相似度阈值">
            <el-slider v-model="settings.settings.saucenao_min_sim" :min="0" :max="100" show-input style="width: 260px" /> %
          </el-form-item>
          <el-form-item label="已配置密钥">
            <el-tag v-for="k in keys" :key="k.name" closable class="key-tag" @close="removeKey(k.name)">
              {{ k.name }}（{{ k.tier === 'member' ? '付费' : '免费' }}）
            </el-tag>
            <el-tag v-if="keys.length === 0" type="info">未配置密钥</el-tag>
          </el-form-item>
          <el-form-item>
            <el-button @click="openManage">管理密钥（查看额度）</el-button>
          </el-form-item>
        </el-form>
      </el-tab-pane>

      <el-tab-pane label="本地推理" name="inference">
        <!-- 推理服务状态卡片：服务健康 + 当前模型路径 + 启动/停止（桌面版） -->
        <el-card header="推理服务" shadow="never" class="inf-card">
          <div class="infer-status-row">
            <el-tag :type="inferStatusType()">{{ inferStatusText() }}</el-tag>
            <el-button size="small" :icon="Refresh" :loading="inferLoading" @click="loadInferHealth">刷新</el-button>
            <template v-if="isTauri()">
              <el-button size="small" type="primary" plain :icon="CaretRight" :loading="inferBusy" :disabled="inferOverall === 'running'" @click="onInferStart">启动服务</el-button>
              <el-button size="small" type="danger" plain :icon="VideoPause" :loading="inferBusy" :disabled="inferOverall === 'stopped'" @click="onInferStop">停止服务</el-button>
            </template>
            <span v-else class="hint">浏览器模式仅展示状态；启动请运行 python/run_server.bat</span>
          </div>
          <!-- 依赖缺失：显示原因 + 一键安装（仅桌面版） -->
          <el-alert
            v-if="isTauri() && shellDepsMissing.length > 0"
            type="warning"
            :closable="false"
            show-icon
            class="inf-alert"
          >
            <template #title>
              推理服务依赖缺失：{{ shellDepsMissing.join('、') }}
              <el-button
                size="small"
                type="warning"
                :icon="Download"
                :loading="installingDeps"
                style="margin-left: 8px"
                @click="onInstallDeps"
              >
                一键安装依赖
              </el-button>
            </template>
            <span>将安装 fastapi / uvicorn / transformers（纯 CPU 包），完成后自动尝试启动服务</span>
          </el-alert>
          <el-descriptions :column="1" size="small" border class="infer-desc">
            <el-descriptions-item label="打标模型种类">
              <span>{{ inferHealth?.models?.tagger?.kind ? kindLabel(inferHealth.models.tagger.kind) : '（服务未启动）' }}</span>
            </el-descriptions-item>
            <el-descriptions-item label="打标模型目录">
              <span class="mono">{{ inferHealth?.paths?.tagger_model_dir || '（服务未启动）' }}</span>
              <span v-if="inferHealth && !settings.settings.tagger_model_dir" class="hint">自动探测</span>
              <span v-else-if="settings.settings.tagger_model_dir" class="hint">自定义（设置保存后生效）</span>
            </el-descriptions-item>
            <el-descriptions-item label="打标模型状态">{{ modelStateText(inferHealth?.models?.tagger) }}</el-descriptions-item>
            <el-descriptions-item label="美学模型">
              <span class="mono">{{ inferHealth?.paths?.aesthetic_model || '（服务未启动）' }}</span>
            </el-descriptions-item>
            <el-descriptions-item label="美学模型状态">{{ modelStateText(inferHealth?.models?.aesthetic) }}</el-descriptions-item>
          </el-descriptions>
        </el-card>

        <el-card header="打标" shadow="never" class="inf-card">
          <el-form label-width="160px" style="max-width: 720px">
            <el-form-item label="模型种类">
              <el-select
                :model-value="settings.settings.tagger_model_kind || 'auto'"
                style="width: 260px"
                @change="onKindSelect"
              >
                <el-option v-for="o in taggerKindOptions" :key="o.value" :label="o.label" :value="o.value" />
              </el-select>
              <span class="hint">auto=按模型目录自动判定；cl-tagger 用 model_vocabulary.json，wd14 用 selected_tags.csv</span>
            </el-form-item>
            <el-form-item label="打标模型来源">
              <el-select
                :model-value="settings.settings.tagger_model_dir ? '自定义目录' : '自动探测（推荐）'"
                style="width: 260px"
                @change="onModelSelect"
              >
                <el-option v-for="o in taggerModelOptions" :key="o.name" :label="o.name" :value="o.name" />
              </el-select>
              <span class="hint">推荐自动探测：项目内 models/tagger → 旧位置 → 自定义</span>
            </el-form-item>
            <el-form-item label="自定义目录">
              <el-input v-model="settings.settings.tagger_model_dir" placeholder="留空 = 自动探测（推荐）" style="width: 400px" />
              <span class="hint">留空自动探测；填写后保存设置并重跑打标任务生效</span>
            </el-form-item>
            <el-form-item label="置信度阈值">
              <el-slider v-model="settings.settings.tag_threshold" :min="0" :max="1" :step="0.05" show-input style="width: 260px" />
            </el-form-item>
            <el-form-item label="推理设备">
              <el-select v-model="settings.settings.tagger_device" style="width: 260px">
                <el-option v-for="d in taggerDevices" :key="d.id" :label="d.name" :value="d.id" />
              </el-select>
              <span class="hint">GPU 加速打标（下次任务生效）</span>
            </el-form-item>
          </el-form>
        </el-card>
        <el-card header="美学评分" shadow="never" class="inf-card">
          <el-form label-width="160px" style="max-width: 720px">
            <el-form-item label="模型">
              <el-input v-model="settings.settings.aesthetic_model" />
            </el-form-item>
            <el-form-item label="推理设备">
              <el-select v-model="settings.settings.aesthetic_device" style="width: 260px">
                <el-option v-for="d in aestheticDevices" :key="d.id" :label="d.name" :value="d.id" />
              </el-select>
              <span class="hint">GPU 加速美学评分（下次任务生效）</span>
            </el-form-item>
          </el-form>
        </el-card>
        <!-- 改进2：查重设置（本地推理页末尾独立卡片） -->
        <el-card header="查重" shadow="never" class="inf-card">
          <el-form label-width="160px" style="max-width: 720px">
            <el-form-item label="pHash 汉明距离">
              <el-slider v-model="settings.settings.dedup_hamming" :min="0" :max="64" show-input style="width: 260px" />
              <span class="hint">查重分组阈值（默认 8）</span>
            </el-form-item>
          </el-form>
        </el-card>
      </el-tab-pane>

      <el-tab-pane label="回收站 / sidecar" name="misc">
        <el-form label-width="160px" style="max-width: 720px">
          <el-form-item label="自动清空天数">
            <el-input-number v-model="settings.settings.recycle_days" :min="0" :max="365" />
            <span class="hint">0 = 不自动清空</span>
          </el-form-item>
          <el-form-item label="sidecar .txt">
            <el-switch v-model="settings.settings.sidecar_enabled" active-text="开启" inactive-text="关闭" />
          </el-form-item>
        </el-form>
      </el-tab-pane>

      <!-- 改进1：标签设置页（字典相关设置从回收站页移出） -->
      <el-tab-pane label="标签" name="tags">
        <el-form label-width="160px" style="max-width: 720px">
          <el-form-item label="中文字典">
            <el-switch v-model="settings.settings.cn_dict_enabled" active-text="开启" inactive-text="关闭" />
            <span class="hint">打标/显示时使用中英文对照</span>
          </el-form-item>
          <el-form-item label="优先中文标签">
            <el-switch v-model="settings.settings.tag_show_cn_first" active-text="开启" inactive-text="关闭" />
            <span class="hint">开启后显示为 女孩(1girl)，关闭为 1girl(女孩)</span>
          </el-form-item>
          <el-form-item label="中文字典导入">
            <el-button :loading="dictImporting" @click="importCnDict">导入中文字典</el-button>
            <span class="hint">从 ffdfkj 的 Danbooru 中英对照表（tag.sqlite，317K+ 条）回填中文别名，仅填空缺</span>
          </el-form-item>
        </el-form>
      </el-tab-pane>

      <!-- 增强1：BUG追踪器（原日志面板） -->
      <el-tab-pane label="BUG追踪器" name="logs">
        <div class="log-panel">
          <div class="log-toolbar">
            <el-switch v-model="bugTrackerEnabled" active-text="追踪已开启" inactive-text="追踪关闭" />
            <span class="hint">开启后详细记录操作与报错；应用退出时自动转储为 txt 文件</span>
          </div>
          <div class="log-toolbar" style="margin-top: 6px">
            <el-button size="small" @click="loadLogs">刷新</el-button>
            <el-button size="small" type="primary" plain @click="exportLogs">导出 txt（前端）</el-button>
            <el-button size="small" type="warning" plain :loading="dumpLoading" @click="dumpLogsBackend">转储（后端）</el-button>
            <el-button size="small" type="danger" plain @click="clearLogs">清空日志</el-button>
          </div>
          <div class="log-toolbar" style="margin-top: 6px; flex-wrap: wrap">
            <el-switch v-model="settings.settings.log_clear_on_start" active-text="启动时清空旧日志" inactive-text="保留旧日志" />
          </div>
          <div class="log-toolbar" style="margin-top: 6px; flex-wrap: wrap">
            <span class="hint">自动刷新：</span>
            <el-select v-model="logAutoRefresh" size="small" style="width: 100px">
              <el-option v-for="o in logRefreshOptions" :key="o.value" :value="o.value" :label="o.label" />
            </el-select>
            <span class="hint">自动滚动：</span>
            <el-switch v-model="logAutoScroll" size="small" active-text="开" inactive-text="关" />
          </div>
          <div v-loading="logLoading" ref="logListRef" class="log-list">
            <el-empty v-if="logs.length === 0 && !logLoading" description="暂无日志" :image-size="50" />
            <div v-for="l in logs" :key="l.id" class="log-line">
              <span class="log-time">{{ new Date(l.created_at * 1000).toLocaleString() }}</span>
              <el-tag :type="logLevelType(l.level)" size="small">{{ l.level }}</el-tag>
              <el-tag size="small" type="info">{{ logCategoryLabel(l.category) }}</el-tag>
              <span class="log-msg">{{ l.message }}</span>
            </div>
          </div>
        </div>
      </el-tab-pane>
    </el-tabs>

    <div class="save-bar">
      <el-button type="primary" :loading="saving" @click="saveSettings">保存设置</el-button>
      <el-button @click="settings.reset()">恢复默认</el-button>
    </div>

    <!-- 管理密钥弹窗（E7）：实时额度 + 编辑 + 状态 + 红垃圾桶删除 -->
    <el-dialog v-model="manageVisible" title="管理密钥" width="640px">
      <el-table :data="keys" size="small">
        <el-table-column prop="name" label="名称" width="100" />
        <el-table-column label="等级" width="70">
          <template #default="{ row }">
            <el-tag :type="row.tier === 'member' ? 'primary' : 'info'" size="small">
              {{ row.tier === 'member' ? '付费' : '免费' }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column prop="key_masked" label="密钥" width="120" />
        <el-table-column label="当日额度" width="120">
          <template #default="{ row }">
            <span :class="{ warn: (row.long_remaining as number) < 10 }">
              {{ row.long_remaining ?? '—' }}
            </span>
            <el-button size="small" text type="primary" @click="editQuota(row.name)">改</el-button>
          </template>
        </el-table-column>
        <el-table-column label="状态" width="90">
          <template #default="{ row }">
            <el-tag v-if="row.daily_paused" type="danger" size="small">已停用</el-tag>
            <el-tag v-else-if="(row.cooldown_secs as number) > 0" type="warning" size="small">冷却中</el-tag>
            <el-tag v-else type="success" size="small">可用</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="" width="56" align="center">
          <template #default="{ row }">
            <el-button size="small" type="danger" text title="删除密钥" @click="removeKey(row.name)">
              <el-icon><Delete /></el-icon>
            </el-button>
          </template>
        </el-table-column>
      </el-table>
      <template #footer>
        <el-button @click="manageVisible = false">关闭</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.settings-page {
  max-width: 960px;
}
.hint {
  margin-left: 8px;
  color: var(--el-text-color-secondary);
  font-size: 12px;
}
.save-bar {
  margin-top: 16px;
}
.key-tag {
  margin-right: 8px;
}
.inf-card {
  margin-bottom: 12px;
}
.infer-status-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
  flex-wrap: wrap;
}
.infer-desc {
  margin-top: 4px;
}
.inf-alert {
  margin-bottom: 12px;
}
.mono {
  font-family: monospace;
  font-size: 12px;
  word-break: break-all;
}
.warn {
  color: var(--el-color-danger);
  font-weight: 600;
}
.log-panel {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.log-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
}
.log-list {
  max-height: 60vh;
  overflow-y: auto;
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 6px;
  padding: 8px;
  background: var(--el-fill-color-lighter);
}
.log-line {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 3px 4px;
  font-size: 12px;
  border-bottom: 1px solid var(--el-border-color-lighter);
}
.log-time {
  color: var(--el-text-color-secondary);
  flex: none;
  font-family: monospace;
}
.log-msg {
  flex: 1;
  word-break: break-all;
}
</style>
