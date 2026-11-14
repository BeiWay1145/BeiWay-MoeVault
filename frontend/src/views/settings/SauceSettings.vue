<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Delete } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox, ElNotification } from 'element-plus'
import { get, post, put, del } from '@/api/client'
import { useSettingsStore, type SauceKeyConfig } from '@/stores/settings'

const settings = useSettingsStore()

// ---- 多 key 管理 ----
const newKey = ref('')
const newKeyName = ref('')
const newKeyTier = ref('free')
const keys = ref<SauceKeyConfig[] & Array<Record<string, unknown>>>([])
const manageVisible = ref(false)

/** 全局 IP 池窗口状态（免费账号 4 次 / 30 秒，跨 key 共享）。 */
interface RateWindow {
  used: number
  limit: number
  cooling_secs: number
  window_secs: number
}
const rateWindow = ref<RateWindow | null>(null)

// ---- 多出口 IP 轮换（Clash 多入站端口）----
interface ExitItem {
  port: number
  exit_ip: string | null
  total_requests: number
  disabled: boolean
  cooldown_secs: number
  consecutive_failures: number
}
const exitPortsText = ref('')
const exitEnabled = ref(false)
const exitItems = ref<ExitItem[]>([])
const exitTesting = ref(false)
/** 测试结果中的重复 IP 警告。 */
const exitHint = ref('')

/** 读取出口配置与实时状态。 */
async function loadExits() {
  try {
    const d = await get<{ enabled: boolean; ports: number[]; exits: ExitItem[] }>(
      '/settings/sauce-exits',
    )
    exitEnabled.value = d.enabled
    exitPortsText.value = d.ports.join('\n')
    exitItems.value = d.exits ?? []
  } catch {
    /* 未配置时接口可能不可用，静默 */
  }
}

/** 保存出口配置（端口列表 + 开关）。 */
async function saveExits() {
  try {
    await put('/settings', {
      sauce_proxy_enabled: String(exitEnabled.value),
      sauce_proxy_ports: exitPortsText.value,
    })
    ElMessage.success('出口配置已保存')
    await loadExits()
  } catch (e) {
    ElNotification({ title: '保存失败', message: (e as Error).message, type: 'error' })
  }
}

/** 测试全部出口：探测各自出口 IP。多个端口 IP 相同会给出明确警告。 */
async function testExits() {
  exitTesting.value = true
  exitHint.value = ''
  try {
    // 先保存再测试，保证后端读到的就是界面上的端口
    await put('/settings', {
      sauce_proxy_enabled: String(exitEnabled.value),
      sauce_proxy_ports: exitPortsText.value,
    })
    const d = await post<{
      exits: { port: number; exit_ip: string | null; note: string | null }[]
      duplicate_ips: boolean
      hint: string
    }>('/settings/sauce-exits')
    exitItems.value = d.exits.map((e) => ({
      port: e.port,
      exit_ip: e.exit_ip,
      total_requests: 0,
      disabled: !e.exit_ip,
      cooldown_secs: 0,
      consecutive_failures: 0,
    }))
    exitHint.value = d.hint || ''
    if (d.duplicate_ips) {
      ElNotification({
        title: '出口配置有问题',
        message: d.hint,
        type: 'warning',
        duration: 15000,
      })
    } else {
      ElMessage.success(`已测试 ${d.exits.length} 个出口，各自 IP 不同 ✓`)
    }
  } catch (e) {
    ElNotification({ title: '测试失败', message: (e as Error).message, type: 'error' })
  } finally {
    exitTesting.value = false
  }
}

