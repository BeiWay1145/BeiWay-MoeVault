<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRoute, onBeforeRouteLeave } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { useSettingsStore } from '@/stores/settings'
import { reportLog } from '@/api/log'
// 六个设置页各自独立成组件：状态互不干扰，也避免单文件膨胀到 1500+ 行
import GeneralSettings from './settings/GeneralSettings.vue'
import SauceSettings from './settings/SauceSettings.vue'
import InferenceSettings from './settings/InferenceSettings.vue'
import MiscSettings from './settings/MiscSettings.vue'
import TagsSettings from './settings/TagsSettings.vue'
import LogSettingsPanel from './settings/LogSettingsPanel.vue'

const settings = useSettingsStore()
const route = useRoute()
// 默认打开「通用」设置页；支持 ?tab=inference 直达本地推理状态
const activeTab = ref('library')
if (route.query.tab === 'inference') {
  activeTab.value = 'inference'
}

const saving = ref(false)

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

// ---- 未保存设置离开提醒 ----
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
  const action = await ElMessageBox.confirm('设置尚未保存，离开将丢失修改。', '未保存的设置', {
    confirmButtonText: '保存并离开',
    cancelButtonText: '放弃修改',
    distinguishCancelAndClose: true,
    type: 'warning',
  })
    .then(() => 'save' as const)
    .catch((a: string | 'cancel' | 'close') =>
      a === 'cancel' ? ('discard' as const) : ('stay' as const),
    )
  if (action === 'save') {
    await saveSettings()
    return true
  }
  if (action === 'discard') return true
  return false
})

onMounted(async () => {
  // 各子组件自行加载自己的数据（密钥/出口、推理设备与健康、字典缓存、日志设置）
  await settings.load()
})
</script>

<template>
  <div class="settings-page">
    <el-tabs v-model="activeTab">
      <el-tab-pane label="通用" name="library">
        <GeneralSettings />
      </el-tab-pane>

      <el-tab-pane label="SauceNAO" name="saucenao">
        <SauceSettings />
      </el-tab-pane>

      <el-tab-pane label="本地推理" name="inference">
        <InferenceSettings />
      </el-tab-pane>

      <el-tab-pane label="回收站 / sidecar" name="misc">
        <MiscSettings />
      </el-tab-pane>

      <el-tab-pane label="标签" name="tags">
        <TagsSettings />
      </el-tab-pane>

      <el-tab-pane label="BUG追踪器" name="logs">
        <!-- active 显式传入：日志定时器要随 tab 切换启停，不能靠父级 activeTab 变量 -->
        <LogSettingsPanel :active="activeTab === 'logs'" />
      </el-tab-pane>
    </el-tabs>

    <div class="save-bar">
      <el-button type="primary" :loading="saving" @click="saveSettings">保存设置</el-button>
      <el-button @click="settings.reset()">恢复默认</el-button>
    </div>
  </div>
</template>

<style scoped>
.settings-page {
  max-width: 960px;
}
.save-bar {
  margin-top: 16px;
}
</style>

<style>
/*
 * .hint 是设置页跨多个子组件共用的弱提示样式。
 * 必须放在非 scoped 块里：scoped 样式只作用于本组件自己的模板元素，
 * 不会进入子组件内部（子组件根节点除外），否则拆出去的六个面板会全部丢失该样式。
 * 用 .settings-page 前缀限定作用范围，不影响应用其它区域。
 */
.settings-page .hint {
  margin-left: 8px;
  color: var(--el-text-color-secondary);
  font-size: 12px;
}
.settings-page .hint-warn {
  color: var(--el-color-warning);
}
.settings-page .mono {
  font-family: monospace;
  font-size: 12px;
  word-break: break-all;
}
</style>
