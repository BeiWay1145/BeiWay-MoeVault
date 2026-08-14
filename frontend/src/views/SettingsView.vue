<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Delete } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { get, post, del, put } from '@/api/client'
import { useSettingsStore, type SauceKeyConfig } from '@/stores/settings'
import { reportLog } from '@/api/log'

const settings = useSettingsStore()
// 默认打开「通用」设置页
const activeTab = ref('library')

// ---- SauceNAO 多 key ----
const newKey = ref('')
const newKeyName = ref('')
const newKeyTier = ref('free')
const keys = ref<SauceKeyConfig[] & Array<Record<string, unknown>>>([])
const manageVisible = ref(false)
const saving = ref(false)

// ---- 打标模型 ----
const taggerModelOptions = [
  { name: 'cl_tagger (SIGLIP2 ONNX)', dir: 'D:/Game/AI/cl_tagger/models' },
  { name: '自定义目录', dir: '' },
]

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
    reportLog('用户修改并保存了设置')
    ElMessage.success('设置已保存')
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    saving.value = false
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
  if (opt && opt.dir) {
    settings.settings.tagger_model_dir = opt.dir
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
            <span class="hint">开启后图库按每页固定条数分页（适合低配置 PC），关闭则一次加载全部</span>
          </el-form-item>
          <el-form-item v-if="settings.settings.pagination_enabled" label="每页条数">
            <el-select v-model="settings.settings.page_size" style="width: 120px">
              <el-option :value="25" label="25" />
              <el-option :value="50" label="50" />
              <el-option :value="75" label="75" />
              <el-option :value="100" label="100" />
            </el-select>
          </el-form-item>
          <el-form-item label="预加载图片张数">
            <el-input-number v-model="settings.settings.preload_count" :min="0" :max="5" />
            <span class="hint">详情页切换图片时预加载前后各 N 张原图（0=关闭，默认 2），减少切换闪灰</span>
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
        <el-card header="打标" shadow="never" class="inf-card">
          <el-form label-width="160px" style="max-width: 720px">
            <el-form-item label="打标模型">
              <el-select
                :model-value="settings.settings.tagger_model_name"
                style="width: 260px"
                @change="onModelSelect"
              >
                <el-option v-for="o in taggerModelOptions" :key="o.name" :label="o.name" :value="o.name" />
              </el-select>
              <el-button style="margin-left: 8px" @click="ElMessage.info('当前: ' + settings.settings.tagger_model_dir)">
                模型路径
              </el-button>
            </el-form-item>
            <el-form-item label="模型目录">
              <el-input v-model="settings.settings.tagger_model_dir" placeholder="D:/Game/AI/cl_tagger/models" style="width: 400px" />
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
      </el-tab-pane>

      <el-tab-pane label="查重" name="dedup">
        <el-form label-width="160px" style="max-width: 720px">
          <el-form-item label="pHash 汉明距离阈值">
            <el-slider v-model="settings.settings.dedup_hamming" :min="0" :max="64" show-input style="width: 260px" />
          </el-form-item>
        </el-form>
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
          <el-form-item label="中文字典">
            <el-switch v-model="settings.settings.cn_dict_enabled" active-text="开启" inactive-text="关闭" />
          </el-form-item>
        </el-form>
      </el-tab-pane>

      <!-- 日志面板（日志追踪器）：任务/溯源/打标/前端操作记录，排查问题用 -->
      <el-tab-pane label="日志" name="logs">
        <div class="log-panel">
          <div class="log-toolbar">
            <el-button size="small" @click="loadLogs">刷新</el-button>
            <el-button size="small" type="primary" plain @click="exportLogs">导出 txt</el-button>
            <el-button size="small" type="danger" plain @click="clearLogs">清空日志</el-button>
            <span class="hint">记录任务生命周期、溯源/打标结果、前端操作，排查打标/溯源失败用</span>
          </div>
          <div class="log-toolbar" style="margin-top: 6px; flex-wrap: wrap">
            <el-switch v-model="settings.settings.log_clear_on_start" active-text="启动时清空旧日志" inactive-text="保留旧日志" />
            <span class="hint">开启后每次启动服务自动清空旧日志并写入一条「服务已启动」记录（默认开启）</span>
          </div>
          <div class="log-toolbar" style="margin-top: 6px; flex-wrap: wrap">
            <span class="hint">自动刷新：</span>
            <el-select v-model="logAutoRefresh" size="small" style="width: 100px">
              <el-option v-for="o in logRefreshOptions" :key="o.value" :value="o.value" :label="o.label" />
            </el-select>
            <span class="hint">自动滚动：</span>
            <el-switch v-model="logAutoScroll" size="small" active-text="开" inactive-text="关" />
            <span class="hint">打开本页立即刷新；自动滚动仅在已滚到底部时跟随最新日志</span>
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
