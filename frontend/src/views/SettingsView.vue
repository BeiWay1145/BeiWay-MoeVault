<script setup lang="ts">
import { ref } from 'vue'
import { ElMessage } from 'element-plus'
import { useSettingsStore } from '@/stores/settings'

const settings = useSettingsStore()
const activeTab = ref('library')
const saucenaoTest = ref(false)

function testSaucenao() {
  if (!settings.settings.saucenaoKey) {
    ElMessage.warning('请先填写 API key')
    return
  }
  saucenaoTest.value = true
  // TODO(backend): POST /api/v1/settings/test-saucenao
  setTimeout(() => {
    saucenaoTest.value = false
    ElMessage.success('API key 有效（骨架占位，未实际请求）')
  }, 600)
}

function save() {
  // TODO(backend): PUT /api/v1/settings
  ElMessage.success('设置已保存（骨架占位，仅前端内存）')
}
</script>

<template>
  <div class="settings-page">
    <el-tabs v-model="activeTab">
      <el-tab-pane label="图库" name="library">
        <el-form label-width="160px" style="max-width: 720px">
          <el-form-item label="库目录">
            <el-input v-model="settings.settings.libraryDir" placeholder="data/library" />
          </el-form-item>
          <el-form-item label="缩略图规格">
            <el-input-number :model-value="512" :min="256" :max="1024" :step="128" /> px（card）
          </el-form-item>
        </el-form>
      </el-tab-pane>

      <el-tab-pane label="SauceNAO" name="saucenao">
        <el-form label-width="160px" style="max-width: 720px">
          <el-form-item label="API key">
            <el-input v-model="settings.settings.saucenaoKey" type="password" show-password style="width: 320px" />
            <el-button style="margin-left: 8px" :loading="saucenaoTest" @click="testSaucenao">测试</el-button>
          </el-form-item>
          <el-form-item label="相似度阈值">
            <el-slider v-model="settings.settings.saucenaoSimilarity" :min="0" :max="100" show-input style="width: 260px" /> %
          </el-form-item>
          <el-form-item label="限流等级">
            <el-select :model-value="'free'" style="width: 200px">
              <el-option label="免费 API（约 30s/次）" value="free" />
              <el-option label="会员 API（约 2s/次）" value="member" />
            </el-select>
          </el-form-item>
        </el-form>
      </el-tab-pane>

      <el-tab-pane label="打标" name="tagging">
        <el-form label-width="160px" style="max-width: 720px">
          <el-form-item label="置信度阈值">
            <el-slider v-model="settings.settings.tagThreshold" :min="0" :max="1" :step="0.05" show-input style="width: 260px" />
          </el-form-item>
        </el-form>
      </el-tab-pane>

      <el-tab-pane label="查重" name="dedup">
        <el-form label-width="160px" style="max-width: 720px">
          <el-form-item label="pHash 汉明距离阈值">
            <el-slider v-model="settings.settings.dedupHamming" :min="0" :max="64" show-input style="width: 260px" />
          </el-form-item>
          <el-form-item>
            <el-button type="danger" plain>全库重扫（骨架占位）</el-button>
          </el-form-item>
        </el-form>
      </el-tab-pane>

      <el-tab-pane label="美学" name="aesthetic">
        <el-form label-width="160px" style="max-width: 720px">
          <el-form-item label="模型">
            <el-input v-model="settings.settings.aestheticModel" />
          </el-form-item>
          <el-form-item>
            <el-button type="danger" plain>全量重算（骨架占位）</el-button>
          </el-form-item>
        </el-form>
      </el-tab-pane>

      <el-tab-pane label="回收站 / sidecar" name="misc">
        <el-form label-width="160px" style="max-width: 720px">
          <el-form-item label="自动清空天数">
            <el-input-number v-model="settings.settings.recycleDays" :min="0" :max="365" />
            <span class="hint">0 = 不自动清空</span>
          </el-form-item>
          <el-form-item label="sidecar .txt">
            <el-switch v-model="settings.settings.sidecarEnabled" active-text="开启" inactive-text="关闭" />
          </el-form-item>
          <el-form-item label="中文字典">
            <el-switch v-model="settings.settings.cnDictEnabled" active-text="开启" inactive-text="关闭" />
          </el-form-item>
        </el-form>
      </el-tab-pane>
    </el-tabs>

    <div class="save-bar">
      <el-button type="primary" @click="save">保存设置</el-button>
      <el-button @click="settings.reset()">恢复默认</el-button>
    </div>
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
</style>
