<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { get, del } from '@/api/client'
import { useSettingsStore } from '@/stores/settings'
import { reportLog } from '@/api/log'
import { getTrackerState, setTrackerState, type TrackerState } from '@/api/tracking'

const settings = useSettingsStore()

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
/** 显示条数（服务端一次拉取量，默认 200，最大 5000）。 */
const logLimit = ref(200)
const logRefreshOptions = [
  { value: 0, label: '关闭' },
  { value: 5, label: '5 秒' },
  { value: 10, label: '10 秒' },
  { value: 30, label: '30 秒' },
  { value: 60, label: '60 秒' },
]
const logLimitOptions = [
  { value: 200, label: '200 条' },
  { value: 500, label: '500 条' },
  { value: 1000, label: '1000 条' },
  { value: 2000, label: '2000 条' },
  { value: 5000, label: '5000 条（最全）' },
]
/** 服务端本次返回的条数（用于提示「是否被截断」）。 */
const logLoadedCount = ref(0)
let logTimer: number | undefined

/**
 * 本组件是否可见（由父级 el-tab-pane 的激活态驱动）。
 *
 * 拆出组件后不能再读父级的 activeTab，否则定时器会在其它 tab 下继续跑。
 */
const props = defineProps<{ active?: boolean }>()

function loadLogSettings() {
  try {
    const raw = localStorage.getItem(LOG_SETTINGS_KEY)
    if (!raw) return
    const s = JSON.parse(raw) as { refresh?: number; scroll?: boolean; limit?: number }
    if (typeof s.refresh === 'number' && logRefreshOptions.some((o) => o.value === s.refresh)) {
      logAutoRefresh.value = s.refresh
    }
    if (typeof s.scroll === 'boolean') logAutoScroll.value = s.scroll
    if (typeof s.limit === 'number' && logLimitOptions.some((o) => o.value === s.limit)) {
      logLimit.value = s.limit
    }
  } catch {
    /* 解析失败用默认值 */
  }
}
function saveLogSettings() {
  try {
    localStorage.setItem(
      LOG_SETTINGS_KEY,
      JSON.stringify({
        refresh: logAutoRefresh.value,
        scroll: logAutoScroll.value,
        limit: logLimit.value,
      }),
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
    // 级别过滤交给服务端：否则海量 info 会把 error 挤出返回窗口（BUG 追踪器核心诉求）
    const qs = new URLSearchParams({ limit: String(logLimit.value) })
    const levels = activeLevels.value.join(',')
    if (levels) qs.set('levels', levels)
    const d = await get<{ items: LogEntry[] }>('/logs?' + qs.toString())
    // 最新在下（后端返回倒序，反转显示）
    logs.value = [...d.items].reverse()
    logLoadedCount.value = d.items.length
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

// BUG 追踪器——三态（关闭 / 本会话开启 / 开启，开关 + 仅本次会话勾选）+ 后端转储
const trackerState = ref<TrackerState>(getTrackerState())
const trackerSessionOnly = ref(trackerState.value === 'session')
const trackerOn = ref(trackerState.value !== 'off')
watch([trackerOn, trackerSessionOnly], () => {
  const next: TrackerState = trackerOn.value ? (trackerSessionOnly.value ? 'session' : 'on') : 'off'
  setTrackerState(next)
  trackerState.value = next
  reportLog(
    `BUG追踪器状态：${next === 'on' ? '开启' : next === 'session' ? '本会话开启' : '关闭'}`,
  )
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

// 日志设置变化 → 持久化 + 重启定时器；本组件可见时立即刷新 + 启动定时器
watch([logAutoRefresh, logAutoScroll], () => {
  saveLogSettings()
  if (props.active) startLogTimer()
})
watch(
  () => props.active,
  (on) => {
    if (on) {
      loadLogs()
      startLogTimer()
    } else {
      stopLogTimer()
    }
  },
)

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
  ({
    task: '任务',
    sauce: '溯源',
    tag: '打标',
    aesthetic: '美学',
    frontend: '前端',
    import: '导入',
    system: '系统',
    track: '追踪',
  })[c] ?? c

/** 追踪日志格式化：JSON 行 → 易读文本（[API] POST /logs → 200 (2ms) / [错误] msg）。 */
function formatLogMsg(category: string, message: string): string {
  if (category !== 'track') return message
  const lines = message.split('\n')
  const out: string[] = []
  for (const line of lines) {
    try {
      const o = JSON.parse(line)
      if (o.t === 'api') {
        out.push(
          `[API] ${o.m} ${o.p} → ${o.s ?? '网络错误'} (${o.ms ?? '?'}ms)${o.e ? ` ✗ ${o.e}` : ''}`,
        )
      } else if (o.t === 'err') {
        out.push(`[错误·${o.src}] ${o.msg}${o.stack ? `\n    ${String(o.stack).slice(0, 300)}` : ''}`)
      } else if (o.t === 'act') {
        out.push(`[操作] ${o.a}`)
      } else {
        out.push(line)
      }
    } catch {
      out.push(line)
    }
  }
  return out.join('\n')
}

/** 级别筛选（默认全开）——同时下发服务端做 SQL 过滤。 */
const filterInfo = ref(true)
const filterWarn = ref(true)
const filterError = ref(true)
/** 已勾选的级别列表（服务端查询参数）。 */
const activeLevels = computed(() => {
  const out: string[] = []
  if (filterInfo.value) out.push('info')
  if (filterWarn.value) out.push('warn')
  if (filterError.value) out.push('error')
  return out
})
/** 只看错误：一键过滤掉 info/warning，error 不再被顶掉。 */
function onlyErrors() {
  filterInfo.value = false
  filterWarn.value = false
  filterError.value = true
}
/** 恢复全级别。 */
function allLevels() {
  filterInfo.value = true
  filterWarn.value = true
  filterError.value = true
}
/** 级别勾选变化 → 按新级别重新拉取（服务端过滤）。 */
watch([filterInfo, filterWarn, filterError], () => {
  if (props.active) loadLogs()
})
/** 显示条数变化 → 持久化 + 重新拉取。 */
watch(logLimit, () => {
  saveLogSettings()
  if (props.active) loadLogs()
})
/** 文本搜索。 */
const logSearch = ref('')
/** 过滤后的日志列表（级别已由服务端过滤，这里做文本搜索兜底）。 */
const filteredLogs = computed(() => {
  const kw = logSearch.value.trim().toLowerCase()
  return logs.value.filter((l) => {
    if (l.level === 'info' && !filterInfo.value) return false
    if (l.level === 'warn' && !filterWarn.value) return false
    if (l.level === 'error' && !filterError.value) return false
    if (kw) {
      const fmt = formatLogMsg(l.category, l.message).toLowerCase()
      if (!fmt.includes(kw) && !l.category.includes(kw)) return false
    }
    return true
  })
})

onMounted(() => {
  loadLogSettings()
  if (props.active) {
    loadLogs()
    startLogTimer()
  }
})

// 定时器必须随组件卸载停止，否则离开设置页后仍在后台请求
onBeforeUnmount(stopLogTimer)

defineExpose({ reload: loadLogs })
</script>

<template>
  <div class="log-panel">
    <div class="log-toolbar">
      <el-switch v-model="trackerOn" active-text="追踪开启" inactive-text="追踪关闭" />
      <el-checkbox v-model="trackerSessionOnly" :disabled="!trackerOn">仅本次会话</el-checkbox>
      <span class="hint">
        {{
          trackerState === 'on'
            ? '状态：开启（持久，重启仍生效）'
            : trackerState === 'session'
              ? '状态：本会话开启（重启后关闭）'
              : '状态：关闭（不记录）'
        }}
      </span>
    </div>
    <div class="log-toolbar" style="margin-top: 4px">
      <span class="hint">
        开启后全量记录：API 请求（脱敏）+ 前端错误堆栈 + 关键操作；应用退出时自动转储为 txt
      </span>
    </div>
    <div class="log-toolbar" style="margin-top: 6px">
      <el-button size="small" @click="loadLogs">刷新</el-button>
      <el-button size="small" type="primary" plain @click="exportLogs">导出 txt（前端）</el-button>
      <el-button size="small" type="warning" plain :loading="dumpLoading" @click="dumpLogsBackend">
        转储（后端）
      </el-button>
      <el-button size="small" type="danger" plain @click="clearLogs">清空日志</el-button>
    </div>
    <div class="log-toolbar" style="margin-top: 6px; flex-wrap: wrap">
      <el-switch
        v-model="settings.settings.log_clear_on_start"
        active-text="启动时清空旧日志"
        inactive-text="保留旧日志"
      />
    </div>
    <div class="log-toolbar" style="margin-top: 6px; flex-wrap: wrap">
      <span class="hint">自动刷新：</span>
      <el-select v-model="logAutoRefresh" size="small" style="width: 100px">
        <el-option v-for="o in logRefreshOptions" :key="o.value" :value="o.value" :label="o.label" />
      </el-select>
      <span class="hint">自动滚动：</span>
      <el-switch v-model="logAutoScroll" size="small" active-text="开" inactive-text="关" />
    </div>
    <div class="log-toolbar" style="margin-top: 6px; flex-wrap: wrap">
      <span class="hint">级别筛选（服务端）：</span>
      <el-checkbox v-model="filterInfo" size="small">info</el-checkbox>
      <el-checkbox v-model="filterWarn" size="small">warning</el-checkbox>
      <el-checkbox v-model="filterError" size="small">error</el-checkbox>
      <el-button size="small" text type="primary" @click="onlyErrors">只看错误</el-button>
      <el-button size="small" text @click="allLevels">全级别</el-button>
      <span class="hint" style="margin-left: 8px">显示条数：</span>
      <el-select v-model="logLimit" size="small" style="width: 140px">
        <el-option v-for="o in logLimitOptions" :key="o.value" :value="o.value" :label="o.label" />
      </el-select>
      <el-input
        v-model="logSearch"
        size="small"
        placeholder="搜索日志内容…"
        clearable
        style="width: 180px; margin-left: 8px"
      />
      <span class="hint" style="margin-left: 8px">
        已加载 {{ logLoadedCount }} 条{{
          logLoadedCount >= logLimit ? '（已达上限，可增大显示条数）' : ''
        }}
      </span>
    </div>
    <div v-loading="logLoading" ref="logListRef" class="log-list">
      <el-empty
        v-if="filteredLogs.length === 0 && !logLoading"
        description="暂无日志（或筛选后为空）"
        :image-size="50"
      />
      <div
        v-for="l in filteredLogs"
        :key="l.id"
        class="log-line"
        :class="{ 'log-line-err': l.level === 'error' }"
      >
        <span class="log-time">{{ new Date(l.created_at * 1000).toLocaleString() }}</span>
        <el-tag :type="logLevelType(l.level)" size="small">{{ l.level }}</el-tag>
        <el-tag size="small" type="info">{{ logCategoryLabel(l.category) }}</el-tag>
        <span class="log-msg">{{ formatLogMsg(l.category, l.message) }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.log-panel {
  display: flex;
  flex-direction: column;
  max-width: 1100px;
}
.log-toolbar {
  display: flex;
  align-items: center;
  gap: 6px;
}
.log-list {
  margin-top: 8px;
  height: 520px;
  overflow-y: auto;
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 4px;
  padding: 6px 8px;
  background: var(--el-fill-color-blank);
  font-variant-numeric: tabular-nums;
}
.log-line {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  padding: 2px 0;
  font-size: 12px;
  line-height: 1.5;
  border-bottom: 1px dashed var(--el-border-color-extra-light);
}
.log-line-err {
  background: var(--el-color-danger-light-9);
}
.log-time {
  color: var(--el-text-color-secondary);
  flex: none;
}
.log-msg {
  white-space: pre-wrap;
  word-break: break-all;
}
</style>