/** 生成可直接粘贴到 Clash 配置的 listeners 片段。 */
function clashTemplate(): string {
  const ports = exitPortsText.value
    .split(/[\n,;\s]+/)
    .map((s) => s.trim())
    .filter((s) => /^\d+$/.test(s))
  if (ports.length === 0) return '# 请先在上方填写端口号（每行一个）'
  const lines: string[] = ['listeners:']
  ports.forEach((p, i) => {
    lines.push(`  - name: sauce-${i + 1}`)
    lines.push('    type: mixed')
    lines.push(`    port: ${p}`)
    lines.push(`    proxy: "SauceNAO-${i + 1}"   # 指向你在 proxy-groups 里定义的策略组`)
  })
  lines.push('')
  lines.push('# 同时需要在 proxy-groups 中为每个出口建一个只含单一节点的 select 组：')
  ports.forEach((_, i) => {
    lines.push('# proxy-groups:')
    lines.push(`#   - name: "SauceNAO-${i + 1}"`)
    lines.push('#     type: select')
    lines.push(`#     proxies: ["节点名${i + 1}"]`)
  })
  return lines.join('\n')
}

/** 显示 Clash 配置模板。 */
function showClashTemplate() {
  ElMessageBox.alert(
    `<pre style="white-space:pre-wrap;font-size:12px">${clashTemplate()}</pre>`,
    'Clash 配置模板',
    { dangerouslyUseHTMLString: true, confirmButtonText: '知道了' },
  )
}

