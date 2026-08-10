<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { ElMessage } from 'element-plus'
import { get, post, del, put } from '@/api/client'
import { useSettingsStore, type SauceKeyConfig } from '@/stores/settings'

const settings = useSettingsStore()
const activeTab = ref('saucenao')

// ---- SauceNAO 多 key ----
const newKey = ref('')
const newKeyName = ref('')
const newKeyTier = ref('free')
const keys = ref<SauceKeyConfig[]>([])
const keyStatuses = ref<Array<Record<string, unknown>>>([])
const manageVisible = ref(false)
const saving = ref(false)

// ---- 打标模型 ----
const taggerModelOptions = [
  { name: 'cl_tagger (SIGLIP2 ONNX)', dir: 'D:/Game/AI/cl_tagger/models' },
  { name: '自定义目录', dir: '' },
]

async function loadKeys() {
  try {
    const d = await get<{ keys: SauceKeyConfig[]; count: number }>('/settings/saucenao-keys')
    keys.value = d.keys
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function loadKeyStatuses() {
  try {
    const d = await get<{ keys: Array<Record<string, unknown>> }>('/tagging/keys')
    keyStatuses.value = d.keys
  } catch {
    keyStatuses.value = []
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
    await loadKeyStatuses()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function removeKey(name: string) {
  try {
    await del(`/settings/saucenao-keys/${name}`)
    ElMessage.success(`已删除 ${name}`)
    await loadKeys()
    await loadKeyStatuses()
  } catch (e) {
    ElMessage.error((e as Error).message)
  }
}

async function openManage() {
  manageVisible.value = true
  await loadKeyStatuses()
}

async function saveSettings() {
  saving.value = true
  try {
    await settings.save()
    ElMessage.success('设置已保存')
  } catch (e) {
    ElMessage.error((e as Error).message)
  } finally {
    saving.value = false
  }
}

function onModelSelect(name: string) {
  const opt = taggerModelOptions.find((o) => o.name === name)
  if (opt && opt.dir) {
    settings.settings.tagger_model_dir = opt.dir
  }
}

onMounted(async () => {
  await settings.load()
  await loadKeys()
})
</script>

<template>
  <div class="settings-page">
    <el-tabs v-model="activeTab">
      <el-tab-pane label="图库" name="library">
        <el-form label-width="160px" style="max-width: 720px">
          <el-form-item label="库目录">
            <el-input v-model="settings.settings.library_dir" placeholder="data/library" />
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

      <el-tab-pane label="打标" name="tagging">
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
        </el-form>
      </el-tab-pane>

      <el-tab-pane label="查重" name="dedup">
        <el-form label-width="160px" style="max-width: 720px">
          <el-form-item label="pHash 汉明距离阈值">
            <el-slider v-model="settings.settings.dedup_hamming" :min="0" :max="64" show-input style="width: 260px" />
          </el-form-item>
        </el-form>
      </el-tab-pane>

      <el-tab-pane label="美学" name="aesthetic">
        <el-form label-width="160px" style="max-width: 720px">
          <el-form-item label="模型">
            <el-input v-model="settings.settings.aesthetic_model" />
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
    </el-tabs>

    <div class="save-bar">
      <el-button type="primary" :loading="saving" @click="saveSettings">保存设置</el-button>
      <el-button @click="settings.reset()">恢复默认</el-button>
    </div>

    <!-- 管理密钥弹窗：显示各 key 的等级与当前额度 -->
    <el-dialog v-model="manageVisible" title="管理密钥" width="560px">
      <el-table :data="keyStatuses" size="small">
        <el-table-column prop="name" label="名称" width="100" />
        <el-table-column label="等级" width="80">
          <template #default="{ row }">
            <el-tag :type="row.tier === 'member' ? 'primary' : 'info'" size="small">
              {{ row.tier === 'member' ? '付费' : '免费' }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column prop="key_masked" label="密钥" width="140" />
        <el-table-column label="当日额度" width="100">
          <template #default="{ row }">
            <span :class="{ warn: (row.long_remaining as number) < 10 }">
              {{ row.long_remaining }}
            </span>
          </template>
        </el-table-column>
        <el-table-column label="状态" width="90">
          <template #default="{ row }">
            <el-tag v-if="row.daily_paused" type="danger" size="small">已停用</el-tag>
            <el-tag v-else-if="row.cooldown_secs > 0" type="warning" size="small">冷却中</el-tag>
            <el-tag v-else type="success" size="small">可用</el-tag>
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
.warn {
  color: var(--el-color-danger);
  font-weight: 600;
}
</style>
