<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { CaretRight, Download, Refresh, VideoPause } from '@element-plus/icons-vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { get } from '@/api/client'
import { useSettingsStore } from '@/stores/settings'
import {
  fetchInferHealth,
  inferInstallDeps,
  inferInstallGpu,
  inferGpuStatus,
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

// ---- 打标模型：模型种类 + 自动探测（推荐）/ 自定义目录 ----
// 模型种类：auto=按目录内容自动判定；cl_tagger=SIGLIP2 ONNX（当前主力）；wd14=wd14 tagger ONNX（兼容）
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

// ---- GPU 推理环境部署 ----
const gpuInstalling = ref(false)
const gpuCudaReady = ref(false)
const gpuInstallBusy = ref(false)
let gpuPollTimer: number | undefined
/** 手动安装教程对话框。 */
const gpuTutorialVisible = ref(false)
/**
 * 手动安装教程内容。
 *
 * 注意：运行目录在 v0.2 起迁到「安装根目录/runtime/」，
 * 不再是 %LOCALAPPDATA%\BeiWay-MoeVault（见 src-tauri/src/lib.rs 的 runtime_root()）。
 */
const gpuTutorialText = [
  '【手动安装 GPU 推理环境教程】',
  '',
  '自动部署失败通常是本机网络无法直连 PyTorch 官方源（pip TLS 被阻断）。按以下步骤手动安装（约 2.9GB）：',
  '',
  '第 1 步：用浏览器下载两个文件（浏览器网络可直连官方源）',
  '  · https://download.pytorch.org/whl/cu128/torch/',
  '  · 下载 torch-2.11.0+cu128-cp310-cp310-win_amd64.whl',
  '  · 下载 torchvision-0.26.0+cu128-cp310-cp310-win_amd64.whl',
  '',
  '第 2 步：找到应用的运行目录（安装目录下的 runtime\python）',
  '  · 常见位置：D:\Program Files\BeiWay-MoeVault\runtime\python',
  '  · 不确定时看「设置 → 本地推理」里的推理服务日志路径，或搜索 infer.log',
  '  · 把两个 .whl 文件放进该目录',
  '',
  '第 3 步：打开命令提示符（Win+R → cmd），在该目录执行',
  '  · cd /d "D:\Program Files\BeiWay-MoeVault\runtime\python"',
  '  · .venv\Scripts\python.exe -m pip install torch-2.11.0+cu128-cp310-cp310-win_amd64.whl torchvision-0.26.0+cu128-cp310-cp310-win_amd64.whl --index-url https://pypi.tuna.tsinghua.edu.cn/simple',
  '',
  '第 4 步：等待安装完成，重启应用',
  '  · 回到本设置页，GPU 推理环境应显示「CUDA 已就绪」',
  '  · 推理设备下拉会出现「CUDA GPU1（显卡型号）」选项',
  '',
  '提示：若浏览器也下载不动，可尝试其他镜像（百度网盘/迅雷搜索 torch 2.11.0 cu128 或使用代理加速）。',
].join('\n')

function showGpuTutorial() {
  gpuTutorialVisible.value = true
}

/** 复制教程文本到剪贴板。 */
async function copyGpuTutorial() {
  try {
    await navigator.clipboard.writeText(gpuTutorialText)
    ElMessage.success('教程已复制到剪贴板')
  } catch {
    ElMessage.error('复制失败，请手动选择文本复制')
  }
}

async function refreshGpuStatus() {
  try {
    const s = await inferGpuStatus()
    if (s) {
      gpuInstalling.value = s.installing
      gpuCudaReady.value = s.cuda_ready
    }
  } catch {
    /* 静默 */
  }
}

async function installGpuEnv() {
  try {
    await ElMessageBox.confirm(
      '将下载并安装 CUDA 12.8 版 PyTorch（约 3GB，支持 RTX 50 系及旧卡），安装期间可正常使用应用但不可重复点击。继续？',
      '安装 GPU 推理环境',
      { type: 'warning', confirmButtonText: '开始安装' },
    )
  } catch {
    return
  }
  gpuInstallBusy.value = true
  try {
    const r = await inferInstallGpu()
    ElMessage.success(r.message)
    gpuInstalling.value = true
    // 轮询安装状态（10s 间隔）
    if (gpuPollTimer !== undefined) window.clearInterval(gpuPollTimer)
    gpuPollTimer = window.setInterval(async () => {
      await refreshGpuStatus()
      if (!gpuInstalling.value && gpuPollTimer !== undefined) {
        window.clearInterval(gpuPollTimer)
        gpuPollTimer = undefined
        ElMessage.success(
          gpuCudaReady.value
            ? 'GPU 推理环境已就绪（CUDA 可用）'
            : 'GPU 环境安装流程结束（详见 python/infer.log）',
        )
      }
    }, 10000)
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    gpuInstallBusy.value = false
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

// ---- 当前生效模型名称与来源 ----
const taggerEffectiveName = computed(() => {
  const dir = inferHealth.value?.paths?.tagger_model_dir
  if (!dir) return '（服务未启动）'
  const parts = dir.replace(/[\\/]+$/, '').split(/[\\/]/)
  const base = parts[parts.length - 1] || dir
  return base
})
const taggerEffectiveSource = computed(() => {
  const dir = inferHealth.value?.paths?.tagger_model_dir
  if (!dir) return '未知'
  if (dir.toLowerCase().includes('models') && dir.includes('image')) return `本地目录 ${dir}`
  return dir
})

// ---- 美学模型预设选择 ----
const AESTHETIC_PRESETS = [
  'trojblue/distill-q-align-aesthetic-siglip2-base',
  'Disty0/aesthetic-shadow-v2',
]
const aestheticModelPreset = computed(() => {
  const m = settings.settings.aesthetic_model
  if (AESTHETIC_PRESETS.includes(m)) return m
  return '__custom__'
})
function onAestheticModelSelect(v: string) {
  if (v === '__custom__') {
    settings.settings.aesthetic_model = ''
    ElMessage.info('请输入自定义 HF 仓库名或本地目录')
  } else {
    settings.settings.aesthetic_model = v
    ElMessage.info('模型已切换，保存设置并重跑美学任务生效')
  }
}
const aestheticEffectiveSource = computed(() => {
  const dir = inferHealth.value?.paths?.aesthetic_model
  if (!dir) return '未知'
  // 含盘符/反斜杠 → 本地目录；否则 HF 仓库
  if (/^[A-Za-z]:[\\/]/.test(dir) || dir.includes('\\')) return `本地目录（首次无需下载）`
  return 'HF 仓库（首次使用自动下载）'
})

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
/** 推理能力诊断（来自 /devices）：用于提示"打标 CUDA 是否真的可用"。 */
const inferCaps = ref<{
  tagger_cuda?: boolean
  tagger_providers?: string[]
  aesthetic_cuda?: boolean
} | null>(null)
async function loadDevices() {
  try {
    const d = await get<{
      devices: DeviceOption[]
      capabilities?: { tagger_cuda?: boolean; tagger_providers?: string[]; aesthetic_cuda?: boolean }
    }>('/devices')
    inferCaps.value = d.capabilities ?? null
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
  await loadDevices()
  loadInferHealth()
  loadShellDiagnostics()
  refreshGpuStatus()
})

// GPU 安装状态轮询必须随组件卸载停止，否则离开设置页后仍在后台请求
onBeforeUnmount(() => {
  if (gpuPollTimer !== undefined) window.clearInterval(gpuPollTimer)
})

defineExpose({ reload: loadInferHealth })
</script>

<template>
  <div>
    <!-- 推理服务状态卡片：服务健康 + 当前模型路径 + 启动/停止（桌面版） -->
    <el-card header="推理服务" shadow="never" class="inf-card">
      <div class="infer-status-row">
        <el-tag :type="inferStatusType()">{{ inferStatusText() }}</el-tag>
        <el-button size="small" :icon="Refresh" :loading="inferLoading" @click="loadInferHealth">
          刷新
        </el-button>
        <template v-if="isTauri()">
          <el-button
            size="small"
            type="primary"
            plain
            :icon="CaretRight"
            :loading="inferBusy"
            :disabled="inferOverall === 'running'"
            @click="onInferStart"
          >
            启动服务
          </el-button>
          <el-button
            size="small"
            type="danger"
            plain
            :icon="VideoPause"
            :loading="inferBusy"
            :disabled="inferOverall === 'stopped'"
            @click="onInferStop"
          >
            停止服务
          </el-button>
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
          <span>{{
            inferHealth?.models?.tagger?.kind ? kindLabel(inferHealth.models.tagger.kind) : '（服务未启动）'
          }}</span>
        </el-descriptions-item>
        <el-descriptions-item label="打标模型目录">
          <span class="mono">{{ inferHealth?.paths?.tagger_model_dir || '（服务未启动）' }}</span>
          <span v-if="inferHealth && !settings.settings.tagger_model_dir" class="hint">自动探测</span>
          <span v-else-if="settings.settings.tagger_model_dir" class="hint">自定义（设置保存后生效）</span>
        </el-descriptions-item>
        <el-descriptions-item label="打标模型状态">
          {{ modelStateText(inferHealth?.models?.tagger) }}
        </el-descriptions-item>
        <el-descriptions-item label="美学模型">
          <span class="mono">{{ inferHealth?.paths?.aesthetic_model || '（服务未启动）' }}</span>
        </el-descriptions-item>
        <el-descriptions-item label="美学模型状态">
          {{ modelStateText(inferHealth?.models?.aesthetic) }}
        </el-descriptions-item>
        <el-descriptions-item label="GPU 推理环境">
          <template v-if="isTauri()">
            <el-tag v-if="gpuCudaReady" type="success" size="small">CUDA 已就绪</el-tag>
            <el-tag v-else-if="gpuInstalling" type="warning" size="small">
              安装中…（进度见 python/infer.log）
            </el-tag>
            <el-tag v-else type="info" size="small">未安装（CPU 模式）</el-tag>
            <el-button
              size="small"
              type="primary"
              plain
              style="margin-left: 8px"
              :disabled="gpuInstalling || gpuCudaReady"
              :loading="gpuInstallBusy"
              @click="installGpuEnv"
            >
              {{ gpuCudaReady ? '已就绪' : gpuInstalling ? '安装中…' : '一键部署 GPU 环境' }}
            </el-button>
            <el-button size="small" text type="primary" style="margin-left: 6px" @click="showGpuTutorial">
              手动安装教程
            </el-button>
          </template>
          <span v-else class="hint">仅桌面版支持部署</span>
        </el-descriptions-item>
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
          <span class="hint">
            auto=按模型目录自动判定；cl-tagger（当前主力）用 model_vocabulary.json，wd14（兼容）用
            selected_tags.csv
          </span>
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
        <el-form-item label="当前生效模型">
          <span class="mono">{{ taggerEffectiveName }}</span>
          <span class="hint">来源：{{ taggerEffectiveSource }}</span>
        </el-form-item>
        <el-form-item label="自定义目录">
          <el-input
            v-model="settings.settings.tagger_model_dir"
            placeholder="留空 = 自动探测（推荐）"
            style="width: 400px"
          />
          <span class="hint">留空自动探测；填写后保存设置并重跑打标任务生效</span>
        </el-form-item>
        <el-form-item label="置信度阈值">
          <el-slider
            v-model="settings.settings.tag_threshold"
            :min="0"
            :max="1"
            :step="0.05"
            show-input
            style="width: 260px"
          />
        </el-form-item>
        <el-form-item label="推理设备">
          <el-select v-model="settings.settings.tagger_device" style="width: 260px">
            <el-option v-for="d in taggerDevices" :key="d.id" :label="d.name" :value="d.id" />
          </el-select>
          <span v-if="inferCaps && inferCaps.tagger_cuda === false" class="hint hint-warn">
            打标当前只能跑 CPU：onnxruntime 未提供 CUDAExecutionProvider （已安装的 providers：{{
              (inferCaps.tagger_providers || []).join('、') || '未知'
            }}）。 如需 GPU 打标，请在推理环境安装 onnxruntime-gpu。
          </span>
          <span v-else-if="inferCaps && inferCaps.tagger_cuda" class="hint">
            打标 GPU 可用（CUDAExecutionProvider 已就绪）
          </span>
          <span class="hint">GPU 加速打标（下次任务生效）</span>
        </el-form-item>
      </el-form>
    </el-card>

    <el-card header="美学评分" shadow="never" class="inf-card">
      <el-form label-width="160px" style="max-width: 720px">
        <el-form-item label="模型种类">
          <el-radio-group v-model="settings.settings.aesthetic_kind">
            <el-radio value="qalign">Q-Align（偏真人摄影）</el-radio>
            <el-radio value="anime">Aesthetic Shadow V2（二次元特化）</el-radio>
          </el-radio-group>
          <span class="hint">二次元图建议用 anime 模型；首次使用自动下载（约 2.2GB）；下次评分任务生效</span>
        </el-form-item>
        <el-form-item label="模型来源">
          <el-select
            :model-value="aestheticModelPreset"
            style="width: 380px"
            @change="(v: string) => onAestheticModelSelect(v)"
          >
            <el-option
              label="Q-Align：trojblue/distill-q-align-aesthetic-siglip2-base（HF 仓库）"
              value="trojblue/distill-q-align-aesthetic-siglip2-base"
            />
            <el-option
              label="Aesthetic Shadow V2：Disty0/aesthetic-shadow-v2（HF 仓库，二次元）"
              value="Disty0/aesthetic-shadow-v2"
            />
            <el-option label="自定义（本地目录 / HF 仓库名）" value="__custom__" />
          </el-select>
          <span class="hint">HF 仓库模型首次使用自动下载；本地目录需含 config.json</span>
        </el-form-item>
        <el-form-item v-if="aestheticModelPreset === '__custom__'" label="自定义模型">
          <el-input v-model="settings.settings.aesthetic_model" placeholder="HF 仓库名或本地目录路径" />
        </el-form-item>
        <el-form-item label="当前生效模型">
          <span class="mono">{{ inferHealth?.paths?.aesthetic_model || '（服务未启动）' }}</span>
          <span class="hint">{{ aestheticEffectiveSource }}</span>
        </el-form-item>
        <el-form-item label="推理设备">
          <el-select v-model="settings.settings.aesthetic_device" style="width: 260px">
            <el-option v-for="d in aestheticDevices" :key="d.id" :label="d.name" :value="d.id" />
          </el-select>
          <span class="hint">GPU 加速美学评分（下次任务生效）</span>
        </el-form-item>
      </el-form>
    </el-card>

    <!-- 查重设置（本地推理页末尾独立卡片） -->
    <el-card header="查重" shadow="never" class="inf-card">
      <el-form label-width="160px" style="max-width: 720px">
        <el-form-item label="pHash 汉明距离">
          <el-slider
            v-model="settings.settings.dedup_hamming"
            :min="0"
            :max="64"
            show-input
            style="width: 260px"
          />
          <span class="hint">查重分组阈值（默认 8）</span>
        </el-form-item>
      </el-form>
    </el-card>

    <!-- GPU 手动安装教程对话框 -->
    <el-dialog
      v-model="gpuTutorialVisible"
      title="GPU 推理环境 · 手动安装教程"
      width="640px"
      append-to-body
    >
      <pre class="gpu-tutorial">{{ gpuTutorialText }}</pre>
      <template #footer>
        <el-button @click="gpuTutorialVisible = false">关闭</el-button>
        <el-button type="primary" plain @click="copyGpuTutorial">复制教程</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.inf-card {
  margin-bottom: 12px;
  max-width: 900px;
}
.infer-status-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
}
.infer-alert {
  margin-bottom: 10px;
}
.gpu-tutorial {
  white-space: pre-wrap;
  font-size: 12px;
  line-height: 1.6;
  margin: 0;
  max-height: 420px;
  overflow: auto;
}
</style>
