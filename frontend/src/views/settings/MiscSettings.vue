<script setup lang="ts">
import { useSettingsStore } from '@/stores/settings'

const settings = useSettingsStore()
</script>

<template>
  <el-form label-width="160px" style="max-width: 720px">
    <el-form-item label="自动清空天数">
      <el-input-number v-model="settings.settings.recycle_days" :min="0" :max="365" />
      <span class="hint">0 = 不自动清空</span>
    </el-form-item>
    <el-form-item label="sidecar .txt">
      <el-switch v-model="settings.settings.sidecar_enabled" active-text="开启" inactive-text="关闭" />
    </el-form-item>
  </el-form>
</template>