async function loadKeys() {
  try {
    // list_keys 返回实时配额（short/long/cooldown/status）+ 全局窗口
    const d = await get<{
      keys: SauceKeyConfig[] & Array<Record<string, unknown>>
      count: number
      rate_window?: RateWindow | null
    }>('/settings/saucenao-keys')
    keys.value = d.keys
    rateWindow.value = d.rate_window ?? null
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 手动修改某 key 的当日额度。 */
async function editQuota(name: string) {
  const k = keys.value.find((x) => x.name === name) as
    | (SauceKeyConfig & Record<string, unknown>)
    | undefined
  const cur = Number(k?.long_remaining ?? 95)
  const { value } = await ElMessageBox.prompt('设置当日剩余额度', `修改额度 · ${name}`, {
    inputValue: String(cur),
    inputPattern: /^\d+$/,
    inputErrorMessage: '请输入数字',
  }).catch(() => ({ value: null as string | null }))
  if (value === null) return
  try {
    await put(`/settings/saucenao-keys/${encodeURIComponent(name)}/quota`, {
      long_remaining: Number(value),
    })
    ElMessage.success('额度已更新')
    await loadKeys()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

/** 安全删除密钥（需确认）。 */
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
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function openManage() {
  manageVisible.value = true
  await loadKeys()
}

/** 当前已配置的密钥数量（模板展示用）。 */
const keyCount = computed(() => keys.value.length)

onMounted(async () => {
  await loadKeys()
  await loadExits()
})

defineExpose({ reload: loadKeys })
</script>

<template>
  <el-form label-width="160px" style="max-width: 720px">
    <el-form-item label="API key">
      <el-input
        v-model="newKey"
        type="password"
        show-password
        placeholder="输入 SauceNAO API key"
        style="width: 300px"
      />
      <el-input
        v-model="newKeyName"
        placeholder="密钥名称（默认 Key0/1/2...）"
        style="width: 160px; margin-left: 8px"
      />
      <el-select v-model="newKeyTier" style="width: 110px; margin-left: 8px">
        <el-option label="免费" value="free" />
        <el-option label="付费" value="member" />
      </el-select>
      <el-button type="primary" style="margin-left: 8px" @click="addKey">添加</el-button>
    </el-form-item>
    <el-form-item label="相似度阈值">
      <el-slider
        v-model="settings.settings.saucenao_min_sim"
        :min="0"
        :max="100"
        show-input
        style="width: 260px"
      />
      %
    </el-form-item>
    <el-form-item label="已配置密钥">
      <el-tag v-for="k in keys" :key="k.name" closable class="key-tag" @close="removeKey(k.name)">
        {{ k.name }}（{{ k.tier === 'member' ? '付费' : '免费' }}）
      </el-tag>
      <el-tag v-if="keyCount === 0" type="info">未配置密钥</el-tag>
    </el-form-item>
    <el-form-item>
      <el-button @click="openManage">管理密钥（查看额度）</el-button>
    </el-form-item>
  </el-form>

  <!-- 管理密钥弹窗：实时额度 + 编辑 + 状态 + 删除 + 多出口 -->
  <el-dialog v-model="manageVisible" title="管理密钥" width="680px">
    <el-alert
      v-if="rateWindow"
      class="rate-window-alert"
      type="info"
      :closable="false"
      show-icon
    >
      <template #title>
        IP 池窗口：{{ rateWindow.used }} / {{ rateWindow.limit }} 次（每
        {{ rateWindow.window_secs }} 秒）
        <span v-if="rateWindow.cooling_secs > 0" class="rate-cooling">
          · 限流冷却中，剩余 {{ rateWindow.cooling_secs }} 秒
        </span>
      </template>
      <div class="hint">
        免费账号共享同一 IP 池，超出会被 SauceNAO 限流（-2）；调度器已按窗口自动排队，无需手动干预。
      </div>
    </el-alert>

    <el-divider content-position="left">多出口 IP 轮换（Clash）</el-divider>
    <el-alert type="info" :closable="false" show-icon style="margin-bottom: 10px">
      <template #title>用一个 IP 时，免费账号受「4 次 / 30 秒」限制</template>
      <div class="hint">
        在 Clash 里为不同节点各开一个入站端口，把端口号填在下面（每行一个）。 应用会为每个端口建一个出口，各自独立计算限额；
        当前出口被限流时会自动切到还有空闲的出口。<br />
        注意：账号的 <b>100 次/日</b> 是按账号算的，多出口只提升速度、不增加总量。
      </div>
    </el-alert>
    <el-form label-width="140px" style="margin-bottom: 8px">
      <el-form-item label="启用多出口">
        <el-switch v-model="exitEnabled" />
      </el-form-item>
      <el-form-item label="出口端口列表">
        <el-input
          v-model="exitPortsText"
          type="textarea"
          :rows="4"
          placeholder="每行一个端口号，例如：&#10;7901&#10;7902&#10;7903"
          style="width: 260px"
        />
        <el-button style="margin-left: 8px" @click="saveExits">保存</el-button>
        <el-button :loading="exitTesting" @click="testExits">测试全部</el-button>
        <el-button plain @click="showClashTemplate">Clash 配置模板</el-button>
      </el-form-item>
    </el-form>
    <el-alert v-if="exitHint" type="warning" :closable="false" show-icon style="margin-bottom: 10px">
      {{ exitHint }}
    </el-alert>
    <el-table v-if="exitItems.length > 0" :data="exitItems" size="small" style="margin-bottom: 12px">
      <el-table-column prop="port" label="端口" width="90" />
      <el-table-column label="状态" width="100">
        <template #default="{ row }">
          <el-tag :type="row.disabled ? 'danger' : 'success'" size="small">
            {{ row.disabled ? '不可用' : '正常' }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column prop="exit_ip" label="出口 IP" min-width="140">
        <template #default="{ row }">{{ row.exit_ip ?? '—' }}</template>
      </el-table-column>
      <el-table-column label="已请求" width="90">
        <template #default="{ row }">{{ row.total_requests }}</template>
      </el-table-column>
      <el-table-column label="冷却" width="90">
        <template #default="{ row }">
          {{ row.cooldown_secs > 0 ? row.cooldown_secs + 's' : '—' }}
        </template>
      </el-table-column>
    </el-table>

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
</template>

<style scoped>
.key-tag {
  margin-right: 8px;
}
.warn {
  color: var(--el-color-danger);
  font-weight: 600;
}
.rate-window-alert {
  margin-bottom: 10px;
}
.rate-cooling {
  color: var(--el-color-warning);
}
/* .hint 由父级 SettingsView 的非 scoped 全局块提供，此处不再重复定义 */
</style>
